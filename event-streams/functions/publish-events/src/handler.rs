//! Delivery-only publish consumer: reads admitted publish commands from SQS,
//! re-checks the domain is ACTIVE, builds the canonical envelope stamped with
//! the admission-resolved schema version, and calls PutEvents on the domain
//! bus.
//!
//! Every admission decision (authorization, schema resolution, payload
//! validation) already happened synchronously in the publish-events-admission
//! Lambda before the command was enqueued; queue writes are restricted to that
//! function. This consumer performs no authorization evaluation, no registry
//! reads, and no payload validation — the `SchemaVersion` message attribute is
//! the version the payload was validated against, so the stamp can never drift
//! from the validated definition.
//!
//! Retry semantics: every failure (malformed envelope, missing attributes,
//! inactive or missing domain, envelope validation, PutEvents failure) is a
//! processing failure reported for SQS retry with the same immutable message
//! attributes.

use std::env;
use std::time::Instant;

use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde::Deserialize;
use serde_json::{json, Value};

use konductor_common::dynamodb::{get_s, s};
use konductor_common::envelope::{published_at_from_sent_timestamp, EventEnvelope, SchemaName};
use konductor_common::models::{DomainStatus, PublishRequest};
use konductor_common::namespace::NamespaceResolver;
use konductor_common::principal_context::sqs_attribute_map;

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub eb: aws_sdk_eventbridge::Client,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: aws_sdk_dynamodb::Client::new(config),
            eb: aws_sdk_eventbridge::Client::new(config),
        }
    }
}

/// The SQS message wrapper mirrors the other write commands
/// (`callerAccountId` plus `body`); only the body is consumed here.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishMessage {
    pub body: PublishRequest,
}

pub async fn handle(clients: &Clients, event: LambdaEvent<Value>) -> Result<Value, Error> {
    let records = event
        .payload
        .get("Records")
        .and_then(|r| r.as_array())
        .context("missing Records array")?;

    let mut batch_item_failures = Vec::new();

    for record in records {
        let Some(message_id) = record.get("messageId").and_then(|v| v.as_str()) else {
            tracing::error!("skipping SQS record without messageId");
            continue;
        };

        if let Err(e) = process_record(clients, record).await {
            tracing::error!(error = %e, message_id, "publish processing failed; reporting for retry");
            batch_item_failures.push(json!({ "itemIdentifier": message_id }));
        }
    }

    Ok(json!({ "batchItemFailures": batch_item_failures }))
}

/// Err reports the record for SQS retry; admission guarantees only valid,
/// authorized commands are ever enqueued, so there is no acknowledge-without-
/// delivery path left here.
async fn process_record(clients: &Clients, record: &Value) -> Result<()> {
    let started = Instant::now();
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let resolver = NamespaceResolver::default_pattern();

    let parsed = parse_record(record)?;
    let request = &parsed.request;

    tracing::info!(
        metric = "PublishReceived",
        domain = request.domain,
        environment = parsed.environment,
        "publish command received"
    );

    require_active_domain(clients, &domains_table, &request.domain).await?;

    let schema_name = SchemaName::for_domain(
        &resolver.namespace(&organization, &request.domain),
        &request.event_name,
    );
    let envelope = EventEnvelope::new(
        schema_name.source.clone(),
        schema_name.detail_type.clone(),
        resolver.event_bus_name(&parsed.environment, &organization, &request.domain),
        parsed.schema_version,
        parsed.idempotency_key.clone(),
        parsed.published_at,
        request.data.clone(),
    )?;

    put_event(clients, &envelope).await?;

    tracing::info!(
        metric = "PublishDelivered",
        domain = request.domain,
        environment = parsed.environment,
        latency_ms = started.elapsed().as_secs_f64() * 1000.0,
        queue_latency_ms = (chrono::Utc::now() - parsed.published_at).num_milliseconds(),
        "event published to domain bus"
    );
    Ok(())
}

struct ParsedRecord {
    environment: String,
    idempotency_key: String,
    schema_version: i64,
    published_at: chrono::DateTime<chrono::Utc>,
    request: PublishRequest,
}

