//! Routes one recipient-scoped system event from the system bus to the
//! recipient domain's recorded, ownership-verified SNS topic. The destination
//! always comes from the domain record and must equal the deterministic
//! managed ARN; nothing is derived from event payload beyond selecting the
//! recipient domain.

use std::env;

use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_sns::types::MessageAttributeValue;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::Value;
use tracing::info;

use konductor_common::models::DomainStatus;
use konductor_common::system_events::{
    expected_topic_arn, SystemEvent, SystemEventDetail, SystemEventType,
};

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub sns: aws_sdk_sns::Client,
}

pub async fn handle(clients: &Clients, event: LambdaEvent<Value>) -> Result<Value, Error> {
    let payload = event.payload;
    let detail_type = payload
        .get("detail-type")
        .and_then(Value::as_str)
        .ok_or("event carries no detail-type")?;
    let event_type = SystemEventType::from_detail_type(detail_type)
        .ok_or_else(|| format!("unsupported system detail type: {detail_type}"))?;
    let detail: SystemEventDetail = serde_json::from_value(
        payload
            .get("detail")
            .cloned()
            .ok_or("event carries no detail")?,
    )
    .map_err(|e| format!("malformed system event detail: {e}"))?;
    let domain_name = detail.data.domain.clone();

    let table = env::var("DOMAINS_TABLE").map_err(|_| "DOMAINS_TABLE is required")?;
    let item = clients
        .ddb
        .get_item()
        .table_name(&table)
        .consistent_read(true)
        .key("domainName", AttributeValue::S(domain_name.clone()))
        .send()
        .await
        .map_err(|e| format!("read domain {domain_name}: {e}"))?
        .item
        .ok_or_else(|| format!("domain {domain_name} not found"))?;

    let status = item
        .get("status")
        .and_then(|v| v.as_s().ok())
        .and_then(|s| s.parse::<DomainStatus>().ok());
    if !matches!(status, Some(DomainStatus::Active | DomainStatus::Inactive)) {
        let status = status.map_or_else(|| "unknown".to_string(), |s| s.to_string());
        return Err(
            format!("domain {domain_name} status {status} cannot receive system events").into(),
        );
    }

    let recorded = item
        .get("systemEventsTopicArn")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| {
            format!("domain {domain_name} has no systemEventsTopicArn; run the topic backfill")
        })?;
    let expected = expected_topic_arn(
        &required_env("PLATFORM_PARTITION")?,
        &required_env("AWS_REGION")?,
        &required_env("PLATFORM_ACCOUNT_ID")?,
        &required_env("ORGANIZATION")?,
        &domain_name,
    );
    if recorded != &expected {
        return Err(format!(
            "domain {domain_name} records {recorded}, expected managed topic {expected}; refusing to publish"
        )
        .into());
    }

    let published_at = detail
        .metadata
        .published_at
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let event_model = SystemEvent::new(event_type, String::new(), detail.metadata, detail.data)
        .map_err(|e| format!("event fails contract validation: {e}"))?;
    let attributes = event_model
        .sns_attributes()
        .into_iter()
        .map(|(name, attr)| {
            (
                name,
                MessageAttributeValue::builder()
                    .data_type(attr.data_type.as_str())
                    .string_value(attr.value)
                    .build()
                    .expect("message attribute"),
            )
        })
        .collect();

    clients
        .sns
        .publish()
        .topic_arn(recorded)
        // The full original event is forwarded verbatim — including fields a
        // future contract version adds — so consumers never wait on a router
        // deploy for additive contract changes.
        .message(payload.to_string())
        .set_message_attributes(Some(attributes))
        .send()
        .await
        .map_err(|e| format!("publish to {recorded}: {e}"))?;
    info!(domain = %domain_name, detail_type, "routed system event");
    stamp_last_event(
        &clients.ddb,
        &table,
        &domain_name,
        &published_at,
        detail_type,
    )
    .await;
    Ok(Value::String(recorded.clone()))
}

/// Best-effort freshness marker for the console's unread indicator. Runs
/// after a successful publish; never fails the handler — a lost marker means
/// a stale indicator, while failing here would republish the event. The
/// condition keeps the newest event's timestamp under duplicate or
/// out-of-order delivery.
async fn stamp_last_event(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    domain_name: &str,
    published_at: &str,
    detail_type: &str,
) {
    let result = ddb
        .update_item()
        .table_name(table)
        .key("domainName", AttributeValue::S(domain_name.to_string()))
        .update_expression("SET systemEventsLastEventAt = :at, systemEventsLastEventType = :t")
        .condition_expression(
            "attribute_not_exists(systemEventsLastEventAt) OR systemEventsLastEventAt <= :at",
        )
        .expression_attribute_values(":at", AttributeValue::S(published_at.to_string()))
        .expression_attribute_values(":t", AttributeValue::S(detail_type.to_string()))
        .send()
        .await;
    match result {
        Ok(_) => {}
        Err(e) if is_conditional_failure(&e) => {}
        Err(e) => {
            // Log the SdkError itself: into_service_error() panics on
            // dispatch/timeout errors, which would fail the invocation and
            // republish the event — the exact outcome this marker avoids.
            tracing::warn!(
                domain = domain_name,
                error = %e,
                "latest-event marker write failed; console indicator may be stale"
            );
        }
    }
}

