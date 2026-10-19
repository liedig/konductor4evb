//! Request handler for `schema-promotions`.
//!
//! `POST /schemas/promotions` accepts a promotion on a target stage: the
//! stage selects the target environment and the source is the previous entry
//! of the platform environment list. The handler performs one synchronous
//! commit, the conditional `PROMOTION` head-item put; everything else rides
//! on the promotions table stream.
//!
//! `GET /schemas/promotions` returns the head item's status view through a
//! consistent read.

use aws_sdk_dynamodb::operation::put_item::PutItemError;
use aws_sdk_dynamodb::types::ReturnValuesOnConditionCheckFailure;
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_schemas::Client as SchemasClient;
use chrono::Utc;
use lambda_runtime::LambdaEvent;
use serde_json::{json, Value};
use uuid::Uuid;

use konductor_common::api;
use konductor_common::dynamodb::{get_s, s};
use konductor_common::error::ControlPlaneError;
use konductor_common::grant_engine::actions;
use konductor_common::models::CreatePromotionRequest;
use konductor_common::principal::{AuthorizationDecision, ResourceAuthorizer, TrustedPrincipal};
use konductor_common::principal_context::log_principal;
use konductor_common::promotion::{promotion_pk, Promotion, PromotionStatus, PROMOTION_SK};
use konductor_common::promotion_store::promotion_to_item;

use crate::status;
use crate::support::{
    actor_of, promotions_table, require_active_domain, source_latest_version, source_target,
    stage_of,
};

/// Promotions are history; the TTL keeps the head item for audits without
/// accumulating forever. Long past any single promotion's one-hour timeout.
const PROMOTION_TTL_SECONDS: i64 = 90 * 24 * 3600;

pub struct Clients {
    pub ddb: DynamoDbClient,
    pub schemas: SchemasClient,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: DynamoDbClient::new(config),
            schemas: SchemasClient::new(config),
        }
    }
}

pub async fn handle(
    clients: &Clients,
    authorizer: &impl ResourceAuthorizer,
    event: LambdaEvent<Value>,
) -> Value {
    let (event, _ctx) = event.into_parts();
    let request_context = event.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(_) => return api::from_control_plane_error(&ControlPlaneError::AccessDenied),
    };
    let method = event["httpMethod"].as_str().unwrap_or_default();
    let resource = event["resource"].as_str().unwrap_or_default();
    let result = match (method, resource) {
        ("POST", "/schemas/promotions") => {
            request_promotion(clients, authorizer, &principal, &event).await
        }
        ("GET", "/schemas/promotions") => {
            status::promotion_status(clients, authorizer, &principal, &event).await
        }
        _ => Err(api::validation_error(format!(
            "unsupported route {method} {resource}"
        ))),
    };
    match result {
        Ok(response) => response,
        Err(err) => api::from_control_plane_error(&err),
    }
}

async fn request_promotion(
    clients: &Clients,
    authorizer: &impl ResourceAuthorizer,
    principal: &TrustedPrincipal,
    event: &Value,
) -> Result<Value, ControlPlaneError> {
    let request: CreatePromotionRequest = api::parse_body(event)?;
    let (source, target) = source_target(stage_of(event)?)?;
    if authorizer
        .authorize(principal, actions::SCHEMA_PROMOTE, &request.domain)
        .await
        != AuthorizationDecision::Allow
    {
        return Err(ControlPlaneError::AccessDenied);
    }
    require_active_domain(clients, &request.domain).await?;
    let requested_version = source_latest_version(clients, &source, &request).await?;

    let now = Utc::now();
    let promotion = Promotion {
        promotion_id: Uuid::new_v4().to_string(),
        domain: request.domain.clone(),
        schema_name: request.event_name.clone(),
        source_environment: source,
        target_environment: target,
        from_version: None,
        requested_version,
        last_written_version: None,
        phase: PromotionStatus::Validating,
        reason: None,
        failed_version: None,
        actor: actor_of(principal),
        repin_matched: None,
        repin_repinned: None,
        repin_failed: None,
        drain_until: None,
        created_at: now,
        updated_at: now,
        expires_at: Some(now.timestamp() + PROMOTION_TTL_SECONDS),
    };
    match put_promotion(clients, &promotion).await? {
        Some(active_id) => Ok(conflict_response(&active_id)),
        None => Ok(api::success_response(202, &promotion.view())),
    }
}