fn parse_record(record: &Value) -> Result<ParsedRecord> {
    let attributes = sqs_attribute_map(record);
    let environment = required_attribute(&attributes, "Environment")?;
    let idempotency_key = required_attribute(&attributes, "IdempotencyKey")?;
    let schema_version = required_attribute(&attributes, "SchemaVersion")?
        .parse::<i64>()
        .context("SchemaVersion attribute is not a number")?;
    let sent_timestamp = record
        .get("attributes")
        .and_then(|a| a.get("SentTimestamp"))
        .and_then(|v| v.as_str())
        .context("missing SQS SentTimestamp attribute")?;
    let published_at = published_at_from_sent_timestamp(sent_timestamp)?;

    let body_str = record
        .get("body")
        .and_then(|b| b.as_str())
        .context("missing SQS record body")?;
    let message: PublishMessage =
        serde_json::from_str(body_str).context("invalid PublishMessage")?;

    Ok(ParsedRecord {
        environment,
        idempotency_key,
        schema_version,
        published_at,
        request: message.body,
    })
}

fn required_attribute(
    attributes: &std::collections::HashMap<String, String>,
    name: &'static str,
) -> Result<String> {
    attributes
        .get(name)
        .filter(|v| !v.is_empty())
        .cloned()
        .with_context(|| format!("missing message attribute {name}"))
}

async fn require_active_domain(clients: &Clients, table: &str, domain_name: &str) -> Result<()> {
    let output = clients
        .ddb
        .get_item()
        .table_name(table)
        .key("domainName", s(domain_name))
        .send()
        .await
        .context("failed to get domain record")?;

    let Some(item) = output.item else {
        anyhow::bail!("domain '{domain_name}' not found");
    };
    let status = get_s(&item, "status").unwrap_or_default();
    if status.parse::<DomainStatus>() != Ok(DomainStatus::Active) {
        anyhow::bail!("domain '{domain_name}' is not ACTIVE (status '{status}')");
    }
    Ok(())
}