fn is_conditional_failure(
    e: &aws_sdk_dynamodb::error::SdkError<
        aws_sdk_dynamodb::operation::update_item::UpdateItemError,
    >,
) -> bool {
    matches!(
        e.as_service_error(),
        Some(aws_sdk_dynamodb::operation::update_item::UpdateItemError::ConditionalCheckFailedException(_))
    )
}
fn required_env(name: &str) -> Result<String, Error> {
    env::var(name).map_err(|_| format!("{name} environment variable is required").into())
}
#[cfg(test)]
mod tests {
    use super::*;

    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use serde_json::json;

    const ORG: &str = "acme";
    const DOMAIN: &str = "checkout";
    const ACCOUNT: &str = "999988887777";
    const REGION: &str = "eu-west-1";
    const ARN: &str = "arn:aws:sns:eu-west-1:999988887777:acme-checkout-system-events";

    fn set_env() {
        env::set_var("DOMAINS_TABLE", "konductor-domains");
        env::set_var("ORGANIZATION", ORG);
        env::set_var("PLATFORM_PARTITION", "aws");
        env::set_var("PLATFORM_ACCOUNT_ID", ACCOUNT);
        env::set_var("AWS_REGION", REGION);
    }

    fn started_detail() -> Value {
        json!({
            "metadata": {
                "schemaVersion": 1,
                "idempotencyKey": "system-event/v1/abc123",
                "publishedAt": "2026-07-30T10:30:00.123Z",
                "correlationId": "promotion-42",
                "resourceRevision": 5
            },
            "data": {
                "audience": "CONSUMER",
                "domain": DOMAIN,
                "environment": "prod",
                "resource": { "type": "SUBSCRIPTION", "id": "checkout.orders.OrderNotification" },
                "producerDomain": "orders",
                "schemaName": "acme-orders@OrderNotification",
                "trigger": "PROMOTION",
                "fromVersion": 3,
                "requestedVersion": 4,
                "resultingVersion": null,
                "intermediateVersions": null,
                "enabled": true,
                "subscriptionStatus": "ACTIVE",
                "expectedRepinAt": null,
                "contracts": {
                    "from": { "producerDomain": "orders", "schemaName": "acme-orders@OrderNotification", "environment": "prod", "schemaVersion": 3, "metadataRevision": 0 },
                    "requested": { "producerDomain": "orders", "schemaName": "acme-orders@OrderNotification", "environment": "prod", "schemaVersion": 4, "metadataRevision": 0 },
                    "resulting": null
                }
            }
        })
    }

    fn bus_event(detail: Value) -> LambdaEvent<Value> {
        LambdaEvent::new(
            json!({
                "version": "0",
                "id": "evt-1",
                "detail-type": "SubscriptionUpgradeStarted",
                "source": "konductor.system",
                "account": ACCOUNT,
                "time": "2026-07-30T10:30:00.123Z",
                "region": REGION,
                "resources": [],
                "detail": detail
            }),
            Default::default(),
        )
    }

    fn domain_item(
        status: &str,
        topic_arn: Option<&str>,
    ) -> aws_sdk_dynamodb::operation::get_item::GetItemOutput {
        let mut item = std::collections::HashMap::from([
            (
                "domainName".to_string(),
                AttributeValue::S(DOMAIN.to_string()),
            ),
            ("status".to_string(), AttributeValue::S(status.to_string())),
        ]);
        if let Some(arn) = topic_arn {
            item.insert(
                "systemEventsTopicArn".to_string(),
                AttributeValue::S(arn.to_string()),
            );
        }
        aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
            .set_item(Some(item))
            .build()
    }

