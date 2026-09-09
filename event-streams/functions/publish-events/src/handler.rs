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
//! Dual-format queue: messages without a `format` field are the legacy
//! wrapper (identity in SQS attributes, three-field metadata output);
//! `format: 2` messages carry the admission-resolved identity, frozen
//! metadata values, and the compiled metadata schema in the body. Delivery
//! uses only what the message records — no current-configuration reads, no
//! value regeneration — and validates the composed metadata block (frozen
//! values plus the late-bound `publishedAt`) against the carried schema.
//!
//! Retry semantics: every failure (malformed envelope, missing or
//! inconsistent fields, inactive or missing domain, envelope validation,
//! PutEvents failure) is a processing failure reported for SQS retry with
//! the same immutable message.

use std::env;
use std::time::Instant;

use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use konductor_common::dynamodb::{get_s, s};
use konductor_common::envelope::{published_at_from_sent_timestamp, EventEnvelope, SchemaName};
use konductor_common::models::{DomainStatus, PublishRequest};
use konductor_common::namespace::NamespaceResolver;
use konductor_common::principal_context::sqs_attribute_map;
use konductor_common::publish_queue::{AdmittedCommand, VersionedAdmission};
use konductor_common::schema_validation::ValidatorCache;

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

pub async fn handle(
    clients: &Clients,
    validators: &ValidatorCache,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
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

        if let Err(e) = process_record(clients, validators, record).await {
            tracing::error!(error = %e, message_id, "publish processing failed; reporting for retry");
            batch_item_failures.push(json!({ "itemIdentifier": message_id }));
        }
    }

    Ok(json!({ "batchItemFailures": batch_item_failures }))
}

/// Err reports the record for SQS retry; admission guarantees only valid,
/// authorized commands are ever enqueued, so there is no acknowledge-without-
/// delivery path left here.
async fn process_record(
    clients: &Clients,
    validators: &ValidatorCache,
    record: &Value,
) -> Result<()> {
    let started = Instant::now();
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let resolver = NamespaceResolver::default_pattern();

    match parse_record(record)? {
        ParsedRecord::Legacy(legacy) => {
            deliver_legacy(clients, &resolver, &organization, &domains_table, legacy).await?
        }
        ParsedRecord::Versioned {
            published_at,
            command,
        } => deliver_versioned(clients, validators, &domains_table, command, published_at).await?,
    }

    tracing::info!(
        metric = "PublishDelivered",
        latency_ms = started.elapsed().as_secs_f64() * 1000.0,
        "event published to domain bus"
    );
    Ok(())
}

/// The legacy path, byte-for-byte as before the dual-format rollout: the
/// three-field metadata output and the identity carried in SQS attributes.
async fn deliver_legacy(
    clients: &Clients,
    resolver: &NamespaceResolver,
    organization: &str,
    domains_table: &str,
    legacy: LegacyDelivery,
) -> Result<()> {
    tracing::info!(
        metric = "PublishReceived",
        domain = legacy.request.domain,
        environment = legacy.environment,
        format = "legacy",
        "publish command received"
    );
    require_active_domain(clients, domains_table, &legacy.request.domain).await?;
    let schema_name = SchemaName::for_domain(
        &resolver.namespace(organization, &legacy.request.domain),
        &legacy.request.event_name,
    );
    let envelope = EventEnvelope::new(
        schema_name.source.clone(),
        schema_name.detail_type.clone(),
        resolver.event_bus_name(&legacy.environment, organization, &legacy.request.domain),
        legacy.schema_version,
        legacy.idempotency_key,
        legacy.published_at,
        legacy.request.data.clone(),
    )?;
    put_event(clients, &envelope).await
}

/// The format-2 path: delivery uses exactly what the message records and
/// validates the composed metadata against the carried contract.
async fn deliver_versioned(
    clients: &Clients,
    validators: &ValidatorCache,
    domains_table: &str,
    command: Box<VersionedAdmission>,
    published_at: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    tracing::info!(
        metric = "PublishReceived",
        domain = command.body.domain,
        environment = command.admission.environment,
        format = "2",
        metadata_revision = command.admission.metadata.metadata_revision,
        "publish command received"
    );
    require_active_domain(clients, domains_table, &command.body.domain).await?;
    let envelope = composed_envelope(&command, published_at)?;
    validate_composed_metadata(validators, &command, &envelope)?;
    put_event(clients, &envelope).await
}

/// Builds the new-format envelope from exactly what the message records:
/// resolved identity, target, frozen values, and the late-bound queue
/// timestamp. No current configuration is consulted.
fn composed_envelope(
    command: &VersionedAdmission,
    published_at: chrono::DateTime<chrono::Utc>,
) -> Result<EventEnvelope> {
    let schema = SchemaName::parse(&command.admission.schema_name)?;
    Ok(EventEnvelope::new_composed(
        schema.source,
        schema.detail_type,
        command.admission.event_bus_name.clone(),
        command.admission.schema_version,
        command.admission.idempotency_key.clone(),
        published_at,
        command.admission.metadata.metadata_revision,
        command.admission.metadata.values.clone(),
        command.body.data.clone(),
    )?)
}

