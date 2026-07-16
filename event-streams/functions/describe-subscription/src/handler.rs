use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};
use std::env;

use nexus_common::api;
use nexus_common::error::{ControlPlaneError, ValidationExceptionReason};
use nexus_common::streams::SubscriptionId;
use nexus_common::streams_db;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;

    let Some(caller_account_id) = api::caller_account_id(&event.payload) else {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "missing caller identity",
        )));
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
            nexus_common::models::ApiError {
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
            // A subscription is a relationship between two domains; either
            // party has a legitimate interest in seeing it, but a third
            // party does not. Report 404 rather than 403 so the endpoint
            // doesn't confirm the subscription's existence to a non-party.
            match caller_may_view(ddb, &domains_table, &caller_account_id, &stream).await {
                Ok(true) => Ok(api::success_response(
                    200,
                    &json!({
                        "subscriptionId": stream.subscription_id().to_string(),
                        "consumerDomain": stream.consumer_domain,
                        "producerDomain": stream.producer_domain,
                        "source": stream.source,
                        "detailType": stream.detail_type,
                        "schemaName": stream.schema_name,
                        "status": stream.status,
                        "environments": stream.environments,
                        "createdAt": stream.created_at.to_rfc3339(),
                        "updatedAt": stream.updated_at.to_rfc3339(),
                        "removedAt": stream.removed_at.map(|t| t.to_rfc3339()),
                    }),
                )),
                Ok(false) => Ok(not_found()),
                Err(e) => {
                    tracing::error!(error = %e, "ownership check failed");
                    Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                        e.to_string(),
                    )))
                }
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

/// A subscription is a relationship between two domains; either party — the
/// consumer that subscribed or the producer whose contract it pinned — has a
/// legitimate interest in seeing it, but a third party does not.
async fn caller_may_view(
    ddb: &aws_sdk_dynamodb::Client,
    domains_table: &str,
    caller_account_id: &str,
    stream: &nexus_common::streams::EventStream,
) -> Result<bool> {
    for domain in [&stream.consumer_domain, &stream.producer_domain] {
        let output = ddb
            .get_item()
            .table_name(domains_table)
            .key("domainName", nexus_common::dynamodb::s(domain))
            .send()
            .await
            .context("failed to get domain record for ownership check")?;
        let owner = output
            .item
            .as_ref()
            .and_then(|item| nexus_common::dynamodb::get_s(item, "ownerAccountId"));
        if owner.as_deref() == Some(caller_account_id) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lambda_runtime::Context as LambdaContext;

    #[tokio::test]
    async fn malformed_subscription_id_returns_validation_error() {
        std::env::set_var("EVENT_STREAMS_TABLE", "streams");
        std::env::set_var("DOMAINS_TABLE", "domains");
        std::env::set_var("ORGANIZATION", "acme");
        let config = aws_config::SdkConfig::builder()
            .behavior_version(aws_config::BehaviorVersion::latest())
            .build();
        let ddb = aws_sdk_dynamodb::Client::new(&config);

        let event = LambdaEvent::new(
            json!({
                "pathParameters": { "subscriptionId": "not-enough-segments" },
                "requestContext": { "identity": { "accountId": "111122223333" } }
            }),
            LambdaContext::default(),
        );
        let response = handle(&ddb, event).await.unwrap();

        assert_eq!(response["statusCode"], 400);
    }

    #[tokio::test]
    async fn missing_caller_identity_returns_validation_error() {
        std::env::set_var("EVENT_STREAMS_TABLE", "streams");
        std::env::set_var("DOMAINS_TABLE", "domains");
        std::env::set_var("ORGANIZATION", "acme");
        let config = aws_config::SdkConfig::builder()
            .behavior_version(aws_config::BehaviorVersion::latest())
            .build();
        let ddb = aws_sdk_dynamodb::Client::new(&config);

        let event = LambdaEvent::new(
            json!({ "pathParameters": { "subscriptionId": "checkout.orders.OrderNotification" } }),
            LambdaContext::default(),
        );
        let response = handle(&ddb, event).await.unwrap();

        assert_eq!(response["statusCode"], 400);
    }

    fn stream_between(consumer: &str, producer: &str) -> nexus_common::streams::EventStream {
        nexus_common::streams::EventStream {
            producer_domain: producer.to_string(),
            consumer_domain: consumer.to_string(),
            source: format!("acme-{producer}"),
            detail_type: "OrderNotification".to_string(),
            schema_name: format!("acme-{producer}@OrderNotification"),
            status: nexus_common::streams::StreamStatus::Active,
            environments: std::collections::HashMap::new(),
            request_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            removed_at: None,
            expires_at: None,
        }
    }

    fn domain_owner_client(owners: Vec<(&'static str, &'static str)>) -> aws_sdk_dynamodb::Client {
        use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
        use aws_smithy_mocks::{mock, mock_client, RuleMode};

        let rules: Vec<_> = owners
            .into_iter()
            .map(|(domain, owner)| {
                mock!(aws_sdk_dynamodb::Client::get_item)
                    .match_requests(move |req| {
                        req.key()
                            .unwrap()
                            .get("domainName")
                            .and_then(|v| v.as_s().ok())
                            .map(|v| v == domain)
                            .unwrap_or(false)
                    })
                    .then_output(move || {
                        GetItemOutput::builder()
                            .item("domainName", nexus_common::dynamodb::s(domain))
                            .item("ownerAccountId", nexus_common::dynamodb::s(owner))
                            .build()
                    })
            })
            .collect();
        let rule_refs: Vec<_> = rules.iter().collect();
        mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, rule_refs)
    }

    #[tokio::test]
    async fn caller_may_view_true_for_consumer_owner() {
        let ddb = domain_owner_client(vec![
            ("checkout", "111122223333"),
            ("orders", "444455556666"),
        ]);
        let stream = stream_between("checkout", "orders");
        assert!(caller_may_view(&ddb, "domains", "111122223333", &stream)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn caller_may_view_true_for_producer_owner() {
        let ddb = domain_owner_client(vec![
            ("checkout", "111122223333"),
            ("orders", "444455556666"),
        ]);
        let stream = stream_between("checkout", "orders");
        assert!(caller_may_view(&ddb, "domains", "444455556666", &stream)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn caller_may_view_false_for_third_party() {
        let ddb = domain_owner_client(vec![
            ("checkout", "111122223333"),
            ("orders", "444455556666"),
        ]);
        let stream = stream_between("checkout", "orders");
        assert!(!caller_may_view(&ddb, "domains", "999900001111", &stream)
            .await
            .unwrap());
    }
}
