use anyhow::{Context, Result};
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;
use aws_sdk_dynamodb::types::ReturnValue;
use lambda_runtime::{Error, LambdaEvent};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;

use konductor_common::api;
use konductor_common::dynamodb::{get_s, s};
use konductor_common::error::{ControlPlaneError, ValidationExceptionReason};
use konductor_common::models::DomainStatus;
use konductor_common::principal_context::log_principal;

/// Platform-managed enable/disable. Disabling is deliberately shallow: the
/// status flip alone makes the publish, schema, and subscription paths
/// reject the domain (they all require `ACTIVE`), while every existing
/// subscription, entitlement, and provisioned resource stays untouched.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StateRequest {
    action: StateAction,
}

#[derive(Debug, Deserialize)]
enum StateAction {
    #[serde(rename = "enable")]
    Enable,
    #[serde(rename = "disable")]
    Disable,
    #[serde(rename = "retry-provisioning")]
    RetryProvisioning,
}

impl StateAction {
    fn required_current(&self) -> DomainStatus {
        match self {
            Self::Enable => DomainStatus::Inactive,
            Self::Disable => DomainStatus::Active,
            Self::RetryProvisioning => DomainStatus::ProvisioningFailed,
        }
    }

    fn target(&self) -> DomainStatus {
        match self {
            Self::Enable => DomainStatus::Active,
            Self::Disable => DomainStatus::Inactive,
            Self::RetryProvisioning => DomainStatus::Approved,
        }
    }
}

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    events: &aws_sdk_eventbridge::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let event_bus = env::var("KONDUKTOR_EVENT_BUS").context("KONDUKTOR_EVENT_BUS not set")?;

    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) if p.is_platform_admin() => p,
        Ok(_) => {
            tracing::warn!("domain state: deny (platform-admin only)");
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ));
        }
        Err(e) => {
            tracing::warn!(error = %e, "domain state: no trusted principal");
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ));
        }
    };

    let Some(domain_name) = api::path_param(&event.payload, "domainName") else {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "Missing domainName path parameter",
        )));
    };

    let body_str = event
        .payload
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("");
    let request: StateRequest = match serde_json::from_str(body_str) {
        Ok(r) => r,
        Err(e) => {
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::ValidationException {
                    message: format!("invalid request body: {e}"),
                    reason: ValidationExceptionReason::UnknownOperation,
                    field_list: None,
                },
            ));
        }
    };

    match request.action {
        StateAction::RetryProvisioning => {
            retry_provisioning(
                ddb,
                events,
                &table,
                &event_bus,
                &domain_name,
                principal.principal_id(),
            )
            .await
        }
        _ => transition(ddb, &table, &domain_name, &request.action).await,
    }
}