async fn put_event(clients: &Clients, envelope: &EventEnvelope) -> Result<()> {
    let detail = serde_json::to_string(&envelope.detail).context("envelope not serializable")?;
    let put_started = Instant::now();
    let result = clients
        .eb
        .put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(&envelope.event_bus_name)
                .source(&envelope.source)
                .detail_type(&envelope.detail_type)
                .detail(detail)
                .build(),
        )
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("PutEvents failed: {}", e.into_service_error()))?;

    if result.failed_entry_count > 0 {
        let reason = result
            .entries()
            .first()
            .and_then(|e| e.error_message.clone())
            .unwrap_or_default();
        tracing::warn!(
            metric = "PublishPutEventsRejected",
            latency_ms = put_started.elapsed().as_secs_f64() * 1000.0,
            "EventBridge rejected the entry"
        );
        anyhow::bail!("PutEvents rejected the entry: {reason}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};

    const SENT_MS: &str = "1785407400123"; // 2026-07-30T10:30:00.123Z

    fn set_env() {
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var("DOMAINS_TABLE", "domains-test");
    }

    fn admitted_attributes() -> Vec<(&'static str, String)> {
        vec![
            ("PrincipalVersion", "1".into()),
            ("AuthMechanism", "oauth".into()),
            ("PrincipalId", "sub-123".into()),
            ("Issuer", "https://issuer.example.com".into()),
            ("ClientId", "client-1".into()),
            ("GlobalRoles", "[]".into()),
            ("TeamGroups", "[\"team-orders\"]".into()),
            ("Environment", "prod".into()),
            ("IdempotencyKey", "key-1".into()),
            ("SchemaVersion", "4".into()),
        ]
    }

    fn record(attributes: Vec<(&'static str, String)>) -> Value {
        let message_attributes: serde_json::Map<String, Value> = attributes
            .into_iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    json!({"stringValue": v, "dataType": "String"}),
                )
            })
            .collect();
        json!({
            "messageId": "msg-1",
            "body": json!({
                "callerAccountId": "111122223333",
                "body": {
                    "domain": "orders",
                    "eventName": "OrderNotification",
                    "data": {"orderId": 123, "amount": 99.5}
                }
            })
            .to_string(),
            "messageAttributes": Value::Object(message_attributes),
            "attributes": {"SentTimestamp": SENT_MS}
        })
    }

    fn sqs_event(records: Vec<Value>) -> LambdaEvent<Value> {
        LambdaEvent::new(json!({"Records": records}), Default::default())
    }

    fn failures(response: &Value) -> Vec<&str> {
        response["batchItemFailures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["itemIdentifier"].as_str().unwrap())
            .collect()
    }

    fn active_domain_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.table_name() == Some("domains-test")
                    && req.key().and_then(|k| k.get("domainName")) == Some(&s("orders"))
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .item("domainName", s("orders"))
                    .item("status", s("ACTIVE"))
                    .build()
            })
    }

    fn put_events_rule() -> Rule {
        mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                let entry = &req.entries()[0];
                if entry.event_bus_name() != Some("acme-prod-orders-event-bus")
                    || entry.source() != Some("acme-orders")
                    || entry.detail_type() != Some("OrderNotification")
                {
                    return false;
                }
                let detail: Value = serde_json::from_str(entry.detail().unwrap()).unwrap();
                detail["metadata"]["schemaVersion"] == json!(4)
                    && detail["metadata"]["idempotencyKey"] == json!("key-1")
                    && detail["metadata"]["publishedAt"] == json!("2026-07-30T10:30:00.123Z")
                    && detail["data"] == json!({"orderId": 123, "amount": 99.5})
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .entries(
                        aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
                            .event_id("evt-1")
                            .build(),
                    )
                    .build()
            })
    }

    fn clients(ddb_rules: Vec<Rule>, eb_rules: Vec<Rule>) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, &ddb_rules),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, &eb_rules),
        }
    }

    #[tokio::test]
    async fn admitted_command_delivers_the_admission_stamped_envelope() {
        set_env();
        let clients = clients(vec![active_domain_rule()], vec![put_events_rule()]);

        let response = handle(&clients, sqs_event(vec![record(admitted_attributes())]))
            .await
            .unwrap();

        assert!(failures(&response).is_empty());
    }

    #[tokio::test]
    async fn publish_to_inactive_domain_is_rejected_without_delivery() {
        set_env();
        let inactive_domain_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.table_name() == Some("domains-test"))
            .then_output(|| {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .item("domainName", s("orders"))
                    .item("status", s("INACTIVE"))
                    .build()
            });
        // No EventBridge rule: an INACTIVE domain must stop delivery.
        let clients = clients(vec![inactive_domain_rule], vec![]);

        let response = handle(&clients, sqs_event(vec![record(admitted_attributes())]))
            .await
            .unwrap();

        assert_eq!(failures(&response).len(), 1, "inactive domain rejects");
    }

    #[tokio::test]
    async fn missing_domain_is_reported_for_retry() {
        set_env();
        let absent = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build()
        });
        let clients = clients(vec![absent], vec![]);

        let response = handle(&clients, sqs_event(vec![record(admitted_attributes())]))
            .await
            .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }

    #[tokio::test]
    async fn put_events_api_error_is_reported_for_retry() {
        set_env();
        let eb_error = mock!(aws_sdk_eventbridge::Client::put_events).then_error(|| {
            aws_sdk_eventbridge::operation::put_events::PutEventsError::generic(
                aws_sdk_eventbridge::error::ErrorMetadata::builder()
                    .message("throttled")
                    .build(),
            )
        });
        let clients = clients(vec![active_domain_rule()], vec![eb_error]);

        let response = handle(&clients, sqs_event(vec![record(admitted_attributes())]))
            .await
            .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }

    #[tokio::test]
    async fn put_events_failed_entry_is_reported_for_retry() {
        set_env();
        let failed_entry = mock!(aws_sdk_eventbridge::Client::put_events).then_output(|| {
            aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                .failed_entry_count(1)
                .entries(
                    aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
                        .error_code("InternalFailure")
                        .error_message("boom")
                        .build(),
                )
                .build()
        });
        let clients = clients(vec![active_domain_rule()], vec![failed_entry]);

        let response = handle(&clients, sqs_event(vec![record(admitted_attributes())]))
            .await
            .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }

    #[tokio::test]
    async fn missing_schema_version_attribute_is_reported_for_retry() {
        set_env();
        // Zero rules: a missing admission stamp must stop before any resource
        // access.
        let clients = clients(vec![], vec![]);
        let broken: Vec<(&'static str, String)> = admitted_attributes()
            .into_iter()
            .filter(|(k, _)| *k != "SchemaVersion")
            .collect();

        let response = handle(&clients, sqs_event(vec![record(broken)]))
            .await
            .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }

    #[tokio::test]
    async fn missing_environment_attribute_is_reported_for_retry() {
        set_env();
        let clients = clients(vec![], vec![]);
        let broken: Vec<(&'static str, String)> = admitted_attributes()
            .into_iter()
            .filter(|(k, _)| *k != "Environment")
            .collect();

        let response = handle(&clients, sqs_event(vec![record(broken)]))
            .await
            .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }
}