    fn stamp_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                req.condition_expression()
                    .is_some_and(|c| c.contains("systemEventsLastEventAt"))
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::update_item::UpdateItemOutput::builder().build()
            })
    }

    fn clients(ddb_rules: &[&Rule], sns_rules: &[&Rule]) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
            sns: mock_client!(aws_sdk_sns, RuleMode::MatchAny, sns_rules),
        }
    }

    #[tokio::test]
    async fn routes_to_the_recorded_topic_with_exact_attributes() {
        set_env();
        let get = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.consistent_read() == Some(true)
                    && req
                        .key()
                        .is_some_and(|k| k["domainName"].as_s().is_ok_and(|s| s == DOMAIN))
            })
            .then_output(|| domain_item("ACTIVE", Some(ARN)));
        let publish = mock!(aws_sdk_sns::Client::publish)
            .match_requests(|req| {
                let attrs = req.message_attributes().cloned().unwrap_or_default();
                req.topic_arn() == Some(ARN)
                    && req.message().is_some_and(|m| {
                        m.contains("\"id\":\"evt-1\"")
                            && m.contains("\"idempotencyKey\":\"system-event/v1/abc123\"")
                    })
                    && attrs["eventType"].string_value() == Some("SubscriptionUpgradeStarted")
                    && attrs["eventType"].data_type() == "String"
                    && attrs["schemaVersion"].string_value() == Some("1")
                    && attrs["schemaVersion"].data_type() == "Number"
                    && attrs["domain"].string_value() == Some(DOMAIN)
                    && attrs["environment"].string_value() == Some("prod")
                    && attrs["resourceType"].string_value() == Some("SUBSCRIPTION")
                    && attrs["trigger"].string_value() == Some("PROMOTION")
                    && attrs["audience"].string_value() == Some("CONSUMER")
                    && attrs.len() == 7
                    && !attrs.contains_key("outcome")
            })
            .then_output(|| {
                aws_sdk_sns::operation::publish::PublishOutput::builder()
                    .message_id("m-1")
                    .build()
            });
        let stamp = stamp_rule();
        let clients = clients(&[&get, &stamp], &[&publish]);

        let out = handle(&clients, bus_event(started_detail())).await.unwrap();

        assert_eq!(out, Value::String(ARN.to_string()));
        assert_eq!(publish.num_calls(), 1);
    }

    #[tokio::test]
    async fn publishes_for_inactive_domains_with_retained_topics() {
        set_env();
        let get = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| domain_item("INACTIVE", Some(ARN)));
        let publish = mock!(aws_sdk_sns::Client::publish).then_output(|| {
            aws_sdk_sns::operation::publish::PublishOutput::builder()
                .message_id("m-1")
                .build()
        });
        let stamp = stamp_rule();
        let clients = clients(&[&get, &stamp], &[&publish]);

        handle(&clients, bus_event(started_detail())).await.unwrap();

        assert_eq!(publish.num_calls(), 1);
    }

    #[tokio::test]
    async fn refuses_missing_mismatched_or_foreign_destinations() {
        set_env();
        let publish = mock!(aws_sdk_sns::Client::publish)
            .then_output(|| unreachable!("never publish to an unverified destination"));

        for (status, arn, marker) in [
            ("ACTIVE", None, "no systemEventsTopicArn"),
            (
                "ACTIVE",
                Some("arn:aws:sns:eu-west-1:999988887777:acme-orders-system-events"),
                "refusing to publish",
            ),
            (
                "ACTIVE",
                Some("arn:aws:sns:eu-west-1:123456789012:acme-checkout-system-events"),
                "refusing to publish",
            ),
            ("PROVISIONING", Some(ARN), "cannot receive system events"),
            ("REMOVED", Some(ARN), "cannot receive system events"),
        ] {
            let get = mock!(aws_sdk_dynamodb::Client::get_item)
                .then_output(move || domain_item(status, arn));
            let stamp = stamp_rule();
            let clients = clients(&[&get, &stamp], &[&publish]);

            let error = handle(&clients, bus_event(started_detail()))
                .await
                .unwrap_err();

            assert!(error.to_string().contains(marker), "{marker}: {error}");
        }
        assert_eq!(publish.num_calls(), 0);
    }

    #[tokio::test]
    async fn sns_transient_failure_surfaces_for_eventbridge_retry() {
        set_env();
        let get = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| domain_item("ACTIVE", Some(ARN)));
        let publish = mock!(aws_sdk_sns::Client::publish).then_error(|| {
            aws_sdk_sns::operation::publish::PublishError::generic(
                aws_sdk_sns::error::ErrorMetadata::builder()
                    .message("throttled")
                    .build(),
            )
        });
        let stamp = stamp_rule();
        let clients = clients(&[&get, &stamp], &[&publish]);

        let error = handle(&clients, bus_event(started_detail()))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("publish"), "{error}");
    }

    #[tokio::test]
    async fn duplicate_delivery_publishes_the_identical_message() {
        set_env();
        let get = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| domain_item("ACTIVE", Some(ARN)));
        let publish = mock!(aws_sdk_sns::Client::publish)
            .match_requests(|req| {
                req.message()
                    .is_some_and(|m| m.contains("\"id\":\"evt-1\""))
            })
            .then_output(|| {
                aws_sdk_sns::operation::publish::PublishOutput::builder()
                    .message_id("m-1")
                    .build()
            });
        let stamp = stamp_rule();
        let clients = clients(&[&get, &stamp], &[&publish]);

        handle(&clients, bus_event(started_detail())).await.unwrap();
        handle(&clients, bus_event(started_detail())).await.unwrap();

        assert_eq!(
            publish.num_calls(),
            2,
            "at-least-once: consumers deduplicate on idempotencyKey"
        );
    }

    #[tokio::test]
    async fn unknown_detail_types_are_rejected() {
        set_env();
        let publish = mock!(aws_sdk_sns::Client::publish).then_output(|| unreachable!());
        let clients = clients(&[], &[&publish]);
        let mut event = bus_event(started_detail());
        event.payload["detail-type"] = json!("SomethingElse");

        let error = handle(&clients, event).await.unwrap_err();

        assert!(
            error.to_string().contains("unsupported system detail type"),
            "{error}"
        );
    }
    #[tokio::test]
    async fn successful_publish_stamps_the_latest_event_marker() {
        set_env();
        let get = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("domainName", AttributeValue::S(DOMAIN.to_string()))
                .item("status", AttributeValue::S("ACTIVE".to_string()))
                .item("systemEventsTopicArn", AttributeValue::S(ARN.to_string()))
                .build()
        });
        let stamp = mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                let values = req.expression_attribute_values().unwrap();
                values[":at"]
                    .as_s()
                    .is_ok_and(|s| s == "2026-07-30T10:30:00.123Z")
                    && values[":t"]
                        .as_s()
                        .is_ok_and(|s| s == "SubscriptionUpgradeStarted")
                    && req
                        .condition_expression()
                        .is_some_and(|c| c.contains("attribute_not_exists"))
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::update_item::UpdateItemOutput::builder().build()
            });
        let publish = mock!(aws_sdk_sns::Client::publish).then_output(|| {
            aws_sdk_sns::operation::publish::PublishOutput::builder()
                .message_id("m-1")
                .build()
        });
        let clients = clients(&[&get, &stamp], &[&publish]);

        handle(&clients, bus_event(started_detail())).await.unwrap();

        assert_eq!(stamp.num_calls(), 1);
    }

    #[tokio::test]
    async fn a_newer_recorded_marker_makes_the_stamp_a_noop() {
        set_env();
        let get = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("domainName", AttributeValue::S(DOMAIN.to_string()))
                .item("status", AttributeValue::S("ACTIVE".to_string()))
                .item("systemEventsTopicArn", AttributeValue::S(ARN.to_string()))
                .build()
        });
        let stamp = mock!(aws_sdk_dynamodb::Client::update_item).then_error(|| {
            aws_sdk_dynamodb::operation::update_item::UpdateItemError::ConditionalCheckFailedException(
                aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder().build(),
            )
        });
        let publish = mock!(aws_sdk_sns::Client::publish).then_output(|| {
            aws_sdk_sns::operation::publish::PublishOutput::builder()
                .message_id("m-1")
                .build()
        });
        let clients = clients(&[&get, &stamp], &[&publish]);

        let result = handle(&clients, bus_event(started_detail())).await;

        assert!(result.is_ok(), "superseded marker must not fail routing");
        assert_eq!(publish.num_calls(), 1);
        assert_eq!(stamp.num_calls(), 1);
    }

    #[tokio::test]
    async fn a_failed_marker_write_does_not_fail_or_republish() {
        set_env();
        let get = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("domainName", AttributeValue::S(DOMAIN.to_string()))
                .item("status", AttributeValue::S("ACTIVE".to_string()))
                .item("systemEventsTopicArn", AttributeValue::S(ARN.to_string()))
                .build()
        });
        let stamp = mock!(aws_sdk_dynamodb::Client::update_item).then_error(|| {
            aws_sdk_dynamodb::operation::update_item::UpdateItemError::generic(
                aws_sdk_dynamodb::error::ErrorMetadata::builder()
                    .code("InternalServerError")
                    .message("boom")
                    .build(),
            )
        });
        let publish = mock!(aws_sdk_sns::Client::publish).then_output(|| {
            aws_sdk_sns::operation::publish::PublishOutput::builder()
                .message_id("m-1")
                .build()
        });
        let clients = clients(&[&get, &stamp], &[&publish]);

        let result = handle(&clients, bus_event(started_detail())).await;

        assert!(result.is_ok(), "marker failure must not fail delivery");
        assert_eq!(
            publish.num_calls(),
            1,
            "no republication to retry the marker"
        );
    }
}