/// Retry re-enters the provisioning saga via its own lifecycle event rather
/// than replaying the approval event: the audit trail records that a retry
/// happened, and the workflow input shape stays identical. Bindings from the
/// original approval are deliberately untouched.
async fn retry_provisioning(
    ddb: &aws_sdk_dynamodb::Client,
    events: &aws_sdk_eventbridge::Client,
    table: &str,
    event_bus: &str,
    domain_name: &str,
    retried_by: &str,
) -> Result<Value, Error> {
    let now = chrono::Utc::now().to_rfc3339();
    let result = ddb
        .update_item()
        .table_name(table)
        .key("domainName", s(domain_name))
        .update_expression("SET #status = :target, updatedAt = :now REMOVE errorDetail")
        .condition_expression("#status = :current")
        .expression_attribute_names("#status", "status")
        .expression_attribute_values(":target", s(DomainStatus::Approved.as_str()))
        .expression_attribute_values(":current", s(DomainStatus::ProvisioningFailed.as_str()))
        .expression_attribute_values(":now", s(&now))
        .return_values(ReturnValue::AllNew)
        .return_values_on_condition_check_failure(
            aws_sdk_dynamodb::types::ReturnValuesOnConditionCheckFailure::AllOld,
        )
        .send()
        .await;

    let attrs = match result {
        Ok(output) => output.attributes.unwrap_or_default(),
        Err(e) => return Ok(transition_error(e, domain_name, DomainStatus::Approved)),
    };

    let detail = json!({
        "domainName": domain_name,
        "ownerAccountId": get_s(&attrs, "ownerAccountId"),
        "ownerEmail": get_s(&attrs, "ownerEmail"),
        "secondaryOwnerEmails": konductor_common::dynamodb::get_string_set(&attrs, "secondaryOwnerEmails"),
        "environments": konductor_common::dynamodb::get_string_list(&attrs, "environments").unwrap_or_default(),
        "costCenter": get_s(&attrs, "costCenter"),
        "retriedBy": retried_by,
        "retriedAt": now,
    });
    let put = events
        .put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(event_bus)
                .source("konductor.domain-management")
                .detail_type("DomainProvisioningRetryRequested")
                .detail(detail.to_string())
                .build(),
        )
        .send()
        .await;

    match put {
        Ok(out) if out.failed_entry_count() == 0 => {}
        Ok(out) => {
            tracing::error!(entries = ?out.entries(), "retry event entry failed");
            return Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                "retry event was not accepted by the event bus".to_string(),
            )));
        }
        Err(e) => {
            tracing::error!(error = %e, "retry event emission failed");
            return Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                e.to_string(),
            )));
        }
    }

    Ok(api::success_response(
        200,
        &json!({
            "domainName": domain_name,
            "status": DomainStatus::Approved.as_str(),
            "updatedAt": now,
        }),
    ))
}

/// Maps a conditional-update failure to the API error vocabulary: no record
/// means 404, a status mismatch means 409.
fn transition_error(
    e: aws_sdk_dynamodb::error::SdkError<aws_sdk_dynamodb::operation::update_item::UpdateItemError>,
    domain_name: &str,
    target: DomainStatus,
) -> Value {
    let service_err = e.into_service_error();
    if let UpdateItemError::ConditionalCheckFailedException(ccf) = &service_err {
        return match ccf.item() {
            None => api::from_control_plane_error(&ControlPlaneError::DomainNotFound {
                domain_name: domain_name.to_string(),
            }),
            Some(item) => {
                let actual = get_s(item, "status").unwrap_or_default();
                api::from_control_plane_error(&ControlPlaneError::InvalidStateTransition {
                    current: actual,
                    target: target.as_str().to_string(),
                })
            }
        };
    }
    tracing::error!(error = %service_err, "domain state update failed");
    api::from_control_plane_error(&ControlPlaneError::DynamoDb(service_err.to_string()))
}