/// Validates the composed metadata block — frozen extension values plus the
/// late-bound `publishedAt` — against the compiled metadata schema carried in
/// the message. A schema that does not compile or a block that violates it
/// fails the record for SQS retry; delivery never repairs or regenerates.
fn validate_composed_metadata(
    validators: &ValidatorCache,
    command: &VersionedAdmission,
    envelope: &EventEnvelope,
) -> Result<()> {
    let metadata = serde_json::to_value(&envelope.detail.metadata)
        .context("composed metadata is not serializable")?;
    validators
        .validate(&command.admission.metadata_schema, &metadata)
        .map_err(|e| anyhow::anyhow!("composed metadata failed its carried contract: {e}"))
}

/// The legacy delivery: identity travels in SQS attributes, metadata
/// output is the original three built-in fields.
struct LegacyDelivery {
    environment: String,
    idempotency_key: String,
    schema_version: i64,
    published_at: chrono::DateTime<chrono::Utc>,
    request: PublishRequest,
}

enum ParsedRecord {
    Legacy(LegacyDelivery),
    Versioned {
        published_at: chrono::DateTime<chrono::Utc>,
        command: Box<VersionedAdmission>,
    },
}

fn parse_record(record: &Value) -> Result<ParsedRecord> {
    let attributes = sqs_attribute_map(record);
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
    match AdmittedCommand::parse(body_str) {
        Ok(AdmittedCommand::Legacy(message)) => Ok(ParsedRecord::Legacy(LegacyDelivery {
            environment: required_attribute(&attributes, "Environment")?,
            idempotency_key: required_attribute(&attributes, "IdempotencyKey")?,
            schema_version: required_attribute(&attributes, "SchemaVersion")?
                .parse::<i64>()
                .context("SchemaVersion attribute is not a number")?,
            published_at,
            request: message.body,
        })),
        Ok(AdmittedCommand::Versioned(command)) => {
            check_immutable_consistency(&attributes, &command)?;
            Ok(ParsedRecord::Versioned {
                published_at,
                command,
            })
        }
        Err(e) => Err(anyhow::anyhow!(e.to_string())),
    }
}

