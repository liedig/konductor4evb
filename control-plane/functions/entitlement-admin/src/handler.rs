//! Entitlement administration behind the OAuth surface: create, list, and
//! revoke with dual authorization (platform-admin, or a publisher binding on
//! the producer domain; consumer-scoped lists also allow a subscriber binding
//! on the queried consumer domain). Every mutation commits with its token-free
//! audit record in one transaction.

use anyhow::{Context, Result};
use chrono::Utc;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use konductor_common::api;
use konductor_common::entitlement::{
    Entitlement, EntitlementAuditRecord, EntitlementSummary, EntitlementView,
    ListEntitlementsResponse,
};
use konductor_common::entitlement_store::{EntitlementStore, EntitlementStoreError};
use konductor_common::error::ControlPlaneError;
use konductor_common::grant::AuditMutation;
use konductor_common::grant_engine::has_role_binding;
use konductor_common::grant_store::BindingStore;
use konductor_common::principal::{AuthenticationMechanism, TrustedPrincipal};
use konductor_common::principal_context::log_principal;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let entitlements_table =
        std::env::var("ENTITLEMENTS_TABLE").context("ENTITLEMENTS_TABLE not set")?;
    let bindings_table = std::env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    let entitlements = EntitlementStore::new(ddb.clone(), entitlements_table);
    let bindings = BindingStore::new(ddb.clone(), bindings_table);
    dispatch(&entitlements, &bindings, &event.payload).await
}

fn access_denied() -> Value {
    api::from_control_plane_error(&ControlPlaneError::AccessDenied)
}

async fn dispatch(
    entitlements: &EntitlementStore,
    bindings: &BindingStore,
    payload: &Value,
) -> Result<Value, Error> {
    let request_context = payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(_) => return Ok(access_denied()),
    };
    // Entitlement administration is OAuth-only: the IAM surface exposes no
    // entitlement routes, so an IAM principal here means misrouting.
    if principal.authentication_mechanism() != AuthenticationMechanism::Oauth {
        return Ok(access_denied());
    }

    let method = payload
        .get("httpMethod")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let resource = payload
        .get("resource")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match (method, resource) {
        ("POST", "/entitlements") => create(entitlements, bindings, payload, &principal).await,
        ("GET", "/entitlements") => list(entitlements, bindings, payload, &principal).await,
        ("DELETE", "/entitlements/{entitlementId}") => {
            revoke(entitlements, bindings, payload, &principal).await
        }
        _ => Ok(access_denied()),
    }
}

async fn can_administer(
    bindings: &BindingStore,
    principal: &TrustedPrincipal,
    producer_domain: &str,
) -> bool {
    principal.is_platform_admin()
        || has_role_binding(
            bindings,
            principal,
            konductor_common::grant::ResourceRole::Publisher,
            producer_domain,
        )
        .await
}

async fn create(
    entitlements: &EntitlementStore,
    bindings: &BindingStore,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let request: konductor_common::entitlement::CreateEntitlementRequest =
        match api::parse_body(payload) {
            Ok(r) => r,
            Err(e) => return Ok(api::from_control_plane_error(&e)),
        };
    let entitlement = match request.into_entitlement(Utc::now(), principal.principal_id()) {
        Ok(e) => e,
        Err(e) => {
            return Ok(api::from_control_plane_error(&api::validation_error(
                e.to_string(),
            )))
        }
    };
    if !can_administer(bindings, principal, &entitlement.producer_domain).await {
        return Ok(access_denied());
    }
    let audit = audit_record(&entitlement, AuditMutation::Create, principal);

    match entitlements.create_entitlement(&entitlement, &audit).await {
        Ok(()) => Ok(api::success_response(
            201,
            &EntitlementView::from(&entitlement),
        )),
        Err(EntitlementStoreError::Conflict) => {
            idempotent_recreate(entitlements, &entitlement).await
        }
        Err(e) => Ok(store_failure("create entitlement", e)),
    }
}

/// A create that lost the conditional check raced either an identical
/// entitlement (idempotent re-create: answer with the stored one) or a
/// concurrent revoke (answer 409 so the admin retries against a settled
/// table).
async fn idempotent_recreate(
    entitlements: &EntitlementStore,
    entitlement: &Entitlement,
) -> Result<Value, Error> {
    match entitlements
        .find_entitlement_by_id(&entitlement.entitlement_id)
        .await
    {
        Ok(Some(existing)) => Ok(api::success_response(
            200,
            &EntitlementView::from(&existing),
        )),
        Ok(None) => Ok(api::from_control_plane_error(
            &ControlPlaneError::EntitlementConflict {
                reason: "entitlement changed concurrently; retry".to_string(),
            },
        )),
        Err(e) => Ok(store_failure("re-read entitlement after conflict", e)),
    }
}

