use anyhow::{Context, Result};
use chrono::Utc;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};
use uuid::Uuid;

use nexus_common::api;
use nexus_common::error::ControlPlaneError;
use nexus_common::grant::{
    AuditMutation, AuditRecord, BindingSummary, BindingView, CreateBindingRequest,
    ListBindingsResponse, RoleBinding,
};
use nexus_common::grant_store::{BindingStore, GrantStoreError};
use nexus_common::principal::{AuthenticationMechanism, TrustedPrincipal};
use nexus_common::principal_context::log_principal;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = std::env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    let store = BindingStore::new(ddb.clone(), table);
    dispatch(&store, &event.payload).await
}

async fn dispatch(store: &BindingStore, payload: &Value) -> Result<Value, Error> {
    let request_context = payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(_) => {
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ))
        }
    };
    if principal.authentication_mechanism() != AuthenticationMechanism::Oauth
        || !principal.is_platform_admin()
    {
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::AccessDenied,
        ));
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
        ("POST", "/bindings") => create_binding(store, payload, &principal).await,
        ("GET", "/bindings") => list_bindings(store, payload).await,
        ("DELETE", "/bindings/{bindingId}") => revoke_binding(store, payload, &principal).await,
        _ => Ok(api::from_control_plane_error(
            &ControlPlaneError::BindingNotFound {
                binding_id: format!("{method} {resource}"),
            },
        )),
    }
}

async fn create_binding(
    store: &BindingStore,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let request: CreateBindingRequest = match api::parse_body(payload) {
        Ok(r) => r,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    let binding = match request.into_binding(
        Uuid::new_v4().to_string(),
        Utc::now(),
        principal.principal_id(),
    ) {
        Ok(b) => b,
        Err(e) => {
            return Ok(api::from_control_plane_error(&api::validation_error(
                e.to_string(),
            )))
        }
    };
    let audit = audit_record(&binding, AuditMutation::Create, principal);

    match store.create_binding(&binding, &audit).await {
        Ok(()) => Ok(api::success_response(201, &BindingView::from(&binding))),
        Err(GrantStoreError::Conflict) => idempotent_recreate(store, &binding).await,
        Err(e) => Ok(store_failure("create binding", e)),
    }
}

/// A create that lost the conditional check raced either an identical binding
/// (idempotent re-create: answer with the stored one) or a concurrent revoke
/// (answer 409 so the admin retries against a settled table).
async fn idempotent_recreate(store: &BindingStore, binding: &RoleBinding) -> Result<Value, Error> {
    match store.get_binding(&binding.pk(), &binding.sk()).await {
        Ok(Some(existing)) => Ok(api::success_response(200, &BindingView::from(&existing))),
        Ok(None) => Ok(api::from_control_plane_error(
            &ControlPlaneError::BindingConflict {
                reason: "binding changed concurrently; retry".to_string(),
            },
        )),
        Err(e) => Ok(store_failure("re-read binding after conflict", e)),
    }
}

async fn list_bindings(store: &BindingStore, payload: &Value) -> Result<Value, Error> {
    let domain = match api::query_param(payload, "domain").filter(|d| !d.is_empty()) {
        Some(d) => d,
        None => {
            return Ok(api::from_control_plane_error(&api::validation_error(
                "query parameter 'domain' is required",
            )))
        }
    };
    match store.list_bindings_for_domain(&domain).await {
        Ok(bindings) => Ok(api::success_response(
            200,
            &ListBindingsResponse {
                bindings: bindings.iter().map(BindingView::from).collect(),
            },
        )),
        Err(e) => Ok(store_failure("list bindings", e)),
    }
}

async fn revoke_binding(
    store: &BindingStore,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let binding_id = match api::path_param(payload, "bindingId").filter(|id| !id.is_empty()) {
        Some(id) => id,
        None => {
            return Ok(api::from_control_plane_error(&api::validation_error(
                "path parameter 'bindingId' is required",
            )))
        }
    };
    let existing = match store.find_binding_by_id(&binding_id).await {
        Ok(Some(b)) => b,
        Ok(None) => {
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::BindingNotFound {
                    binding_id: binding_id.clone(),
                },
            ))
        }
        Err(e) => return Ok(store_failure("locate binding", e)),
    };
    let audit = audit_record(&existing, AuditMutation::Revoke, principal);

    match store.revoke_binding(&existing, &audit).await {
        Ok(()) => Ok(api::success_response(
            200,
            &json!({"bindingId": binding_id, "state": "revoked"}),
        )),
        Err(GrantStoreError::NotFound) => Ok(api::from_control_plane_error(
            &ControlPlaneError::BindingNotFound {
                binding_id: binding_id.clone(),
            },
        )),
        Err(e) => Ok(store_failure("revoke binding", e)),
    }
}

