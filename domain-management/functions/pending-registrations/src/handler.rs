use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use nexus_common::api;
use nexus_common::dynamodb::{
    deserialize_pagination_token, get_s, get_string_set, s, serialize_pagination_token,
};
use nexus_common::error::ControlPlaneError;
use nexus_common::models::DomainStatus;
use nexus_common::principal_context::log_principal;
use serde_json::{json, Value};
use std::env;

/// The platform-admin approvals inbox: pending registrations with the
/// workflow task token and owner details. Denied to everyone else,
/// including auditors (read-only) and IAM automation (this route exists
/// only on the OAuth surface, but the handler still checks).
pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;

    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "pending registrations: deny (no trusted principal)");
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ));
        }
    };
    let is_admin = matches!(
        &principal,
        nexus_common::principal::TrustedPrincipal::OAuth(_)
    ) && principal.is_platform_admin();
    if !is_admin {
        tracing::warn!("pending registrations: deny (platform-admin only)");
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::AccessDenied,
        ));
    }

    let max_results = api::query_param(&event.payload, "maxResults")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(100)
        .clamp(1, 100);
    let start_key = api::query_param(&event.payload, "nextToken")
        .as_deref()
        .and_then(deserialize_pagination_token);

    let mut query = ddb
        .query()
        .table_name(&table)
        .index_name("by-status")
        .key_condition_expression("#s = :status")
        .expression_attribute_names("#s", "status")
        .expression_attribute_values(":status", s(DomainStatus::PendingApproval.as_str()))
        .limit(max_results);
    if let Some(key) = start_key {
        for (k, v) in key {
            query = query.exclusive_start_key(k, v);
        }
    }

    match query.send().await {
        Ok(output) => {
            let registrations: Vec<Value> = output
                .items()
                .iter()
                .map(|item| {
                    json!({
                        "domainName": get_s(item, "domainName"),
                        "description": get_s(item, "description"),
                        "ownerAccountId": get_s(item, "ownerAccountId"),
                        "ownerEmail": get_s(item, "ownerEmail"),
                        "secondaryOwnerEmails": get_string_set(item, "secondaryOwnerEmails"),
                        "owningTeam": get_s(item, "owningTeam"),
                        "createdAt": get_s(item, "createdAt"),
                        "taskToken": get_s(item, "taskToken"),
                    })
                })
                .collect();
            let pagination_token = output
                .last_evaluated_key()
                .filter(|k| !k.is_empty())
                .map(serialize_pagination_token);
            Ok(api::success_response(
                200,
                &json!({
                    "registrations": registrations,
                    "nextToken": pagination_token,
                }),
            ))
        }
        Err(e) => {
            tracing::error!(error = %e, "pending registrations query failed");
            Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                e.to_string(),
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::query::QueryOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client};
    use lambda_runtime::Context as LambdaContext;
    use std::collections::HashMap;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

    fn set_env() {
        env::set_var("DOMAINS_TABLE", "domains");
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

    fn event_with(ctx: Value) -> LambdaEvent<Value> {
        LambdaEvent::new(json!({ "requestContext": ctx }), LambdaContext::default())
    }

    fn pending_item(name: &str, team: &str) -> HashMap<String, AttributeValue> {
        HashMap::from([
            ("domainName".to_string(), s(name)),
            ("description".to_string(), s(&format!("{name} domain"))),
            ("ownerAccountId".to_string(), s("111122223333")),
            ("ownerEmail".to_string(), s(&format!("{name}@example.com"))),
            ("status".to_string(), s("PENDING_APPROVAL")),
            ("owningTeam".to_string(), s(team)),
            ("createdAt".to_string(), s("2026-08-11T00:00:00Z")),
            ("taskToken".to_string(), s("AQABAAAA...task-token")),
        ])
    }

    fn pending_query_rule(items: Vec<HashMap<String, AttributeValue>>) -> aws_smithy_mocks::Rule {
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req| {
                req.index_name() == Some("by-status")
                    && req
                        .expression_attribute_values()
                        .and_then(|v| v.get(":status"))
                        .and_then(|v| v.as_s().ok())
                        .is_some_and(|v| v == "PENDING_APPROVAL")
            })
            .then_output(move || {
                QueryOutput::builder()
                    .set_items(Some(items.clone()))
                    .build()
            })
    }

    #[tokio::test]
    async fn platform_admin_gets_pending_registrations_with_task_tokens() {
        set_env();
        let query = pending_query_rule(vec![pending_item("orders", "team-orders")]);
        let ddb = mock_client!(aws_sdk_dynamodb, [&query]);

        let resp = handle(&ddb, event_with(oauth_ctx("[\"platform-admin\"]", "[]")))
            .await
            .unwrap();
        assert_eq!(resp["statusCode"], 200);

        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        let regs = body["registrations"].as_array().unwrap();
        assert_eq!(regs.len(), 1);
        assert_eq!(regs[0]["domainName"], "orders");
        assert_eq!(regs[0]["owningTeam"], "team-orders");
        assert_eq!(regs[0]["taskToken"], "AQABAAAA...task-token");
        assert_eq!(regs[0]["ownerEmail"], "orders@example.com");
    }

    #[tokio::test]
    async fn empty_inbox_returns_200_with_empty_list() {
        set_env();
        let query = pending_query_rule(vec![]);
        let ddb = mock_client!(aws_sdk_dynamodb, [&query]);

        let resp = handle(&ddb, event_with(oauth_ctx("[\"platform-admin\"]", "[]")))
            .await
            .unwrap();
        assert_eq!(resp["statusCode"], 200);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["registrations"], json!([]));
        assert!(body["nextToken"].is_null());
    }

    #[tokio::test]
    async fn platform_auditor_is_denied() {
        set_env();
        let ddb = mock_client!(aws_sdk_dynamodb, &[]);
        let resp = handle(&ddb, event_with(oauth_ctx("[\"platform-auditor\"]", "[]")))
            .await
            .unwrap();
        assert_eq!(resp["statusCode"], 403);
    }

    #[tokio::test]
    async fn team_caller_is_denied() {
        set_env();
        let ddb = mock_client!(aws_sdk_dynamodb, &[]);
        let resp = handle(&ddb, event_with(oauth_ctx("[]", "[\"team-payments\"]")))
            .await
            .unwrap();
        assert_eq!(resp["statusCode"], 403);
    }

    #[tokio::test]
    async fn iam_caller_is_denied() {
        set_env();
        let ddb = mock_client!(aws_sdk_dynamodb, &[]);
        let ctx = json!({
            "identity": {
                "accountId": "123456789012",
                "userArn": "arn:aws:iam::123456789012:role/deploy"
            }
        });
        let resp = handle(&ddb, event_with(ctx)).await.unwrap();
        assert_eq!(resp["statusCode"], 403);
    }

    #[tokio::test]
    async fn request_without_principal_is_denied() {
        set_env();
        let ddb = mock_client!(aws_sdk_dynamodb, &[]);
        let resp = handle(&ddb, event_with(json!({}))).await.unwrap();
        assert_eq!(resp["statusCode"], 403);
    }
}