async fn list(
    entitlements: &EntitlementStore,
    bindings: &BindingStore,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let consumer = api::query_param(payload, "consumerDomain").filter(|d| !d.is_empty());
    let producer = api::query_param(payload, "producerDomain").filter(|d| !d.is_empty());
    match (consumer, producer) {
        (Some(consumer), None) => {
            let allowed = principal.is_platform_admin()
                || has_role_binding(
                    bindings,
                    principal,
                    konductor_common::grant::ResourceRole::Subscriber,
                    &consumer,
                )
                .await
                || has_role_binding(
                    bindings,
                    principal,
                    konductor_common::grant::ResourceRole::Publisher,
                    &consumer,
                )
                .await;
            if !allowed {
                return Ok(access_denied());
            }
            match entitlements.entitlements_for_consumer(&consumer).await {
                Ok(found) => Ok(list_response(&found)),
                Err(e) => Ok(store_failure("list entitlements by consumer", e)),
            }
        }
        (None, Some(producer)) => {
            if !can_administer(bindings, principal, &producer).await {
                return Ok(access_denied());
            }
            match entitlements.list_entitlements_for_producer(&producer).await {
                Ok(found) => Ok(list_response(&found)),
                Err(e) => Ok(store_failure("list entitlements by producer", e)),
            }
        }
        _ => Ok(api::from_control_plane_error(&api::validation_error(
            "exactly one of 'consumerDomain' or 'producerDomain' is required",
        ))),
    }
}

fn list_response(entitlements: &[Entitlement]) -> Value {
    api::success_response(
        200,
        &ListEntitlementsResponse {
            entitlements: entitlements.iter().map(EntitlementView::from).collect(),
        },
    )
}

async fn revoke(
    entitlements: &EntitlementStore,
    bindings: &BindingStore,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let entitlement_id = match api::path_param(payload, "entitlementId").filter(|id| !id.is_empty())
    {
        Some(id) => id,
        None => {
            return Ok(api::from_control_plane_error(&api::validation_error(
                "path parameter 'entitlementId' is required",
            )))
        }
    };
    let existing = match entitlements.find_entitlement_by_id(&entitlement_id).await {
        Ok(Some(e)) => e,
        Ok(None) => {
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::EntitlementNotFound {
                    entitlement_id: entitlement_id.clone(),
                },
            ))
        }
        Err(e) => return Ok(store_failure("locate entitlement", e)),
    };
    if !can_administer(bindings, principal, &existing.producer_domain).await {
        return Ok(access_denied());
    }
    let audit = audit_record(&existing, AuditMutation::Revoke, principal);

    match entitlements.revoke_entitlement(&existing, &audit).await {
        Ok(()) => Ok(api::success_response(
            200,
            &json!({"entitlementId": entitlement_id, "state": "revoked"}),
        )),
        Err(EntitlementStoreError::NotFound) => Ok(api::from_control_plane_error(
            &ControlPlaneError::EntitlementNotFound {
                entitlement_id: entitlement_id.clone(),
            },
        )),
        Err(e) => Ok(store_failure("revoke entitlement", e)),
    }
}

fn audit_record(
    entitlement: &Entitlement,
    mutation: AuditMutation,
    principal: &TrustedPrincipal,
) -> EntitlementAuditRecord {
    let summary = EntitlementSummary::from(entitlement);
    let (before, after) = match mutation {
        AuditMutation::Create => (None, Some(summary)),
        AuditMutation::Revoke => (Some(summary), None),
    };
    EntitlementAuditRecord {
        entitlement_id: entitlement.entitlement_id.clone(),
        timestamp: Utc::now(),
        mutation,
        actor: principal.principal_id().to_string(),
        before,
        after,
    }
}