fn audit_record(
    binding: &RoleBinding,
    mutation: AuditMutation,
    principal: &TrustedPrincipal,
) -> AuditRecord {
    let summary = BindingSummary::from(binding);
    let (before, after) = match mutation {
        AuditMutation::Create => (None, Some(summary)),
        AuditMutation::Revoke => (Some(summary), None),
    };
    AuditRecord {
        binding_id: binding.binding_id.clone(),
        timestamp: Utc::now(),
        mutation,
        actor: principal.principal_id().to_string(),
        before,
        after,
    }
}

fn store_failure(operation: &str, error: GrantStoreError) -> Value {
    tracing::error!(operation, error = %error, "bindings store operation failed");
    api::from_control_plane_error(&ControlPlaneError::DynamoDb(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::get_item::GetItemInput;
    use aws_sdk_dynamodb::operation::query::QueryInput;
    use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsInput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use nexus_common::dynamodb::get_s;
    use nexus_common::grant::{BindingPrincipal, ResourceRole};
    use nexus_common::grant_store::binding_to_item;
    use std::sync::{Arc, Mutex};

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";
    const ADMIN_SUB: &str = "a1b2c3d4-1111-2222-3333-444455556666";

    fn admin_context() -> Value {
        json!({
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": ADMIN_SUB,
                "issuer": ISSUER,
                "clientId": "client123",
                "globalRoles": "[\"platform-admin\"]",
                "teamGroups": "[]"
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

    fn existing_binding() -> RoleBinding {
        RoleBinding::new(
            "rb-existing",
            BindingPrincipal::aws_account("111122223333").unwrap(),
            ResourceRole::Publisher,
            "orders",
            Utc::now(),
            ADMIN_SUB,
        )
        .unwrap()
    }

    fn store_with(rules: Vec<Rule>) -> BindingStore {
        let client = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, &rules);
        BindingStore::new(client, "test-table")
    }

    async fn invoke(store: &BindingStore, payload: Value) -> Value {
        dispatch(store, &payload).await.unwrap()
    }

    fn status(resp: &Value) -> u16 {
        resp["statusCode"].as_u64().unwrap() as u16
    }

    fn body(resp: &Value) -> Value {
        serde_json::from_str(resp["body"].as_str().unwrap()).unwrap()
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

    #[tokio::test]
    async fn create_binding_returns_201_with_server_assigned_fields() {
        let store = store_with(vec![transact_ok()]);
        let mut payload = event("POST", "/bindings", admin_context());
        payload["body"] = json!(json!({
            "domain": "orders",
            "role": "publisher",
            "principal": {"type": "aws-account", "accountId": "111122223333"}
        })
        .to_string());

        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 201);
        let body = body(&resp);
        assert!(body["bindingId"].as_str().unwrap().len() > 8);
        assert_eq!(body["domain"], "orders");
        assert_eq!(body["role"], "publisher");
        assert_eq!(body["createdBy"], ADMIN_SUB);
        assert!(body["createdAt"].as_str().is_some());
    }

    #[tokio::test]
    async fn create_writes_binding_and_audit_in_one_transaction() {
        let captured: Arc<Mutex<Vec<TransactWriteItemsInput>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&captured);
        let rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
            .match_requests(move |req: &TransactWriteItemsInput| {
                seen.lock().unwrap().push(req.clone());
                true
            })
            .then_output(|| aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsOutput::builder().build());
        let store = store_with(vec![rule]);

        let mut payload = event("POST", "/bindings", admin_context());
        payload["body"] = json!(json!({
            "domain": "orders",
            "role": "publisher",
            "principal": {"type": "cognito-group", "issuer": ISSUER, "group": "team-orders"}
        })
        .to_string());
        assert_eq!(status(&invoke(&store, payload).await), 201);

        let calls = captured.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let items = calls[0].transact_items();
        assert_eq!(items.len(), 2);

        let put = items[0].put().expect("first item is the binding put");
        assert_eq!(
            put.condition_expression().unwrap(),
            "attribute_not_exists(PK)"
        );
        let put_item = put.item();
        assert_eq!(get_s(put_item, "itemType").unwrap(), "RoleBinding");
        assert_eq!(get_s(put_item, "domain").unwrap(), "orders");
        assert_eq!(get_s(put_item, "createdBy").unwrap(), ADMIN_SUB);

        let audit = items[1].put().expect("second item is the audit put");
        let audit_item = audit.item();
        assert_eq!(get_s(audit_item, "itemType").unwrap(), "AuditRecord");
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
    async fn idempotent_recreate_returns_existing_binding() {
        let existing = existing_binding();
        let item = binding_to_item(&existing);
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(move |req: &GetItemInput| req.consistent_read() == Some(true))
            .then_output(move || {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .set_item(Some(item.clone()))
                    .build()
            });
        let store = store_with(vec![transact_conditional_failure(), get_rule]);

        let mut payload = event("POST", "/bindings", admin_context());
        payload["body"] = json!(json!({
            "domain": "orders",
            "role": "publisher",
            "principal": {"type": "aws-account", "accountId": "111122223333"}
        })
        .to_string());

        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 200);
        assert_eq!(body(&resp)["bindingId"], "rb-existing");
    }

    #[tokio::test]
    async fn create_conflict_without_stored_binding_maps_to_409() {
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build()
        });
        let store = store_with(vec![transact_conditional_failure(), get_rule]);

        let mut payload = event("POST", "/bindings", admin_context());
        payload["body"] = json!(json!({
            "domain": "orders",
            "role": "publisher",
            "principal": {"type": "aws-account", "accountId": "111122223333"}
        })
        .to_string());

        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 409);
        assert_eq!(body(&resp)["code"], "ConflictException");
    }

    #[tokio::test]
    async fn list_bindings_requires_domain_and_returns_views() {
        let existing = existing_binding();
        let item = binding_to_item(&existing);
        let query_rule = mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req: &QueryInput| req.index_name() == Some("by-domain"))
            .then_output(move || {
                aws_sdk_dynamodb::operation::query::QueryOutput::builder()
                    .items(item.clone())
                    .build()
            });
        let store = store_with(vec![query_rule]);

        let mut payload = event("GET", "/bindings", admin_context());
        payload["queryStringParameters"] = json!({"domain": "orders"});
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 200);
        let bindings = body(&resp)["bindings"].as_array().unwrap().clone();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0]["bindingId"], "rb-existing");

        let missing = event("GET", "/bindings", admin_context());
        let resp = invoke(&store, missing).await;
        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn revoke_locates_by_id_and_deletes() {
        let existing = existing_binding();
        let item = binding_to_item(&existing);
        let query_rule = mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req: &QueryInput| req.index_name() == Some("by-binding-id"))
            .then_output(move || {
                aws_sdk_dynamodb::operation::query::QueryOutput::builder()
                    .items(item.clone())
                    .build()
            });
        let store = store_with(vec![query_rule, transact_ok()]);

        let mut payload = event("DELETE", "/bindings/{bindingId}", admin_context());
        payload["pathParameters"] = json!({"bindingId": "rb-existing"});
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 200);
        assert_eq!(body(&resp)["state"], "revoked");
    }

    #[tokio::test]
    async fn revoke_unknown_binding_returns_404() {
        let query_rule = mock!(aws_sdk_dynamodb::Client::query)
            .then_output(|| aws_sdk_dynamodb::operation::query::QueryOutput::builder().build());
        let store = store_with(vec![query_rule]);

        let mut payload = event("DELETE", "/bindings/{bindingId}", admin_context());
        payload["pathParameters"] = json!({"bindingId": "rb-missing"});
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 404);
        assert_eq!(body(&resp)["code"], "ResourceNotFoundException");
    }

    #[tokio::test]
    async fn revoke_racing_another_revoke_returns_404() {
        let existing = existing_binding();
        let item = binding_to_item(&existing);
        let query_rule = mock!(aws_sdk_dynamodb::Client::query).then_output(move || {
            aws_sdk_dynamodb::operation::query::QueryOutput::builder()
                .items(item.clone())
                .build()
        });
        let store = store_with(vec![query_rule, transact_conditional_failure()]);

        let mut payload = event("DELETE", "/bindings/{bindingId}", admin_context());
        payload["pathParameters"] = json!({"bindingId": "rb-existing"});
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 404);
    }

    #[tokio::test]
    async fn non_admin_oauth_caller_is_denied() {
        let ctx = json!({
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": "b2c3d4e5-2222-3333-4444-555566667777",
                "issuer": ISSUER,
                "clientId": "client123",
                "globalRoles": "[\"platform-auditor\"]",
                "teamGroups": "[\"team-orders\"]"
            }
        });
        let store = store_with(vec![]);
        for (method, resource) in [
            ("POST", "/bindings"),
            ("GET", "/bindings"),
            ("DELETE", "/bindings/{bindingId}"),
        ] {
            let resp = invoke(&store, event(method, resource, ctx.clone())).await;
            assert_eq!(
                status(&resp),
                403,
                "{method} {resource} must deny non-admin"
            );
            assert_eq!(body(&resp)["code"], "AccessDeniedException");
        }
    }

    #[tokio::test]
    async fn iam_caller_is_denied_on_every_route() {
        let ctx = json!({
            "identity": {"accountId": "111122223333", "userArn": "arn:aws:iam::111122223333:user/ci"}
        });
        let store = store_with(vec![]);
        for (method, resource) in [
            ("POST", "/bindings"),
            ("GET", "/bindings"),
            ("DELETE", "/bindings/{bindingId}"),
        ] {
            let resp = invoke(&store, event(method, resource, ctx.clone())).await;
            assert_eq!(
                status(&resp),
                403,
                "{method} {resource} must deny IAM callers"
            );
        }
    }

    #[tokio::test]
    async fn missing_principal_context_is_denied() {
        let store = store_with(vec![]);
        let resp = invoke(&store, event("POST", "/bindings", json!({}))).await;
        assert_eq!(status(&resp), 403);
    }

    #[tokio::test]
    async fn create_rejects_unknown_fields_and_bad_principals() {
        let store = store_with(vec![]);
        for body in [
            json!({"domain": "orders", "role": "publisher", "principal": {"type": "aws-account", "accountId": "111122223333"}, "createdBy": "attacker"}),
            json!({"domain": "orders", "role": "publisher", "principal": {"type": "aws-account", "accountId": "not-digits!"}}),
            json!({"domain": "", "role": "publisher", "principal": {"type": "aws-account", "accountId": "111122223333"}}),
        ] {
            let mut payload = event("POST", "/bindings", admin_context());
            payload["body"] = json!(body.to_string());
            let resp = invoke(&store, payload).await;
            assert_eq!(status(&resp), 400, "body {body} must be rejected");
        }
    }
}