/// The single synchronous commit. `Ok(Some(id))` means a non-terminal
/// promotion already holds the head item and carries its id.
async fn put_promotion(
    clients: &Clients,
    promotion: &Promotion,
) -> Result<Option<String>, ControlPlaneError> {
    let mut item = promotion_to_item(promotion);
    item.insert(
        "pk".to_string(),
        s(&promotion_pk(
            &promotion.domain,
            &promotion.schema_name,
            &promotion.target_environment,
        )),
    );
    item.insert("sk".to_string(), s(PROMOTION_SK));
    let result = clients
        .ddb
        .put_item()
        .table_name(promotions_table()?)
        .set_item(Some(item))
        .condition_expression(
            "attribute_not_exists(pk) OR #phase IN (:completed, :failed, :cancelled)",
        )
        .expression_attribute_names("#phase", "phase")
        .expression_attribute_values(":completed", s("COMPLETED"))
        .expression_attribute_values(":failed", s("FAILED"))
        .expression_attribute_values(":cancelled", s("CANCELLED"))
        .return_values_on_condition_check_failure(ReturnValuesOnConditionCheckFailure::AllOld)
        .send()
        .await;
    match result {
        Ok(_) => Ok(None),
        Err(e) => {
            let err = e.into_service_error();
            match err {
                PutItemError::ConditionalCheckFailedException(conflict) => {
                    let active = conflict
                        .item()
                        .and_then(|item| get_s(item, "promotionId"))
                        .unwrap_or_else(|| "unknown".to_string());
                    Ok(Some(active))
                }
                other => {
                    tracing::error!(error = %other, "promotion head put failed");
                    Err(ControlPlaneError::DynamoDb(other.to_string()))
                }
            }
        }
    }
}