fn store_failure(operation: &str, error: EntitlementStoreError) -> Value {
    // Debug format: SdkError's Display is terse ("service error") and hides
    // the underlying service error kind needed to diagnose live failures.
    tracing::error!(operation, error = ?error, "entitlements store operation failed");
    api::from_control_plane_error(&ControlPlaneError::DynamoDb(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::get_item::GetItemInput;
    use aws_sdk_dynamodb::operation::query::QueryInput;
    use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsInput;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use konductor_common::dynamodb::get_s;
    use konductor_common::entitlement::ScopePattern;
    use konductor_common::entitlement_store::entitlement_to_item;
    use konductor_common::grant::{BindingPrincipal, ResourceRole, RoleBinding};
    use konductor_common::grant_store::binding_to_item;
    use std::sync::{Arc, Mutex};

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";
    const ADMIN_SUB: &str = "a1b2c3d4-1111-2222-3333-444455556666";
    const PUBLISHER_SUB: &str = "b2c3d4e5-2222-3333-4444-555566667777";

    fn admin_context() -> Value {
        oauth_context(ADMIN_SUB, "[\"platform-admin\"]", "[]")
    }

    fn publisher_context() -> Value {
        oauth_context(PUBLISHER_SUB, "[]", "[\"orders-publishers\"]")
    }

    fn subscriber_context() -> Value {
        oauth_context(
            "c3d4e5f6-3333-4444-5555-666677778888",
            "[]",
            "[\"checkout-subscribers\"]",
        )
    }

    fn oauth_context(sub: &str, global_roles: &str, team_groups: &str) -> Value {
        json!({
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": sub,
                "issuer": ISSUER,
                "clientId": "client123",
                "globalRoles": global_roles,
                "teamGroups": team_groups
            }
        })
    }

    fn event(method: &str, resource: &str, ctx: Value) -> Value {
        json!({
            "httpMethod": method,
            "resource": resource,
            "requestContext": ctx,
        })
    }

    fn create_body(consumer: &str, producer: &str, pattern: &str) -> String {
        json!({
            "consumerDomain": consumer,
            "producerDomain": producer,
            "scopePattern": pattern
        })
        .to_string()
    }

    fn existing_entitlement() -> Entitlement {
        Entitlement::new(
            "checkout",
            "orders",
            ScopePattern::new("Order*").unwrap(),
            Utc::now(),
            ADMIN_SUB,
        )
        .unwrap()
    }

    fn role_binding(group: &str, role: ResourceRole, domain: &str) -> RoleBinding {
        RoleBinding::new(
            "rb-fixture",
            BindingPrincipal::cognito_group(ISSUER, group).unwrap(),
            role,
            domain,
            Utc::now(),
            "fixture",
        )
        .unwrap()
    }

    fn binding_rule(binding: &RoleBinding) -> Rule {
        let item = binding_to_item(binding);
        let pk = binding.pk();
        let sk = binding.sk();
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(move |req: &GetItemInput| {
                let key = req.key();
                let has_key = |name: &str, want: &String| {
                    key.and_then(|k| k.get(name)).and_then(|v| v.as_s().ok()) == Some(want)
                };
                req.consistent_read() == Some(true) && has_key("PK", &pk) && has_key("SK", &sk)
            })
            .then_output(move || {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .set_item(Some(item.clone()))
                    .build()
            })
    }

    fn empty_get_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build())
    }

    fn transact_ok() -> Rule {
        mock!(aws_sdk_dynamodb::Client::transact_write_items)
            .then_output(|| aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsOutput::builder().build())
    }

    fn transact_conditional_failure() -> Rule {
        mock!(aws_sdk_dynamodb::Client::transact_write_items).then_error(|| {
            let reason = aws_sdk_dynamodb::types::CancellationReason::builder()
                .code("ConditionalCheckFailed")
                .build();
            aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError::TransactionCanceledException(
                aws_sdk_dynamodb::types::error::TransactionCanceledException::builder()
                    .cancellation_reasons(reason)
                    .build(),
            )
        })
    }

    fn by_id_rule(entitlement: &Entitlement) -> Rule {
        let item = entitlement_to_item(entitlement);
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req: &QueryInput| req.index_name() == Some("by-entitlement-id"))
            .then_output(move || {
                aws_sdk_dynamodb::operation::query::QueryOutput::builder()
                    .items(item.clone())
                    .build()
            })
    }

    fn stores_with(rules: Vec<Rule>) -> (EntitlementStore, BindingStore) {
        let client = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, &rules);
        (
            EntitlementStore::new(client.clone(), "entitlements-test"),
            BindingStore::new(client, "bindings-test"),
        )
    }

    async fn invoke(stores: &(EntitlementStore, BindingStore), payload: Value) -> Value {
        dispatch(&stores.0, &stores.1, &payload).await.unwrap()
    }

    fn status(resp: &Value) -> u16 {
        resp["statusCode"].as_u64().unwrap() as u16
    }

    fn body(resp: &Value) -> Value {
        serde_json::from_str(resp["body"].as_str().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn platform_admin_creates_with_server_assigned_fields() {
        let stores = stores_with(vec![transact_ok()]);
        let mut payload = event("POST", "/entitlements", admin_context());
        payload["body"] = json!(create_body("checkout", "orders", "Order*"));

        let resp = invoke(&stores, payload).await;
        assert_eq!(status(&resp), 201);
        let body = body(&resp);
        let expected_id = Entitlement::new(
            "checkout",
            "orders",
            ScopePattern::new("Order*").unwrap(),
            Utc::now(),
            "anyone",
        )
        .unwrap()
        .entitlement_id;
        assert_eq!(body["entitlementId"], expected_id);
        assert_eq!(body["consumerDomain"], "checkout");
        assert_eq!(body["producerDomain"], "orders");
        assert_eq!(body["scopePattern"], "Order*");
        assert_eq!(body["createdBy"], ADMIN_SUB);
        assert!(body["createdAt"].as_str().is_some());
    }

    #[tokio::test]
    async fn create_writes_entitlement_and_audit_in_one_transaction() {
        let captured: Arc<Mutex<Vec<TransactWriteItemsInput>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&captured);
        let rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
            .match_requests(move |req: &TransactWriteItemsInput| {
                seen.lock().unwrap().push(req.clone());
                true
            })
            .then_output(|| aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsOutput::builder().build());
        let stores = stores_with(vec![rule]);

        let mut payload = event("POST", "/entitlements", admin_context());
        payload["body"] = json!(create_body("checkout", "orders", "Order*"));
        assert_eq!(status(&invoke(&stores, payload).await), 201);

        let calls = captured.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let items = calls[0].transact_items();
        assert_eq!(items.len(), 2);

        let put = items[0].put().expect("first item is the entitlement put");
        assert_eq!(
            put.condition_expression().unwrap(),
            "attribute_not_exists(PK)"
        );
        let put_item = put.item();
        assert_eq!(get_s(put_item, "itemType").unwrap(), "Entitlement");
        assert_eq!(get_s(put_item, "PK").unwrap(), "CONSUMER#checkout");
        assert_eq!(get_s(put_item, "GSI1PK").unwrap(), "PRODUCER#orders");
        assert_eq!(get_s(put_item, "scopePattern").unwrap(), "Order*");
        assert_eq!(get_s(put_item, "createdBy").unwrap(), ADMIN_SUB);

        let audit = items[1].put().expect("second item is the audit put");
        let audit_item = audit.item();
        assert_eq!(
            get_s(audit_item, "itemType").unwrap(),
            "EntitlementAuditRecord"
        );
        assert_eq!(get_s(audit_item, "mutation").unwrap(), "create");
        assert_eq!(get_s(audit_item, "actor").unwrap(), ADMIN_SUB);
        let audit_debug: String = audit_item
            .iter()
            .map(|(k, v)| format!("{k}={v:?};"))
            .collect();
        assert!(!audit_debug.contains("token"), "audit must be token-free");
        assert!(
            !audit_debug.contains("claims"),
            "audit must not carry claims material"
        );
    }

    #[tokio::test]
    async fn publisher_self_service_is_scoped_to_its_own_producer_domain() {
        let orders_publisher = role_binding("orders-publishers", ResourceRole::Publisher, "orders");
        let stores = stores_with(vec![
            binding_rule(&orders_publisher),
            transact_ok(),
            empty_get_rule(),
        ]);

        let mut own = event("POST", "/entitlements", publisher_context());
        own["body"] = json!(create_body("checkout", "orders", "Order*"));
        assert_eq!(status(&invoke(&stores, own).await), 201);

        let mut other = event("POST", "/entitlements", publisher_context());
        other["body"] = json!(create_body("checkout", "payments", "*"));
        assert_eq!(status(&invoke(&stores, other).await), 403);
    }

    #[tokio::test]
    async fn idempotent_recreate_returns_existing_entitlement() {
        let existing = existing_entitlement();
        let stores = stores_with(vec![transact_conditional_failure(), by_id_rule(&existing)]);

        let mut payload = event("POST", "/entitlements", admin_context());
        payload["body"] = json!(create_body("checkout", "orders", "Order*"));

        let resp = invoke(&stores, payload).await;
        assert_eq!(status(&resp), 200);
        assert_eq!(body(&resp)["entitlementId"], existing.entitlement_id);
    }

    #[tokio::test]
    async fn create_conflict_without_stored_entitlement_maps_to_409() {
        let empty_query = mock!(aws_sdk_dynamodb::Client::query)
            .then_output(|| aws_sdk_dynamodb::operation::query::QueryOutput::builder().build());
        let stores = stores_with(vec![transact_conditional_failure(), empty_query]);

        let mut payload = event("POST", "/entitlements", admin_context());
        payload["body"] = json!(create_body("checkout", "orders", "Order*"));

        let resp = invoke(&stores, payload).await;
        assert_eq!(status(&resp), 409);
        assert_eq!(body(&resp)["code"], "ConflictException");
    }

    #[tokio::test]
    async fn list_by_consumer_allows_admin_and_bound_subscriber_only() {
        let existing = existing_entitlement();
        let item = entitlement_to_item(&existing);
        let consumer_query = mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req: &QueryInput| {
                req.consistent_read() == Some(true) && req.index_name().is_none()
            })
            .then_output(move || {
                aws_sdk_dynamodb::operation::query::QueryOutput::builder()
                    .items(item.clone())
                    .build()
            });
        let subscriber = role_binding("checkout-subscribers", ResourceRole::Subscriber, "checkout");
        let stores = stores_with(vec![
            consumer_query,
            binding_rule(&subscriber),
            empty_get_rule(),
        ]);

        let mut admin_list = event("GET", "/entitlements", admin_context());
        admin_list["queryStringParameters"] = json!({"consumerDomain": "checkout"});
        let resp = invoke(&stores, admin_list).await;
        assert_eq!(status(&resp), 200);
        let entitlements = body(&resp)["entitlements"].as_array().unwrap().clone();
        assert_eq!(entitlements.len(), 1);
        assert_eq!(entitlements[0]["entitlementId"], existing.entitlement_id);

        let mut subscriber_list = event("GET", "/entitlements", subscriber_context());
        subscriber_list["queryStringParameters"] = json!({"consumerDomain": "checkout"});
        assert_eq!(status(&invoke(&stores, subscriber_list).await), 200);

        let mut outsider_list = event("GET", "/entitlements", publisher_context());
        outsider_list["queryStringParameters"] = json!({"consumerDomain": "checkout"});
        assert_eq!(status(&invoke(&stores, outsider_list).await), 403);
    }

    #[tokio::test]
    async fn list_by_producer_allows_admin_and_bound_publisher_only() {
        let existing = existing_entitlement();
        let item = entitlement_to_item(&existing);
        let producer_query = mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req: &QueryInput| req.index_name() == Some("by-producer"))
            .then_output(move || {
                aws_sdk_dynamodb::operation::query::QueryOutput::builder()
                    .items(item.clone())
                    .build()
            });
        let publisher = role_binding("orders-publishers", ResourceRole::Publisher, "orders");
        let stores = stores_with(vec![
            producer_query,
            binding_rule(&publisher),
            empty_get_rule(),
        ]);

        let mut publisher_list = event("GET", "/entitlements", publisher_context());
        publisher_list["queryStringParameters"] = json!({"producerDomain": "orders"});
        let resp = invoke(&stores, publisher_list).await;
        assert_eq!(status(&resp), 200);
        assert_eq!(body(&resp)["entitlements"].as_array().unwrap().len(), 1);

        let mut outsider_list = event("GET", "/entitlements", subscriber_context());
        outsider_list["queryStringParameters"] = json!({"producerDomain": "orders"});
        assert_eq!(status(&invoke(&stores, outsider_list).await), 403);
    }

    #[tokio::test]
    async fn list_requires_exactly_one_filter() {
        let stores = stores_with(vec![]);

        let missing = event("GET", "/entitlements", admin_context());
        assert_eq!(status(&invoke(&stores, missing).await), 400);

        let mut both = event("GET", "/entitlements", admin_context());
        both["queryStringParameters"] =
            json!({"consumerDomain": "checkout", "producerDomain": "orders"});
        assert_eq!(status(&invoke(&stores, both).await), 400);
    }

    #[tokio::test]
    async fn revoke_locates_authorizes_and_deletes() {
        let existing = existing_entitlement();
        let publisher = role_binding("orders-publishers", ResourceRole::Publisher, "orders");
        let stores = stores_with(vec![
            by_id_rule(&existing),
            binding_rule(&publisher),
            transact_ok(),
        ]);

        let mut payload = event(
            "DELETE",
            "/entitlements/{entitlementId}",
            publisher_context(),
        );
        payload["pathParameters"] = json!({"entitlementId": existing.entitlement_id});
        let resp = invoke(&stores, payload).await;
        assert_eq!(status(&resp), 200);
        assert_eq!(body(&resp)["state"], "revoked");
    }

    #[tokio::test]
    async fn cross_producer_revoke_is_denied_without_touching_the_store() {
        let existing = existing_entitlement();
        let transacts: Arc<Mutex<Vec<()>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&transacts);
        let transact_capture = mock!(aws_sdk_dynamodb::Client::transact_write_items)
            .match_requests(move |_: &TransactWriteItemsInput| {
                seen.lock().unwrap().push(());
                true
            })
            .then_output(|| aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsOutput::builder().build());
        let payments_publisher =
            role_binding("orders-publishers", ResourceRole::Publisher, "payments");
        let stores = stores_with(vec![
            by_id_rule(&existing),
            binding_rule(&payments_publisher),
            transact_capture,
            empty_get_rule(),
        ]);

        let mut payload = event(
            "DELETE",
            "/entitlements/{entitlementId}",
            publisher_context(),
        );
        payload["pathParameters"] = json!({"entitlementId": existing.entitlement_id});
        let resp = invoke(&stores, payload).await;
        assert_eq!(status(&resp), 403);
        assert!(
            transacts.lock().unwrap().is_empty(),
            "a denied revoke must not reach the store"
        );
    }

    #[tokio::test]
    async fn revoke_unknown_entitlement_returns_404() {
        let empty_query = mock!(aws_sdk_dynamodb::Client::query)
            .then_output(|| aws_sdk_dynamodb::operation::query::QueryOutput::builder().build());
        let stores = stores_with(vec![empty_query]);

        let mut payload = event("DELETE", "/entitlements/{entitlementId}", admin_context());
        payload["pathParameters"] = json!({"entitlementId": "ent-missing"});
        let resp = invoke(&stores, payload).await;
        assert_eq!(status(&resp), 404);
        assert_eq!(body(&resp)["code"], "ResourceNotFoundException");
    }

    #[tokio::test]
    async fn revoke_racing_another_revoke_returns_404() {
        let existing = existing_entitlement();
        let stores = stores_with(vec![by_id_rule(&existing), transact_conditional_failure()]);

        let mut payload = event("DELETE", "/entitlements/{entitlementId}", admin_context());
        payload["pathParameters"] = json!({"entitlementId": existing.entitlement_id});
        let resp = invoke(&stores, payload).await;
        assert_eq!(status(&resp), 404);
    }

    #[tokio::test]
    async fn iam_caller_is_denied_on_every_route() {
        let ctx = json!({
            "identity": {"accountId": "111122223333", "userArn": "arn:aws:iam::111122223333:user/ci"}
        });
        let stores = stores_with(vec![]);
        for (method, resource) in [
            ("POST", "/entitlements"),
            ("GET", "/entitlements"),
            ("DELETE", "/entitlements/{entitlementId}"),
        ] {
            let mut payload = event(method, resource, ctx.clone());
            payload["body"] = json!(create_body("checkout", "orders", "*"));
            payload["queryStringParameters"] = json!({"consumerDomain": "checkout"});
            payload["pathParameters"] = json!({"entitlementId": "ent-1"});
            let resp = invoke(&stores, payload).await;
            assert_eq!(
                status(&resp),
                403,
                "{method} {resource} must deny IAM callers"
            );
        }
    }

    #[tokio::test]
    async fn missing_principal_context_is_denied() {
        let stores = stores_with(vec![]);
        let resp = invoke(&stores, event("POST", "/entitlements", json!({}))).await;
        assert_eq!(status(&resp), 403);
    }

    #[tokio::test]
    async fn create_rejects_unknown_fields_and_malformed_patterns() {
        let stores = stores_with(vec![]);
        for body in [
            json!({"consumerDomain": "checkout", "producerDomain": "orders", "scopePattern": "*", "createdBy": "attacker"}),
            json!({"consumerDomain": "checkout", "producerDomain": "orders", "scopePattern": "Or*der"}),
            json!({"consumerDomain": "", "producerDomain": "orders", "scopePattern": "*"}),
        ] {
            let mut payload = event("POST", "/entitlements", admin_context());
            payload["body"] = json!(body.to_string());
            let resp = invoke(&stores, payload).await;
            assert_eq!(status(&resp), 400, "body {body} must be rejected");
        }
    }
}