async fn transition(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    domain_name: &str,
    action: &StateAction,
) -> Result<Value, Error> {
    let current = action.required_current();
    let target = action.target();
    let now = chrono::Utc::now().to_rfc3339();

    let result = ddb
        .update_item()
        .table_name(table)
        .key("domainName", s(domain_name))
        .update_expression("SET #status = :target, updatedAt = :now")
        .condition_expression("#status = :current")
        .expression_attribute_names("#status", "status")
        .expression_attribute_values(":target", s(target.as_str()))
        .expression_attribute_values(":current", s(current.as_str()))
        .expression_attribute_values(":now", s(&now))
        .return_values(ReturnValue::AllNew)
        .return_values_on_condition_check_failure(
            aws_sdk_dynamodb::types::ReturnValuesOnConditionCheckFailure::AllOld,
        )
        .send()
        .await;

    match result {
        Ok(output) => {
            let attrs = output.attributes.unwrap_or_default();
            Ok(api::success_response(
                200,
                &json!({
                    "domainName": get_s(&attrs, "domainName"),
                    "status": get_s(&attrs, "status"),
                    "updatedAt": get_s(&attrs, "updatedAt"),
                }),
            ))
        }
        Err(e) => Ok(transition_error(e, domain_name, target)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client};
    use lambda_runtime::Context as LambdaContext;
    use std::collections::HashMap;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

    fn set_env() {
        env::set_var("DOMAINS_TABLE", "domains");
        env::set_var("KONDUKTOR_EVENT_BUS", "konductor-bus");
    }

    fn oauth_ctx(global_roles: &str, team_groups: &str) -> Value {
        json!({
            "identity": { "sourceIp": "10.0.0.1" },
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": "user-subject-1",
                "issuer": ISSUER,
                "clientId": "client-123",
                "globalRoles": global_roles,
                "teamGroups": team_groups
            }
        })
    }

    fn state_event(ctx: Value, domain: &str, action: &str) -> LambdaEvent<Value> {
        LambdaEvent::new(
            json!({
                "requestContext": ctx,
                "pathParameters": { "domainName": domain },
                "body": json!({ "action": action }).to_string()
            }),
            LambdaContext::default(),
        )
    }

    fn update_rule_for(expected_condition: &str) -> aws_smithy_mocks::Rule {
        let condition = expected_condition.to_string();
        mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(move |req| req.condition_expression() == Some(condition.as_str()))
            .then_output(|| {
                UpdateItemOutput::builder()
                    .set_attributes(Some(HashMap::from([
                        ("domainName".to_string(), s("orders")),
                        ("status".to_string(), s("INACTIVE")),
                        ("updatedAt".to_string(), s("2026-08-11T12:00:00Z")),
                    ])))
                    .build()
            })
    }

    fn failed_domain_record() -> HashMap<String, AttributeValue> {
        HashMap::from([
            ("domainName".to_string(), s("sales")),
            ("status".to_string(), s("APPROVED")),
            ("updatedAt".to_string(), s("2026-08-19T12:00:00Z")),
            ("ownerAccountId".to_string(), s("210987654321")),
            ("ownerEmail".to_string(), s("owner@example.com")),
            (
                "secondaryOwnerEmails".to_string(),
                AttributeValue::Ss(vec!["backup@example.com".to_string()]),
            ),
            (
                "environments".to_string(),
                AttributeValue::L(vec![s("dev"), s("staging"), s("prod")]),
            ),
        ])
    }

    fn retry_update_rule() -> aws_smithy_mocks::Rule {
        mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                req.condition_expression() == Some("#status = :current")
                    && req
                        .update_expression()
                        .is_some_and(|e| e.contains("REMOVE errorDetail"))
            })
            .then_output(|| {
                UpdateItemOutput::builder()
                    .set_attributes(Some(failed_domain_record()))
                    .build()
            })
    }

    fn retry_event_rule(
        captured: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) -> aws_smithy_mocks::Rule {
        mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(move |req| {
                if let Some(entry) = req.entries().first() {
                    captured.lock().unwrap().push(format!(
                        "{}|{}",
                        entry.detail_type().unwrap_or(""),
                        entry.detail().unwrap_or("")
                    ));
                }
                true
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .build()
            })
    }

    #[tokio::test]
    async fn retry_on_failed_domain_approves_and_emits_retry_event() {
        set_env();
        let update = retry_update_rule();
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_rule = retry_event_rule(std::sync::Arc::clone(&captured));
        let ddb = mock_client!(aws_sdk_dynamodb, [&update]);
        let events = mock_client!(aws_sdk_eventbridge, [&events_rule]);

        let resp = handle(
            &ddb,
            &events,
            state_event(
                oauth_ctx("[\"platform-admin\"]", "[]"),
                "sales",
                "retry-provisioning",
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 200);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["status"], "APPROVED");

        let calls = captured.lock().unwrap();
        assert_eq!(calls.len(), 1, "exactly one lifecycle event");
        let (detail_type, detail) = calls[0].split_once('|').unwrap();
        assert_eq!(detail_type, "DomainProvisioningRetryRequested");
        let detail: Value = serde_json::from_str(detail).unwrap();
        assert_eq!(detail["domainName"], "sales");
        assert_eq!(detail["ownerAccountId"], "210987654321");
        assert_eq!(detail["environments"], json!(["dev", "staging", "prod"]));
        assert_eq!(detail["retriedBy"], "user-subject-1");
        assert!(detail["retriedAt"].is_string());
    }

    #[tokio::test]
    async fn retry_on_non_failed_domain_conflicts_and_emits_nothing() {
        set_env();
        let item = HashMap::from([("status".to_string(), s("ACTIVE"))]);
        let update = mock!(aws_sdk_dynamodb::Client::update_item).then_error(move || {
            UpdateItemError::ConditionalCheckFailedException(
                aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                    .set_item(Some(item.clone()))
                    .build(),
            )
        });
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_rule = retry_event_rule(std::sync::Arc::clone(&captured));
        let ddb = mock_client!(aws_sdk_dynamodb, [&update]);
        let events = mock_client!(aws_sdk_eventbridge, [&events_rule]);

        let resp = handle(
            &ddb,
            &events,
            state_event(
                oauth_ctx("[\"platform-admin\"]", "[]"),
                "sales",
                "retry-provisioning",
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 409);
        let calls = captured.lock().unwrap();
        assert_eq!(calls.len(), 0, "no event on a failed transition");
    }

    #[tokio::test]
    async fn team_caller_retry_is_denied() {
        set_env();
        let ddb = mock_client!(aws_sdk_dynamodb, &[]);
        let events = mock_client!(aws_sdk_eventbridge, &[]);

        let resp = handle(
            &ddb,
            &events,
            state_event(
                oauth_ctx("[]", "[\"team-sales\"]"),
                "sales",
                "retry-provisioning",
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 403);
    }

    #[tokio::test]
    async fn platform_admin_disables_an_active_domain() {
        set_env();
        let update = update_rule_for("#status = :current");
        let ddb = mock_client!(aws_sdk_dynamodb, [&update]);
        let events = mock_client!(aws_sdk_eventbridge, &[]);

        let resp = handle(
            &ddb,
            &events,
            state_event(oauth_ctx("[\"platform-admin\"]", "[]"), "orders", "disable"),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 200);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["status"], "INACTIVE");
        assert_eq!(update.num_calls(), 1);
    }

    #[tokio::test]
    async fn invalid_transition_maps_to_invalid_state_transition_error() {
        set_env();
        let item = HashMap::from([("status".to_string(), s("PENDING_APPROVAL"))]);
        let update = mock!(aws_sdk_dynamodb::Client::update_item).then_error(move || {
            UpdateItemError::ConditionalCheckFailedException(
                aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                    .set_item(Some(item.clone()))
                    .build(),
            )
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&update]);
        let events = mock_client!(aws_sdk_eventbridge, &[]);

        let resp = handle(
            &ddb,
            &events,
            state_event(oauth_ctx("[\"platform-admin\"]", "[]"), "orders", "disable"),
        )
        .await
        .unwrap();

        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(resp["statusCode"], 409);
        assert_eq!(body["code"], "ConflictException");
    }

    #[tokio::test]
    async fn missing_domain_maps_to_not_found() {
        set_env();
        let update = mock!(aws_sdk_dynamodb::Client::update_item).then_error(|| {
            UpdateItemError::ConditionalCheckFailedException(
                aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder().build(),
            )
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&update]);
        let events = mock_client!(aws_sdk_eventbridge, &[]);

        let resp = handle(
            &ddb,
            &events,
            state_event(oauth_ctx("[\"platform-admin\"]", "[]"), "ghost", "disable"),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 404);
    }

    #[tokio::test]
    async fn non_admin_caller_is_denied_and_domain_untouched() {
        set_env();
        let ddb = mock_client!(aws_sdk_dynamodb, &[]);
        let events = mock_client!(aws_sdk_eventbridge, &[]);

        let resp = handle(
            &ddb,
            &events,
            state_event(oauth_ctx("[]", "[\"team-payments\"]"), "orders", "disable"),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 403);
    }

    #[tokio::test]
    async fn unknown_action_is_a_validation_error() {
        set_env();
        let ddb = mock_client!(aws_sdk_dynamodb, &[]);

        let event = LambdaEvent::new(
            json!({
                "requestContext": oauth_ctx("[\"platform-admin\"]", "[]"),
                "pathParameters": { "domainName": "orders" },
                "body": json!({ "action": "pause" }).to_string()
            }),
            LambdaContext::default(),
        );
        let events = mock_client!(aws_sdk_eventbridge, &[]);
        let resp = handle(&ddb, &events, event).await.unwrap();

        assert_eq!(resp["statusCode"], 400);
    }
}