fn conflict_response(active_id: &str) -> Value {
    let body = json!({
        "code": "ConflictException",
        "message": format!(
            "a promotion is already active for this schema in this environment: {active_id}"
        ),
        "promotionId": active_id,
    });
    json!({
        "statusCode": 409,
        "headers": { "Content-Type": "application/json" },
        "body": body.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::DateTime;
    use konductor_common::grant::{BindingPrincipal, ResourceRole, RoleBinding};
    use konductor_common::grant_engine::InMemoryGrantEngine;
    use konductor_common::models::DomainStatus;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/pool";

    fn set_env() {
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var("DOMAINS_TABLE", "domains-test");
        std::env::set_var("PROMOTIONS_TABLE", "promotions-test");
        std::env::set_var("ENVIRONMENTS", "dev,staging,prod");
    }

    fn publisher_binding(group: &str, domain: &str) -> RoleBinding {
        RoleBinding::new(
            "rb-1",
            BindingPrincipal::cognito_group(ISSUER, group).unwrap(),
            ResourceRole::Publisher,
            domain,
            Utc::now(),
            "test",
        )
        .unwrap()
    }

    fn oauth_event(stage: &str, method: &str, body: Option<Value>, global_roles: &str) -> Value {
        let mut event = json!({
            "httpMethod": method,
            "resource": "/schemas/promotions",
            "requestContext": {
                "stage": stage,
                "authorizer": {
                    "authenticationMechanism": "oauth",
                    "principalId": "sub-123",
                    "issuer": ISSUER,
                    "clientId": "client-1",
                    "globalRoles": global_roles,
                    "teamGroups": "[\"team-orders\"]",
                },
            },
        });
        if let Some(body) = body {
            event["body"] = Value::String(body.to_string());
        }
        event
    }

    fn post_event(stage: &str) -> Value {
        oauth_event(
            stage,
            "POST",
            Some(json!({ "domain": "orders", "eventName": "OrderNotification" })),
            "[]",
        )
    }

    fn get_event(stage: &str, global_roles: &str) -> Value {
        let mut event = oauth_event(stage, "GET", None, global_roles);
        event["queryStringParameters"] =
            json!({ "domain": "orders", "eventName": "OrderNotification" });
        event
    }

    fn event(payload: Value) -> LambdaEvent<Value> {
        LambdaEvent::new(payload, Default::default())
    }

    fn status(resp: &Value) -> u16 {
        resp["statusCode"].as_u64().unwrap() as u16
    }

    fn body(resp: &Value) -> Value {
        serde_json::from_str(resp["body"].as_str().unwrap()).unwrap()
    }

    fn active_domain_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.table_name() == Some("domains-test"))
            .then_output(|| {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .item("domainName", s("orders"))
                    .item("status", s("ACTIVE"))
                    .build()
            })
    }

    fn source_latest_rule(version: &'static str) -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema)
            .match_requests(move |req| {
                req.registry_name() == Some("acme-dev-orders-schema-registry")
                    && req.schema_name() == Some("acme-orders@OrderNotification")
            })
            .then_output(move || {
                aws_sdk_schemas::operation::describe_schema::DescribeSchemaOutput::builder()
                    .schema_version(version)
                    .build()
            })
    }

    fn source_missing_rule() -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema).then_error(|| {
            aws_sdk_schemas::operation::describe_schema::DescribeSchemaError::NotFoundException(
                aws_sdk_schemas::types::error::NotFoundException::builder()
                    .message("not found")
                    .build(),
            )
        })
    }

    /// Accepts only the conditional head-item put; the assertion after the
    /// call proves the promotion contract reached DynamoDB.
    fn conditional_put_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::put_item)
            .match_requests(|req| {
                req.table_name() == Some("promotions-test")
                    && req.condition_expression().is_some_and(|c| {
                        c.contains("attribute_not_exists(pk)") && c.contains(":cancelled")
                    })
                    && req.item().is_some_and(|item| {
                        item.get("phase")
                            .and_then(|v| v.as_s().ok())
                            .map(String::as_str)
                            == Some("VALIDATING")
                            && item.contains_key("expiresAt")
                    })
            })
            .then_output(|| aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build())
    }

    fn active_promotion_conflict_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::put_item).then_error(|| {
            PutItemError::ConditionalCheckFailedException(
                aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                    .message("conditional check failed")
                    .item("promotionId", s("promo-active-1"))
                    .build(),
            )
        })
    }

    fn sample_promotion() -> Promotion {
        let at = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
            .unwrap()
            .into();
        Promotion {
            promotion_id: "promo-1".to_string(),
            domain: "orders".to_string(),
            schema_name: "OrderNotification".to_string(),
            source_environment: "dev".to_string(),
            target_environment: "staging".to_string(),
            from_version: Some(1),
            requested_version: 3,
            last_written_version: Some(3),
            phase: PromotionStatus::Completed,
            reason: None,
            failed_version: None,
            actor: "oauth:sub-123".to_string(),
            repin_matched: Some(2),
            repin_repinned: Some(2),
            repin_failed: Some(0),
            drain_until: None,
            created_at: at,
            updated_at: at,
            expires_at: Some(1_777_777_777),
        }
    }

    fn promotions_get_rule() -> Rule {
        let mut item = promotion_to_item(&sample_promotion());
        item.insert("pk".to_string(), s("orders#OrderNotification#staging"));
        item.insert("sk".to_string(), s(PROMOTION_SK));
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.table_name() == Some("promotions-test") && req.consistent_read() == Some(true)
            })
            .then_output(move || {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .set_item(Some(item.clone()))
                    .build()
            })
    }

    fn promotions_absent_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.table_name() == Some("promotions-test"))
            .then_output(|| aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build())
    }

    fn clients(ddb_rules: &[&Rule], schemas_rules: &[&Rule]) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
            schemas: mock_client!(aws_sdk_schemas, RuleMode::MatchAny, schemas_rules),
        }
    }

    #[tokio::test]
    async fn post_accepts_a_promotion_on_a_target_stage() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let latest = source_latest_rule("3");
        let put = conditional_put_rule();
        let clients = clients(&[&domain, &put], &[&latest]);

        let resp = handle(&clients, &authorizer, event(post_event("staging"))).await;

        assert_eq!(status(&resp), 202);
        let body = body(&resp);
        assert!(body["promotionId"]
            .as_str()
            .is_some_and(|id| !id.is_empty()));
        assert_eq!(body["sourceEnvironment"], "dev");
        assert_eq!(body["targetEnvironment"], "staging");
        assert_eq!(body["requestedVersion"], 3);
        assert_eq!(body["status"], "VALIDATING");
        assert_eq!(put.num_calls(), 1, "the head item put must be conditional");
    }

    #[tokio::test]
    async fn post_rejects_the_authoring_stage() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[]);

        let resp = handle(&clients, &authorizer, event(post_event("dev"))).await;

        assert_eq!(status(&resp), 400);
        assert!(body(&resp)["message"]
            .as_str()
            .unwrap()
            .contains("authoring environment"));
    }

    #[tokio::test]
    async fn post_rejects_an_unknown_stage() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[]);

        let resp = handle(&clients, &authorizer, event(post_event("qa"))).await;

        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn post_requires_the_promote_grant() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![]);
        let clients = clients(&[], &[]);

        let resp = handle(&clients, &authorizer, event(post_event("staging"))).await;

        assert_eq!(status(&resp), 403);
    }

    #[tokio::test]
    async fn post_rejects_an_inactive_domain() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let suspended = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.table_name() == Some("domains-test"))
            .then_output(|| {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .item("domainName", s("orders"))
                    .item("status", s(DomainStatus::Inactive.as_str()))
                    .build()
            });
        let clients = clients(&[&suspended], &[]);

        let resp = handle(&clients, &authorizer, event(post_event("staging"))).await;

        assert_eq!(status(&resp), 404);
    }

    #[tokio::test]
    async fn post_rejects_a_schema_missing_from_the_source() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let missing = source_missing_rule();
        let clients = clients(&[&domain], &[&missing]);

        let resp = handle(&clients, &authorizer, event(post_event("staging"))).await;

        assert_eq!(status(&resp), 404);
        assert!(body(&resp)["message"]
            .as_str()
            .unwrap()
            .contains("OrderNotification"));
    }

    #[tokio::test]
    async fn post_conflicts_with_the_active_promotion_id() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let latest = source_latest_rule("3");
        let conflict = active_promotion_conflict_rule();
        let clients = clients(&[&domain, &conflict], &[&latest]);

        let resp = handle(&clients, &authorizer, event(post_event("staging"))).await;

        assert_eq!(status(&resp), 409);
        let body = body(&resp);
        assert_eq!(body["code"], "ConflictException");
        assert_eq!(body["promotionId"], "promo-active-1");
    }

    #[tokio::test]
    async fn get_returns_the_full_status_shape() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let get = promotions_get_rule();
        let clients = clients(&[&domain, &get], &[]);

        let resp = handle(&clients, &authorizer, event(get_event("staging", "[]"))).await;

        assert_eq!(status(&resp), 200);
        let body = body(&resp);
        assert_eq!(body["promotionId"], "promo-1");
        assert_eq!(body["schemaName"], "OrderNotification");
        assert_eq!(body["sourceEnvironment"], "dev");
        assert_eq!(body["targetEnvironment"], "staging");
        assert_eq!(body["requestedVersion"], 3);
        assert_eq!(body["status"], "COMPLETED");
        assert_eq!(body["replayedVersions"], json!([2, 3]));
        assert_eq!(body["targetVersion"], 3);
        assert_eq!(body["repin"]["matched"], 2);
        assert!(body["createdAt"].as_str().is_some());
        assert!(body["updatedAt"].as_str().is_some());
        assert_eq!(get.num_calls(), 1, "status reads are consistent point gets");
    }

    #[tokio::test]
    async fn get_allows_a_platform_auditor_without_a_binding() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![]);
        let domain = active_domain_rule();
        let get = promotions_get_rule();
        let clients = clients(&[&domain, &get], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(get_event("staging", "[\"platform-auditor\"]")),
        )
        .await;

        assert_eq!(status(&resp), 200);
    }

    #[tokio::test]
    async fn get_denies_a_caller_without_grant_or_role() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![]);
        let clients = clients(&[], &[]);

        let resp = handle(&clients, &authorizer, event(get_event("staging", "[]"))).await;

        assert_eq!(status(&resp), 403);
    }

    #[tokio::test]
    async fn get_returns_404_when_no_promotion_is_recorded() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let absent = promotions_absent_rule();
        let clients = clients(&[&domain, &absent], &[]);

        let resp = handle(&clients, &authorizer, event(get_event("staging", "[]"))).await;

        assert_eq!(status(&resp), 404);
        assert_eq!(body(&resp)["code"], "ResourceNotFoundException");
    }

    #[tokio::test]
    async fn get_rejects_the_authoring_stage() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[]);

        let resp = handle(&clients, &authorizer, event(get_event("dev", "[]"))).await;

        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn get_requires_both_query_parameters() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[]);
        let mut payload = get_event("staging", "[]");
        payload["queryStringParameters"] = json!({ "domain": "orders" });

        let resp = handle(&clients, &authorizer, event(payload)).await;

        assert_eq!(status(&resp), 400);
    }
}