/// The SQS routing attributes are stamped at admission alongside the body.
/// A format-2 message whose attributes disagree with its recorded identity
/// fails closed: the message is internally inconsistent and no path may
/// guess which half is right.
fn check_immutable_consistency(
    attributes: &std::collections::HashMap<String, String>,
    command: &VersionedAdmission,
) -> Result<()> {
    let pairs = [
        ("Environment", command.admission.environment.clone()),
        ("IdempotencyKey", command.admission.idempotency_key.clone()),
        (
            "SchemaVersion",
            command.admission.schema_version.to_string(),
        ),
    ];
    for (name, expected) in pairs {
        let actual = required_attribute(attributes, name)?;
        if actual != expected {
            anyhow::bail!(
                "message attribute {name} '{actual}' disagrees with the recorded admission value '{expected}'"
            );
        }
    }
    Ok(())
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

    fn validators() -> ValidatorCache {
        ValidatorCache::new(konductor_common::schema_validation::DEFAULT_VALIDATOR_CACHE_CAPACITY)
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

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![record(admitted_attributes())]),
        )
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

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![record(admitted_attributes())]),
        )
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

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![record(admitted_attributes())]),
        )
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

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![record(admitted_attributes())]),
        )
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

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![record(admitted_attributes())]),
        )
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

        let response = handle(&clients, &validators(), sqs_event(vec![record(broken)]))
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

        let response = handle(&clients, &validators(), sqs_event(vec![record(broken)]))
            .await
            .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }

    const CARRIED_METADATA_SCHEMA: &str =
        r#"{"$schema": "http://json-schema.org/draft-04/schema#", "type": "object"}"#;

    fn versioned_body() -> Value {
        json!({
            "format": 2,
            "callerAccountId": "111122223333",
            "body": {"domain": "orders", "eventName": "OrderNotification", "data": {"orderId": 123}},
            "admission": {
                "environment": "prod",
                "schemaName": "acme-orders@OrderNotification",
                "eventBusName": "acme-prod-orders-event-bus",
                "schemaVersion": 4,
                "idempotencyKey": "key-1",
                "metadata": {
                    "metadataRevision": 2,
                    "definitionDigest": "digest-2",
                    "generation": 1,
                    "values": {"traceId": "trace-abc", "requestSequence": 7}
                },
                "metadataSchema": CARRIED_METADATA_SCHEMA
            }
        })
    }

    fn versioned_record(body: Value, attributes: Vec<(&'static str, String)>) -> Value {
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
            "messageId": "msg-2",
            "body": body.to_string(),
            "messageAttributes": Value::Object(message_attributes),
            "attributes": {"SentTimestamp": SENT_MS}
        })
    }

    fn composed_put_events_rule() -> Rule {
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
                let metadata = &detail["metadata"];
                metadata["schemaVersion"] == json!(4)
                    && metadata["idempotencyKey"] == json!("key-1")
                    && metadata["publishedAt"] == json!("2026-07-30T10:30:00.123Z")
                    && metadata["metadataRevision"] == json!(2)
                    && metadata["traceId"] == json!("trace-abc")
                    && metadata["requestSequence"] == json!(7)
                    // The carried validation descriptor is queue-only.
                    && metadata.get("metadataSchema").is_none()
                    && detail["data"] == json!({"orderId": 123})
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .entries(
                        aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
                            .event_id("evt-2")
                            .build(),
                    )
                    .build()
            })
    }

    #[tokio::test]
    async fn format_two_delivery_stamps_the_frozen_composed_metadata() {
        set_env();
        let clients = clients(vec![active_domain_rule()], vec![composed_put_events_rule()]);

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![versioned_record(
                versioned_body(),
                admitted_attributes(),
            )]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
    }

    #[tokio::test]
    async fn format_two_delivery_keeps_admission_timestamps_distinct_from_queue_time() {
        set_env();
        // The `utc-timestamp` generator freezes the admission instant;
        // `publishedAt` is materialized from the SQS `SentTimestamp`. The two
        // differ legitimately, and delivery recomputes neither.
        let rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                let detail: Value =
                    serde_json::from_str(req.entries()[0].detail().unwrap()).unwrap();
                detail["metadata"]["admittedAt"] == json!("2026-07-30T10:29:59.999Z")
                    && detail["metadata"]["publishedAt"] == json!("2026-07-30T10:30:00.123Z")
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .entries(
                        aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
                            .event_id("evt-timestamps")
                            .build(),
                    )
                    .build()
            });
        let clients = clients(vec![active_domain_rule()], vec![rule]);

        let mut body = versioned_body();
        body["admission"]["metadata"]["values"] = json!({"admittedAt": "2026-07-30T10:29:59.999Z"});

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![versioned_record(body, admitted_attributes())]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
    }

    #[tokio::test]
    async fn sqs_retries_reuse_frozen_values_without_configuration_reads() {
        set_env();
        // One MatchAny rule accepts both deliveries: the retry's composed
        // detail must be byte-identical, frozen values included. The only
        // DDB rule is the domain liveness read — delivery has no metadata
        // configuration path at all.
        let put = composed_put_events_rule();
        let clients = clients(vec![active_domain_rule()], vec![put.clone()]);
        let same_record = versioned_record(versioned_body(), admitted_attributes());

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![same_record.clone(), same_record]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
        assert_eq!(
            put.num_calls(),
            2,
            "both deliveries carry the identical composed detail"
        );
    }

    #[tokio::test]
    async fn format_two_attribute_inconsistency_fails_closed() {
        set_env();
        // Zero EventBridge rules: an inconsistent message must never deliver.
        let clients = clients(vec![active_domain_rule()], vec![]);
        let inconsistent: Vec<(&'static str, String)> = admitted_attributes()
            .into_iter()
            .map(|(k, v)| (k, if k == "SchemaVersion" { "5".into() } else { v }))
            .collect();

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![versioned_record(versioned_body(), inconsistent)]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-2"]);
    }

    #[tokio::test]
    async fn format_two_missing_attribute_fails_closed() {
        set_env();
        let clients = clients(vec![], vec![]);
        let broken: Vec<(&'static str, String)> = admitted_attributes()
            .into_iter()
            .filter(|(k, _)| *k != "IdempotencyKey")
            .collect();

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![versioned_record(versioned_body(), broken)]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-2"]);
    }

    #[tokio::test]
    async fn an_unsupported_queue_format_fails_closed() {
        set_env();
        // Zero rules on every client: an unsupported format must stop before
        // any resource access.
        let clients = clients(vec![], vec![]);
        let mut body = versioned_body();
        body["format"] = json!(3);

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![versioned_record(body, admitted_attributes())]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-2"]);
    }

    #[tokio::test]
    async fn a_carried_schema_violation_fails_closed() {
        set_env();
        // The carried contract demands a field the frozen block lacks.
        // Delivery validates against exactly what the message carries and
        // never repairs the block.
        let clients = clients(vec![active_domain_rule()], vec![]);
        let mut body = versioned_body();
        body["admission"]["metadataSchema"] = json!(
            r#"{"$schema": "http://json-schema.org/draft-04/schema#", "type": "object", "required": ["traceId", "absentField"]}"#
        );

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![versioned_record(body, admitted_attributes())]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-2"]);
    }

    #[tokio::test]
    async fn a_carried_schema_that_does_not_compile_fails_closed() {
        set_env();
        let clients = clients(vec![active_domain_rule()], vec![]);
        let mut body = versioned_body();
        body["admission"]["metadataSchema"] = json!(r#"{"type": "object", "required": 5}"#);

        let response = handle(
            &clients,
            &validators(),
            sqs_event(vec![versioned_record(body, admitted_attributes())]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-2"]);
    }
}
