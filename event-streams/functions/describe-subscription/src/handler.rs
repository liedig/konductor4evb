use std::env;

use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use konductor_common::api;
use konductor_common::error::{ControlPlaneError, ValidationExceptionReason};
use konductor_common::grant::ResourceRole;
use konductor_common::grant_engine::has_role_binding;
use konductor_common::grant_store::BindingStore;
use konductor_common::principal::{GlobalRole, TrustedPrincipal};
use konductor_common::principal_context::log_principal;
use konductor_common::streams::{EventStream, SubscriptionId};
use konductor_common::streams_db;

/// Platform roles read any subscription (operations and audit); everyone
/// else needs the subscriber binding on the consumer domain or the
/// publisher binding on the producer domain.
fn is_platform(principal: &TrustedPrincipal) -> bool {
    principal.is_platform_admin()
        || principal
            .global_roles()
            .contains(&GlobalRole::PlatformAuditor)
}

/// A subscription is a relationship between two domains: either party with
/// the matching binding, or a platform role, may see it. Unauthorized calls
/// are a flat 403 — deny-as-unknown concealment was dropped by design.
async fn caller_may_view(
    store: &BindingStore,
    principal: &TrustedPrincipal,
    stream: &EventStream,
) -> bool {
    if is_platform(principal) {
        return true;
    }
    for (role, domain) in [
        (ResourceRole::Subscriber, &stream.consumer_domain),
        (ResourceRole::Publisher, &stream.producer_domain),
    ] {
        if has_role_binding(store, principal, role, domain).await {
            return true;
        }
    }
    false
}

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let bindings_table = env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;

    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "describe subscription: deny (no trusted principal)");
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ));
        }
    };

    let Some(raw_id) = api::path_param(&event.payload, "subscriptionId") else {
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::ValidationException {
                message: "missing subscriptionId path parameter".to_string(),
                reason: ValidationExceptionReason::FieldValidationFailed,
                field_list: None,
            },
        ));
    };

    let id = match SubscriptionId::parse(&raw_id) {
        Ok(id) => id,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    let schema_name = format!("{organization}-{}@{}", id.producer_domain, id.detail_type);
    let not_found = || {
        api::error_response(
            404,
            konductor_common::models::ApiError {
                code: "ResourceNotFoundException".to_string(),
                message: format!("subscription '{raw_id}' not found"),
                request_id: None,
                reason: None,
                field_list: None,
            },
        )
    };

    match streams_db::get_stream(
        ddb,
        &table,
        &id.producer_domain,
        &schema_name,
        &id.consumer_domain,
    )
    .await
    {
        Ok(Some(stream)) => {
            let store = BindingStore::new(ddb.clone(), &bindings_table);
            if caller_may_view(&store, &principal, &stream).await {
                Ok(api::success_response(
                    200,
                    &json!({
                        "subscriptionId": stream.subscription_id().to_string(),
                        "consumerDomain": stream.consumer_domain,
                        "producerDomain": stream.producer_domain,
                        "source": stream.source,
                        "detailType": stream.detail_type,
                        "schemaName": stream.schema_name,
                        "status": stream.status,
                        "errorDetail": stream.error_detail,
                        "environments": stream.environments,
                        "createdAt": stream.created_at.to_rfc3339(),
                        "updatedAt": stream.updated_at.to_rfc3339(),
                        "removedAt": stream.removed_at.map(|t| t.to_rfc3339()),
                    }),
                ))
            } else {
                tracing::warn!(
                    consumer = stream.consumer_domain,
                    producer = stream.producer_domain,
                    "describe subscription: deny (caller bound to neither side)"
                );
                Ok(api::from_control_plane_error(
                    &ControlPlaneError::AccessDenied,
                ))
            }
        }
        Ok(None) => Ok(not_found()),
        Err(e) => {
            tracing::error!(error = %e, "event stream lookup failed");
            Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                e.to_string(),
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use lambda_runtime::Context as LambdaContext;

    use konductor_common::grant::{BindingPrincipal, RoleBinding};
    use konductor_common::grant_store::binding_to_item;
    use konductor_common::streams::{EnvStream, StreamStatus};
    use konductor_common::streams_db::stream_to_item;

    fn set_env() {
        std::env::set_var("EVENT_STREAMS_TABLE", "streams");
        std::env::set_var("BINDINGS_TABLE", "bindings");
        std::env::set_var("ORGANIZATION", "acme");
    }

    fn iam_request_context(account_id: &str) -> Value {
        json!({
            "identity": {
                "accountId": account_id,
                "userArn": format!("arn:aws:iam::{account_id}:role/operator"),
            }
        })
    }

    fn oauth_platform_context(role: &str) -> Value {
        json!({
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": "user-1",
                "issuer": "https://idp.example.com",
                "clientId": "client-1",
                "globalRoles": format!("[\"{role}\"]"),
                "teamGroups": "[]",
            }
        })
    }

    fn request(context: Value, id: &str) -> LambdaEvent<Value> {
        LambdaEvent::new(
            json!({
                "requestContext": context,
                "pathParameters": { "subscriptionId": id },
            }),
            LambdaContext::default(),
        )
    }

    fn stream() -> EventStream {
        let mut environments = HashMap::new();
        environments.insert(
            "prod".to_string(),
            EnvStream {
                pinned_version: Some(3),
                rule_name: "checkout-OrderNotification".to_string(),
                rule_arn: None,
                enabled: true,
                status: StreamStatus::Active,
                error_detail: None,
            },
        );
        EventStream {
            producer_domain: "orders".to_string(),
            consumer_domain: "checkout".to_string(),
            source: "acme-orders".to_string(),
            detail_type: "OrderNotification".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            status: StreamStatus::Active,
            error_detail: None,
            environments,
            upgrade_events: None,
            request_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
            expires_at: None,
        }
    }

    fn stream_rule(stream: Option<EventStream>) -> Rule {
        let item: Option<HashMap<String, AttributeValue>> = stream.as_ref().map(stream_to_item);
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.key()
                    .and_then(|k| k.get(konductor_common::streams_db::PK))
                    .and_then(|v| v.as_s().ok())
                    .map(|v| v == "orders")
                    .unwrap_or(false)
            })
            .then_output(move || {
                let b = GetItemOutput::builder();
                match &item {
                    Some(i) => b.set_item(Some(i.clone())).build(),
                    None => b.build(),
                }
            })
    }

    fn binding_rule(principal: BindingPrincipal, role: ResourceRole, domain: &str) -> Rule {
        let binding = RoleBinding::new("b-1", principal, role, domain, Utc::now(), "test").unwrap();
        let item = binding_to_item(&binding);
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests({
                let expected = item.clone();
                move |req| {
                    let key = req.key().unwrap();
                    key.get("PK").and_then(|v| v.as_s().ok())
                        == expected.get("PK").and_then(|v| v.as_s().ok())
                        && key.get("SK").and_then(|v| v.as_s().ok())
                            == expected.get("SK").and_then(|v| v.as_s().ok())
                }
            })
            .then_output(move || {
                GetItemOutput::builder()
                    .set_item(Some(item.clone()))
                    .build()
            })
    }

    fn no_binding_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| GetItemOutput::builder().build())
    }

    fn ddb_client(rules: &[&Rule]) -> aws_sdk_dynamodb::Client {
        mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, rules)
    }

    fn body(response: &Value) -> Value {
        serde_json::from_str(response["body"].as_str().unwrap()).unwrap()
    }

    const ID: &str = "checkout.orders.OrderNotification";

    #[tokio::test]
    async fn consumer_side_binding_sees_the_detail() {
        set_env();
        let stream = stream_rule(Some(stream()));
        let binding = binding_rule(
            BindingPrincipal::aws_account("111122223333").unwrap(),
            ResourceRole::Subscriber,
            "checkout",
        );
        let ddb = ddb_client(&[&stream, &binding]);

        let resp = handle(&ddb, request(iam_request_context("111122223333"), ID))
            .await
            .unwrap();

        assert_eq!(resp["statusCode"], 200);
        assert_eq!(body(&resp)["subscriptionId"], ID);
        assert!(body(&resp)["errorDetail"].is_null());
    }

    #[tokio::test]
    async fn failed_stream_exposes_its_error_detail() {
        set_env();
        let mut failed = stream();
        failed.status = StreamStatus::Failed;
        failed.error_detail = Some(
            "schema acme-orders@OrderNotification not found in any environment of 'orders'"
                .to_string(),
        );
        failed.environments.get_mut("prod").unwrap().error_detail =
            Some("PutRule throttled".to_string());
        let stream = stream_rule(Some(failed));
        let ddb = ddb_client(&[&stream]);

        let resp = handle(&ddb, request(oauth_platform_context("platform-admin"), ID))
            .await
            .unwrap();

        assert_eq!(resp["statusCode"], 200);
        let detail = body(&resp);
        assert_eq!(
            detail["errorDetail"],
            "schema acme-orders@OrderNotification not found in any environment of 'orders'"
        );
        assert_eq!(
            detail["environments"]["prod"]["errorDetail"],
            "PutRule throttled"
        );
    }

    #[tokio::test]
    async fn producer_side_binding_sees_the_detail() {
        set_env();
        let stream = stream_rule(Some(stream()));
        // The specific binding rule precedes the catch-all miss: MatchAny
        // evaluates rules in order, and the miss would otherwise swallow the
        // publisher binding lookup too.
        let binding = binding_rule(
            BindingPrincipal::aws_account("444455556666").unwrap(),
            ResourceRole::Publisher,
            "orders",
        );
        let no_subscriber = no_binding_rule();
        let ddb = ddb_client(&[&stream, &binding, &no_subscriber]);

        let resp = handle(&ddb, request(iam_request_context("444455556666"), ID))
            .await
            .unwrap();

        assert_eq!(resp["statusCode"], 200);
    }

    #[tokio::test]
    async fn platform_role_sees_the_detail_without_a_binding() {
        set_env();
        let stream = stream_rule(Some(stream()));
        let ddb = ddb_client(&[&stream]);

        let resp = handle(&ddb, request(oauth_platform_context("platform-admin"), ID))
            .await
            .unwrap();

        assert_eq!(resp["statusCode"], 200);
    }

    #[tokio::test]
    async fn third_party_gets_forbidden_not_not_found() {
        set_env();
        let stream = stream_rule(Some(stream()));
        let no_binding = no_binding_rule();
        let ddb = ddb_client(&[&stream, &no_binding]);

        let resp = handle(&ddb, request(iam_request_context("999900001111"), ID))
            .await
            .unwrap();

        assert_eq!(resp["statusCode"], 403);
        assert_eq!(body(&resp)["code"], "AccessDeniedException");
    }

    #[tokio::test]
    async fn unknown_subscription_is_not_found() {
        set_env();
        let stream = stream_rule(None);
        let ddb = ddb_client(&[&stream]);

        let resp = handle(&ddb, request(iam_request_context("111122223333"), ID))
            .await
            .unwrap();

        assert_eq!(resp["statusCode"], 404);
        assert_eq!(body(&resp)["code"], "ResourceNotFoundException");
    }
}
