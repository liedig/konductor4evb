//! Delivery-only publish consumer: reads admitted publish commands from SQS,
//! re-checks the domain is ACTIVE, builds the canonical envelope stamped with
//! the admission-resolved schema version, and delivers entries to domain
//! buses in shared `PutEvents` requests.
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
//! Delivery: each batch is prepared per record (parse, liveness re-check,
//! envelope build, carried-metadata validation), grouped by target bus, and
//! sent concurrently in `PutEvents` requests bounded by EventBridge's
//! entry-count and size limits. A failed entry maps back to its SQS message
//! by index. A domain's `ACTIVE` confirmation is reused for at most the
//! configured liveness window; `INACTIVE` and missing domains are always
//! re-read.
//!
//! Retry semantics: throttled entries are retried inside the invocation with
//! jittered backoff; entries still throttled are rescheduled with a short
//! per-message visibility backoff that grows with the receive count and are
//! reported for retry. Every other failure (malformed envelope, missing or
//! inconsistent fields, inactive or missing domain, envelope validation,
//! non-throttle PutEvents failure) is a processing failure reported for SQS
//! retry with the standard visibility timeout and the same immutable
//! message.

mod parse;
mod send;

use std::env;
use std::time::Duration;

use anyhow::{Context, Result};
use aws_sdk_eventbridge::types::PutEventsRequestEntry;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use konductor_common::backoff::JitterSource;
use konductor_common::domain_liveness::{Clock, DomainLivenessCache};
use konductor_common::dynamodb::{get_s, s};
use konductor_common::envelope::{put_events_entry_bytes, EventEnvelope, SchemaName};
use konductor_common::models::DomainStatus;
use konductor_common::namespace::NamespaceResolver;
use konductor_common::publish_queue::VersionedAdmission;
use konductor_common::schema_validation::ValidatorCache;

use parse::ParsedRecord;

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub eb: aws_sdk_eventbridge::Client,
    pub sqs: aws_sdk_sqs::Client,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: aws_sdk_dynamodb::Client::new(config),
            eb: aws_sdk_eventbridge::Client::new(config),
            sqs: aws_sdk_sqs::Client::new(config),
        }
    }
}

/// A record that passed preparation and is ready to send: the `PutEvents`
/// entry built once from its envelope (so the detail is serialized exactly
/// once), the entry's measured size, the domain its liveness was confirmed
/// for, and the SQS fields retry handling and latency reporting need.
pub(crate) struct PreparedEntry {
    pub(crate) message_id: String,
    pub(crate) receipt_handle: Option<String>,
    pub(crate) receive_count: u32,
    pub(crate) sent_timestamp_ms: i64,
    pub(crate) domain: String,
    pub(crate) request_entry: PutEventsRequestEntry,
    pub(crate) size_bytes: usize,
}

/// The two publish-path latencies, both measured from the queue's
/// `SentTimestamp`: how long the message waited before this invocation
/// started, and how long queue-to-EventBridge acceptance took in total.
fn delivery_latency(
    sent_timestamp_ms: i64,
    invocation_started_ms: i64,
    accepted_ms: i64,
) -> (i64, i64) {
    (
        invocation_started_ms - sent_timestamp_ms,
        accepted_ms - sent_timestamp_ms,
    )
}

pub async fn handle<C: Clock>(
    clients: &Clients,
    validators: &ValidatorCache,
    liveness: &DomainLivenessCache<C>,
    jitter: &dyn JitterSource,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let invocation_started_ms = chrono::Utc::now().timestamp_millis();
    let deadline_ms = event.context.deadline;
    let records = event
        .payload
        .get("Records")
        .and_then(|r| r.as_array())
        .context("missing Records array")?;

    let (prepared, mut failed) = prepare_all(clients, validators, liveness, records).await;

    let remaining = move || {
        Duration::from_millis(
            deadline_ms.saturating_sub(chrono::Utc::now().timestamp_millis().max(0) as u64),
        )
    };
    failed.extend(record_outcomes(
        send::deliver(clients, liveness, jitter, prepared, &remaining).await,
        invocation_started_ms,
    ));

    Ok(json!({
        "batchItemFailures": failed
            .into_iter()
            .map(|id| json!({ "itemIdentifier": id }))
            .collect::<Vec<_>>()
    }))
}

/// Parses, re-checks, and prepares every record, collecting per-message
/// failures for SQS retry alongside the entries ready to send.
async fn prepare_all<C: Clock>(
    clients: &Clients,
    validators: &ValidatorCache,
    liveness: &DomainLivenessCache<C>,
    records: &[Value],
) -> (Vec<PreparedEntry>, Vec<String>) {
    let mut prepared = Vec::with_capacity(records.len());
    let mut failed: Vec<String> = Vec::new();
    for record in records {
        let Some(message_id) = record
            .get("messageId")
            .and_then(|v| v.as_str())
            .map(str::to_string)
        else {
            tracing::error!("skipping SQS record without messageId");
            continue;
        };
        match prepare(clients, validators, liveness, record).await {
            Ok(entry) => prepared.push(entry),
            Err(error) => {
                tracing::error!(
                    metric = "PublishDeliveryRetry",
                    error = %error,
                    message_id,
                    "publish processing failed; reporting for retry"
                );
                failed.push(message_id);
            }
        }
    }
    (prepared, failed)
}

/// Logs each delivery outcome and returns the message ids to report for
/// retry: throttled entries (already rescheduled with backoff) and failures.
fn record_outcomes(
    outcomes: Vec<send::DeliveryOutcome>,
    invocation_started_ms: i64,
) -> Vec<String> {
    let mut failed = Vec::new();
    for outcome in outcomes {
        match outcome {
            send::DeliveryOutcome::Delivered {
                sent_timestamp_ms,
                accepted_ms,
            } => {
                let (queue_wait_ms, enqueue_to_delivery_ms) =
                    delivery_latency(sent_timestamp_ms, invocation_started_ms, accepted_ms);
                tracing::info!(
                    metric = "PublishDelivered",
                    queue_wait_ms,
                    enqueue_to_delivery_ms,
                    "event published to domain bus"
                );
            }
            send::DeliveryOutcome::Throttled { message_id } => {
                tracing::warn!(
                    metric = "PublishDeliveryRetry",
                    message_id,
                    "delivery throttled; message rescheduled with backoff"
                );
                failed.push(message_id);
            }
            send::DeliveryOutcome::Failed { message_id, error } => {
                tracing::error!(
                    metric = "PublishDeliveryRetry",
                    message_id,
                    error = %error,
                    "delivery failed; reporting for retry"
                );
                failed.push(message_id);
            }
        }
    }
    failed
}

/// Prepare phase: parse, liveness re-check, envelope build, and the carried
/// metadata validation, exactly as the serial path did per record. Any
/// failure is a per-message failure reported for SQS retry; admission
/// guarantees only valid, authorized commands are ever enqueued, so there is
/// no acknowledge-without-delivery path left here.
async fn prepare<C: Clock>(
    clients: &Clients,
    validators: &ValidatorCache,
    liveness: &DomainLivenessCache<C>,
    record: &Value,
) -> Result<PreparedEntry> {
    let message = parse::parse_message(record)?;
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let resolver = NamespaceResolver::default_pattern();
    let domain = match &message.record {
        ParsedRecord::Legacy(legacy) => legacy.request.domain.clone(),
        ParsedRecord::Versioned { command } => command.body.domain.clone(),
    };

    let envelope = match &message.record {
        ParsedRecord::Legacy(legacy) => {
            tracing::info!(
                metric = "PublishReceived",
                domain = legacy.request.domain,
                environment = legacy.environment,
                format = "legacy",
                "publish command received"
            );
            require_active_domain(clients, liveness, &domains_table, &domain).await?;
            let schema_name = SchemaName::for_domain(
                &resolver.namespace(&organization, &legacy.request.domain),
                &legacy.request.event_name,
            );
            EventEnvelope::new(
                schema_name.source,
                schema_name.detail_type,
                resolver.event_bus_name(&legacy.environment, &organization, &legacy.request.domain),
                legacy.schema_version,
                legacy.idempotency_key.clone(),
                message.published_at,
                legacy.request.data.clone(),
            )?
        }
        ParsedRecord::Versioned { command } => {
            tracing::info!(
                metric = "PublishReceived",
                domain = command.body.domain,
                environment = command.admission.environment,
                format = "2",
                metadata_revision = command.admission.metadata.metadata_revision,
                "publish command received"
            );
            require_active_domain(clients, liveness, &domains_table, &domain).await?;
            let envelope = composed_envelope(command, message.published_at)?;
            validate_composed_metadata(validators, command, &envelope)?;
            envelope
        }
    };

    let detail_json =
        serde_json::to_string(&envelope.detail).context("envelope not serializable")?;
    let size_bytes = put_events_entry_bytes(
        &envelope.source,
        &envelope.detail_type,
        &envelope.event_bus_name,
        &detail_json,
    );
    let request_entry = PutEventsRequestEntry::builder()
        .event_bus_name(envelope.event_bus_name)
        .source(envelope.source)
        .detail_type(envelope.detail_type)
        .detail(detail_json)
        .build();
    Ok(PreparedEntry {
        message_id: message.message_id,
        receipt_handle: message.receipt_handle,
        receive_count: message.receive_count,
        sent_timestamp_ms: message.published_at.timestamp_millis(),
        domain,
        request_entry,
        size_bytes,
    })
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

/// Re-checks domain liveness, reusing a fresh `ACTIVE` confirmation from the
/// cache. `INACTIVE` and missing domains are never cached, so a re-enabled
/// domain delivers on its next message. A store failure propagates and the
/// message is reported for retry. The read is strongly consistent, so a
/// fresh confirmation cannot trail a disable.
pub(crate) async fn require_active_domain<C: Clock>(
    clients: &Clients,
    liveness: &DomainLivenessCache<C>,
    table: &str,
    domain_name: &str,
) -> Result<()> {
    let active = liveness
        .confirm_active(domain_name, || async {
            let output = clients
                .ddb
                .get_item()
                .table_name(table)
                .key("domainName", s(domain_name))
                .consistent_read(true)
                .send()
                .await
                .context("failed to get domain record")?;
            let Some(item) = output.item else {
                return Ok::<bool, anyhow::Error>(false);
            };
            let status = get_s(&item, "status").unwrap_or_default();
            Ok(status.parse::<DomainStatus>() == Ok(DomainStatus::Active))
        })
        .await?;
    if !active {
        anyhow::bail!("domain '{domain_name}' is not ACTIVE or does not exist");
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
        std::env::set_var("PUBLISH_QUEUE_URL", "https://sqs.test/publish-queue");
    }

    use std::time::Duration;

    fn no_liveness_cache() -> DomainLivenessCache {
        DomainLivenessCache::new(Duration::ZERO)
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
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, &[]),
        }
    }

    #[tokio::test]
    async fn a_send_task_failure_reports_the_chunk_messages_for_retry_instead_of_losing_them() {
        set_env();
        let clients = clients(vec![active_domain_rule()], vec![]);
        let evt = LambdaEvent::new(
            json!({"Records": [legacy_record("msg-1", "key-1", "orders", json!({}))]}),
            Default::default(),
        );

        let resp = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            evt,
        )
        .await
        .unwrap();

        let failed: Vec<&str> = resp["batchItemFailures"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item["itemIdentifier"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            failed,
            vec!["msg-1"],
            "a crashed send task must still report its messages for SQS retry"
        );
    }

    #[tokio::test]
    async fn admitted_command_delivers_the_admission_stamped_envelope() {
        set_env();
        let clients = clients(vec![active_domain_rule()], vec![put_events_rule()]);

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
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
            &no_liveness_cache(),
            &ZeroJitter,
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
            &no_liveness_cache(),
            &ZeroJitter,
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
            &no_liveness_cache(),
            &ZeroJitter,
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
            &no_liveness_cache(),
            &ZeroJitter,
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

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(vec![record(broken)]),
        )
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

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(vec![record(broken)]),
        )
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
        versioned_record_named("msg-2", body, attributes)
    }

    fn versioned_record_named(
        message_id: &str,
        body: Value,
        attributes: Vec<(&'static str, String)>,
    ) -> Value {
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
            "messageId": message_id,
            "body": body.to_string(),
            "messageAttributes": Value::Object(message_attributes),
            "attributes": {"SentTimestamp": SENT_MS}
        })
    }

    fn ok_entry(event_id: &'static str) -> aws_sdk_eventbridge::types::PutEventsResultEntry {
        aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
            .event_id(event_id)
            .build()
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
            &no_liveness_cache(),
            &ZeroJitter,
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
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(vec![versioned_record(body, admitted_attributes())]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
    }

    #[tokio::test]
    async fn format_two_replay_re_delivers_identical_frozen_values() {
        set_env();
        // A replay (SQS redelivery of the same admitted command) delivers the
        // frozen values and the recorded business schema version verbatim —
        // subscription version matching stays anchored to what admission
        // validated. At-least-once means the duplicate is expected; there is
        // no cross-request deduplication to assert. Both records target the
        // same bus, so they share one request with byte-identical entries.
        let rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                if req.entries().len() != 2 {
                    return false;
                }
                let detail: Value =
                    serde_json::from_str(req.entries()[0].detail().unwrap()).unwrap();
                req.entries()[0].detail() == req.entries()[1].detail()
                    && detail["metadata"]["traceId"] == json!("trace-abc")
                    && detail["metadata"]["requestSequence"] == json!(7)
                    && detail["metadata"]["metadataRevision"] == json!(2)
                    && detail["metadata"]["schemaVersion"] == json!(4)
                    && detail["metadata"]["publishedAt"] == json!("2026-07-30T10:30:00.123Z")
            })
            .then_output(|| put_events_success_output(2));
        let clients = clients(vec![active_domain_rule()], vec![rule.clone()]);

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(vec![
                versioned_record_named("msg-a", versioned_body(), admitted_attributes()),
                versioned_record_named("msg-b", versioned_body(), admitted_attributes()),
            ]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
        assert_eq!(
            rule.num_calls(),
            1,
            "the replayed message is delivered again with identical frozen values"
        );
    }

    #[tokio::test]
    async fn format_two_partial_put_events_failure_is_reported_for_retry() {
        set_env();
        // PutEvents fails for this entry: the record is reported for SQS
        // retry; the later replay carries the same frozen values.
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
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(vec![versioned_record(
                versioned_body(),
                admitted_attributes(),
            )]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-2"]);
    }

    #[tokio::test]
    async fn format_two_delivery_after_activation_and_configuration_outage_uses_recorded_state() {
        set_env();
        // The environment has since activated a new revision (the recorded
        // generation 1 / revision 2 is now stale) and the configuration
        // table is unreachable. Delivery never reads configuration, so the
        // paused message still delivers exactly the recorded frozen values.
        let rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                let detail: Value =
                    serde_json::from_str(req.entries()[0].detail().unwrap()).unwrap();
                detail["metadata"]
                    == json!({
                        "schemaVersion": 4,
                        "idempotencyKey": "key-1",
                        "publishedAt": "2026-07-30T10:30:00.123Z",
                        "metadataRevision": 2,
                        "traceId": "trace-abc",
                        "requestSequence": 7
                    })
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .entries(ok_entry("evt-paused"))
                    .build()
            });
        let clients = clients(vec![active_domain_rule()], vec![rule]);

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
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
    async fn sqs_retries_reuse_frozen_values_without_configuration_reads() {
        set_env();
        // Both records carry the same admitted command: the composed
        // details must be byte-identical, frozen values included, and they
        // share one PutEvents request. The only DDB rule is the domain
        // liveness read — delivery has no metadata configuration path at all.
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                req.entries().len() == 2 && req.entries()[0].detail() == req.entries()[1].detail()
            })
            .then_output(|| put_events_success_output(2));
        let clients = clients(vec![active_domain_rule()], vec![put.clone()]);
        let same_record = versioned_record(versioned_body(), admitted_attributes());

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(vec![same_record.clone(), same_record]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
        assert_eq!(
            put.num_calls(),
            1,
            "both deliveries carry the identical composed detail in one shared request"
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
            &no_liveness_cache(),
            &ZeroJitter,
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
            &no_liveness_cache(),
            &ZeroJitter,
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
            &no_liveness_cache(),
            &ZeroJitter,
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
            &no_liveness_cache(),
            &ZeroJitter,
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
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(vec![versioned_record(body, admitted_attributes())]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-2"]);
    }
    fn record_with_id(
        message_id: &str,
        body: Value,
        attributes: Vec<(&'static str, String)>,
    ) -> Value {
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
            "messageId": message_id,
            "body": body.to_string(),
            "messageAttributes": Value::Object(message_attributes),
            "attributes": {"SentTimestamp": SENT_MS},
        })
    }

    fn legacy_record(message_id: &str, idempotency_key: &str, domain: &str, data: Value) -> Value {
        let attributes: Vec<(&'static str, String)> = admitted_attributes()
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    if k == "IdempotencyKey" {
                        idempotency_key.to_string()
                    } else {
                        v
                    },
                )
            })
            .collect();
        record_with_id(
            message_id,
            json!({
                "callerAccountId": "111122223333",
                "body": {"domain": domain, "eventName": "OrderNotification", "data": data}
            }),
            attributes,
        )
    }

    fn any_domain_rule(status: &'static str) -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("status", s(status))
                .build()
        })
    }

    fn put_events_success_output(
        entry_count: usize,
    ) -> aws_sdk_eventbridge::operation::put_events::PutEventsOutput {
        aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
            .failed_entry_count(0)
            .set_entries(Some(
                (0..entry_count)
                    .map(|i| {
                        aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
                            .event_id(format!("evt-{i}"))
                            .build()
                    })
                    .collect(),
            ))
            .build()
    }

    #[tokio::test]
    async fn ten_records_for_one_bus_share_a_single_put_events_request() {
        set_env();
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                if req.entries().len() != 10 {
                    return false;
                }
                req.entries().iter().enumerate().all(|(i, entry)| {
                    if entry.event_bus_name() != Some("acme-prod-orders-event-bus")
                        || entry.source() != Some("acme-orders")
                        || entry.detail_type() != Some("OrderNotification")
                    {
                        return false;
                    }
                    let detail: Value = serde_json::from_str(entry.detail().unwrap()).unwrap();
                    detail["metadata"]["schemaVersion"] == json!(4)
                        && detail["metadata"]["idempotencyKey"] == json!(format!("key-{i}"))
                        && detail["metadata"]["publishedAt"] == json!("2026-07-30T10:30:00.123Z")
                        && detail["data"] == json!({"orderId": i})
                })
            })
            .then_output(|| put_events_success_output(10));
        let clients = clients(vec![any_domain_rule("ACTIVE")], vec![put.clone()]);

        let records = (0..10)
            .map(|i| {
                legacy_record(
                    &format!("msg-{i}"),
                    &format!("key-{i}"),
                    "orders",
                    json!({"orderId": i}),
                )
            })
            .collect();
        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(records),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
        assert_eq!(
            put.num_calls(),
            1,
            "one bus of fitting entries shares one PutEvents request"
        );
    }

    #[tokio::test]
    async fn legacy_and_format_two_records_share_one_request() {
        set_env();
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                if req.entries().len() != 2 {
                    return false;
                }
                let legacy: Value =
                    serde_json::from_str(req.entries()[0].detail().unwrap()).unwrap();
                let versioned: Value =
                    serde_json::from_str(req.entries()[1].detail().unwrap()).unwrap();
                legacy["metadata"].get("metadataRevision").is_none()
                    && legacy["metadata"]["schemaVersion"] == json!(4)
                    && versioned["metadata"]["metadataRevision"] == json!(2)
                    && versioned["metadata"]["traceId"] == json!("trace-abc")
                    && versioned["metadata"]["schemaVersion"] == json!(4)
            })
            .then_output(|| put_events_success_output(2));
        let clients = clients(vec![any_domain_rule("ACTIVE")], vec![put]);

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(vec![
                legacy_record("msg-legacy", "key-1", "orders", json!({"orderId": 123})),
                versioned_record_named("msg-v2", versioned_body(), admitted_attributes()),
            ]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
    }

    #[tokio::test]
    async fn different_buses_are_sent_as_independent_requests() {
        set_env();
        let orders_put = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                req.entries().len() == 1
                    && req.entries()[0].event_bus_name() == Some("acme-prod-orders-event-bus")
                    && req.entries()[0].source() == Some("acme-orders")
            })
            .then_output(|| put_events_success_output(1));
        let payments_put = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                req.entries().len() == 1
                    && req.entries()[0].event_bus_name() == Some("acme-prod-payments-event-bus")
                    && req.entries()[0].source() == Some("acme-payments")
            })
            .then_output(|| put_events_success_output(1));
        let clients = clients(
            vec![any_domain_rule("ACTIVE")],
            vec![orders_put.clone(), payments_put.clone()],
        );

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(vec![
                legacy_record("msg-orders", "key-o", "orders", json!({})),
                legacy_record("msg-payments", "key-p", "payments", json!({})),
            ]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
        assert_eq!(orders_put.num_calls(), 1);
        assert_eq!(payments_put.num_calls(), 1);
    }

    #[tokio::test]
    async fn an_oversized_same_bus_group_is_split_by_size() {
        set_env();
        let two = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| req.entries().len() == 2)
            .then_output(|| put_events_success_output(2));
        let one = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                req.entries().len() == 1
                    && req.entries()[0].event_bus_name() == Some("acme-prod-orders-event-bus")
            })
            .then_output(|| put_events_success_output(1));
        let clients = clients(
            vec![any_domain_rule("ACTIVE")],
            vec![two.clone(), one.clone()],
        );

        let records = (0..3)
            .map(|i| {
                legacy_record(
                    &format!("msg-{i}"),
                    &format!("key-{i}"),
                    "orders",
                    json!({"blob": "x".repeat(100_000)}),
                )
            })
            .collect();
        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(records),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
        assert_eq!(two.num_calls(), 1, "two ~100 KB entries fit one request");
        assert_eq!(
            one.num_calls(),
            1,
            "the third entry overflows into its own request"
        );
    }

    #[tokio::test]
    async fn a_partial_put_events_failure_reports_exactly_the_failed_entries_messages() {
        set_env();
        let failed_entry = || {
            aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
                .error_code("InternalFailure")
                .error_message("boom")
                .build()
        };
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| req.entries().len() == 4)
            .then_output(move || {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(2)
                    .set_entries(Some(vec![
                        ok_entry("evt-0"),
                        failed_entry(),
                        ok_entry("evt-2"),
                        failed_entry(),
                    ]))
                    .build()
            });
        let clients = clients(vec![any_domain_rule("ACTIVE")], vec![put]);

        let records = (0..4)
            .map(|i| {
                legacy_record(
                    &format!("msg-{i}"),
                    &format!("key-{i}"),
                    "orders",
                    json!({}),
                )
            })
            .collect();
        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(records),
        )
        .await
        .unwrap();

        assert_eq!(
            failures(&response),
            ["msg-1", "msg-3"],
            "failures map back by entry index"
        );
    }

    #[tokio::test]
    async fn malformed_and_inactive_records_are_reported_while_the_rest_deliver() {
        set_env();
        let inactive_payments = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.key().and_then(|k| k.get("domainName")) == Some(&s("payments"))
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .item("status", s("INACTIVE"))
                    .build()
            });
        let clients = clients(
            vec![active_domain_rule(), inactive_payments],
            vec![put_events_rule()],
        );
        let malformed: Vec<(&'static str, String)> = admitted_attributes()
            .into_iter()
            .filter(|(k, _)| *k != "SchemaVersion")
            .collect();

        let response = handle(&clients, &validators(), &no_liveness_cache(), &ZeroJitter, sqs_event(vec![
                record_with_id(
                    "msg-ok",
                    json!({
                        "callerAccountId": "111122223333",
                        "body": {"domain": "orders", "eventName": "OrderNotification", "data": {"orderId": 123, "amount": 99.5}}
                    }),
                    admitted_attributes(),
                ),
                record_with_id(
                    "msg-malformed",
                    json!({
                        "callerAccountId": "111122223333",
                        "body": {"domain": "orders", "eventName": "OrderNotification", "data": {}}
                    }),
                    malformed,
                ),
                legacy_record("msg-inactive", "key-inactive", "payments", json!({})),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-malformed", "msg-inactive"]);
    }

    #[tokio::test]
    async fn a_whole_request_sdk_error_reports_every_message_of_that_request() {
        set_env();
        let eb_error = mock!(aws_sdk_eventbridge::Client::put_events).then_error(|| {
            aws_sdk_eventbridge::operation::put_events::PutEventsError::generic(
                aws_sdk_eventbridge::error::ErrorMetadata::builder()
                    .code("InternalFailure")
                    .message("boom")
                    .build(),
            )
        });
        let clients = clients(vec![any_domain_rule("ACTIVE")], vec![eb_error]);

        let records = (0..3)
            .map(|i| {
                legacy_record(
                    &format!("msg-{i}"),
                    &format!("key-{i}"),
                    "orders",
                    json!({}),
                )
            })
            .collect();
        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(records),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-0", "msg-1", "msg-2"]);
    }

    #[tokio::test]
    async fn two_records_for_one_domain_share_a_single_liveness_read() {
        set_env();
        let ddb = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("status", s("ACTIVE"))
                .build()
        });
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| req.entries().len() == 2)
            .then_output(|| put_events_success_output(2));
        let clients = clients(vec![ddb.clone()], vec![put]);
        let liveness = DomainLivenessCache::new(Duration::from_secs(300));

        let response = handle(
            &clients,
            &validators(),
            &liveness,
            &ZeroJitter,
            sqs_event(vec![
                legacy_record("msg-a", "key-a", "orders", json!({})),
                legacy_record("msg-b", "key-b", "orders", json!({})),
            ]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
        assert_eq!(ddb.num_calls(), 1, "one liveness read per domain per batch");
    }

    #[tokio::test]
    async fn a_warm_liveness_cache_serves_the_next_invocation_without_a_read() {
        set_env();
        let ddb = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("status", s("ACTIVE"))
                .build()
        });
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| put_events_success_output(1));
        let clients = clients(vec![ddb.clone()], vec![put]);
        let liveness = DomainLivenessCache::new(Duration::from_secs(300));

        for id in ["msg-a", "msg-b"] {
            let response = handle(
                &clients,
                &validators(),
                &liveness,
                &ZeroJitter,
                sqs_event(vec![legacy_record(id, "key-a", "orders", json!({}))]),
            )
            .await
            .unwrap();
            assert!(failures(&response).is_empty());
        }

        assert_eq!(
            ddb.num_calls(),
            1,
            "the second invocation reuses the confirmation"
        );
    }

    #[tokio::test]
    async fn an_inactive_result_is_not_reused() {
        set_env();
        let inactive = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("status", s("INACTIVE"))
                .build()
        });
        let active = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("status", s("ACTIVE"))
                .build()
        });
        let ddb = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[inactive, active]);
        let put = mock_client!(
            aws_sdk_eventbridge,
            RuleMode::MatchAny,
            &[mock!(aws_sdk_eventbridge::Client::put_events)
                .then_output(|| put_events_success_output(1))]
        );
        let clients = Clients {
            ddb,
            eb: put,
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, &[]),
        };
        let liveness = DomainLivenessCache::new(Duration::from_secs(300));

        let first = handle(
            &clients,
            &validators(),
            &liveness,
            &ZeroJitter,
            sqs_event(vec![legacy_record("msg-a", "key-a", "orders", json!({}))]),
        )
        .await
        .unwrap();
        assert_eq!(failures(&first), ["msg-a"]);

        let second = handle(
            &clients,
            &validators(),
            &liveness,
            &ZeroJitter,
            sqs_event(vec![legacy_record("msg-b", "key-b", "orders", json!({}))]),
        )
        .await
        .unwrap();
        assert!(
            failures(&second).is_empty(),
            "a re-enabled domain reads again and delivers"
        );
    }

    #[tokio::test]
    async fn a_zero_window_reads_the_domain_for_every_batch() {
        set_env();
        let ddb = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("status", s("ACTIVE"))
                .build()
        });
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| put_events_success_output(1));
        let clients = clients(vec![ddb.clone()], vec![put]);
        let liveness = DomainLivenessCache::new(Duration::ZERO);

        for id in ["msg-a", "msg-b"] {
            let response = handle(
                &clients,
                &validators(),
                &liveness,
                &ZeroJitter,
                sqs_event(vec![legacy_record(id, "key-a", "orders", json!({}))]),
            )
            .await
            .unwrap();
            assert!(failures(&response).is_empty());
        }

        assert_eq!(ddb.num_calls(), 2, "a zero window reads on every batch");
    }

    #[tokio::test]
    async fn the_liveness_read_is_strongly_consistent() {
        set_env();
        let consistent_reads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = consistent_reads.clone();
        let ddb = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(move |req| {
                recorded.lock().unwrap().push(req.consistent_read());
                req.table_name() == Some("domains-test")
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .item("status", s("ACTIVE"))
                    .build()
            });
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| put_events_success_output(1));
        let clients = clients(vec![ddb], vec![put]);

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event(vec![legacy_record("msg-c", "key-c", "orders", json!({}))]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
        assert_eq!(
            *consistent_reads.lock().unwrap(),
            vec![Some(true)],
            "a liveness read seeds the cache, so it must not trail a disable"
        );
    }
    struct ZeroJitter;

    impl JitterSource for ZeroJitter {
        fn up_to(&self, _max: Duration) -> Duration {
            Duration::ZERO
        }
    }

    fn throttle_record(message_id: &str, receive_count: u32) -> Value {
        let mut record = legacy_record(
            message_id,
            &format!("key-{message_id}"),
            "orders",
            json!({}),
        );
        record["receiptHandle"] = json!(format!("receipt-{message_id}"));
        record["attributes"]["ApproximateReceiveCount"] = json!(receive_count.to_string());
        record
    }

    fn sqs_event_with_deadline(records: Vec<Value>, deadline_ms: u64) -> LambdaEvent<Value> {
        let mut context = lambda_runtime::Context::default();
        context.deadline = deadline_ms;
        LambdaEvent::new(json!({"Records": records}), context)
    }

    fn deadline_in_30s() -> u64 {
        (chrono::Utc::now().timestamp_millis() + 30_000) as u64
    }

    fn deadline_in_3s() -> u64 {
        (chrono::Utc::now().timestamp_millis() + 3_000) as u64
    }

    fn past_deadline() -> u64 {
        (chrono::Utc::now().timestamp_millis() - 1_000) as u64
    }

    fn throttled_entry_output(
        entry_count: usize,
    ) -> aws_sdk_eventbridge::operation::put_events::PutEventsOutput {
        aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
            .failed_entry_count(entry_count as i32)
            .set_entries(Some(
                (0..entry_count)
                    .map(|_| {
                        aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
                            .error_code("ThrottlingException")
                            .error_message("rate exceeded")
                            .build()
                    })
                    .collect(),
            ))
            .build()
    }

    fn throttling_error() -> aws_sdk_eventbridge::operation::put_events::PutEventsError {
        aws_sdk_eventbridge::operation::put_events::PutEventsError::generic(
            aws_sdk_eventbridge::error::ErrorMetadata::builder()
                .code("ThrottlingException")
                .message("rate exceeded")
                .build(),
        )
    }

    fn clients_with_sqs(
        ddb_rules: Vec<Rule>,
        eb_rules: Vec<Rule>,
        sqs_rules: Vec<Rule>,
    ) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, &ddb_rules),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, &eb_rules),
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, &sqs_rules),
        }
    }

    fn visibility_output(
    ) -> aws_sdk_sqs::operation::change_message_visibility_batch::ChangeMessageVisibilityBatchOutput
    {
        aws_sdk_sqs::operation::change_message_visibility_batch::ChangeMessageVisibilityBatchOutput::builder()
            .set_successful(Some(vec![]))
            .set_failed(Some(vec![]))
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn throttled_entries_accepted_on_retry_are_acknowledged_without_a_visibility_change() {
        set_env();
        let throttled = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| throttled_entry_output(1));
        let accepted = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| put_events_success_output(1));
        let clients = Clients {
            ddb: mock_client!(
                aws_sdk_dynamodb,
                RuleMode::MatchAny,
                &[any_domain_rule("ACTIVE")]
            ),
            eb: mock_client!(
                aws_sdk_eventbridge,
                RuleMode::Sequential,
                &[throttled.clone(), accepted.clone()]
            ),
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, &[]),
        };

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event_with_deadline(vec![throttle_record("msg-t", 1)], deadline_in_30s()),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
        assert_eq!(throttled.num_calls(), 1);
        assert_eq!(
            accepted.num_calls(),
            1,
            "the in-invocation retry delivered the entry"
        );
    }

    #[tokio::test]
    async fn still_throttled_messages_get_a_visibility_backoff_and_are_reported() {
        set_env();
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| throttled_entry_output(2));
        let visibility = mock!(aws_sdk_sqs::Client::change_message_visibility_batch)
            .match_requests(|req| {
                if req.queue_url() != Some("https://sqs.test/publish-queue") {
                    return false;
                }
                let entries = req.entries();
                if entries.len() != 2 {
                    return false;
                }
                let timeout_of = |id: &str| {
                    entries
                        .iter()
                        .find(|e| e.id() == id)
                        .and_then(|e| e.visibility_timeout())
                };
                timeout_of("msg-a") == Some(1) && timeout_of("msg-b") == Some(2)
            })
            .then_output(|| visibility_output());
        let clients = clients_with_sqs(
            vec![any_domain_rule("ACTIVE")],
            vec![put.clone()],
            vec![visibility.clone()],
        );

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event_with_deadline(
                vec![throttle_record("msg-a", 1), throttle_record("msg-b", 3)],
                deadline_in_30s(),
            ),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-a", "msg-b"]);
        assert_eq!(
            put.num_calls(),
            4,
            "the first send plus three in-invocation retries"
        );
        assert_eq!(visibility.num_calls(), 1, "one batched visibility change");
    }

    #[tokio::test]
    async fn a_request_level_throttle_takes_the_same_recovery_path() {
        set_env();
        let put = mock!(aws_sdk_eventbridge::Client::put_events).then_error(|| throttling_error());
        let visibility = mock!(aws_sdk_sqs::Client::change_message_visibility_batch)
            .match_requests(|req| req.entries().len() == 1 && req.entries()[0].id() == "msg-t")
            .then_output(|| visibility_output());
        let clients = clients_with_sqs(
            vec![any_domain_rule("ACTIVE")],
            vec![put.clone()],
            vec![visibility.clone()],
        );

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event_with_deadline(vec![throttle_record("msg-t", 2)], deadline_in_30s()),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-t"]);
        assert_eq!(
            put.num_calls(),
            12,
            "four sends (first plus three retries), each with the SDK's standard three-attempt retry on request-level throttling"
        );
        assert_eq!(visibility.num_calls(), 1);
    }

    #[tokio::test]
    async fn a_non_throttle_entry_error_is_reported_without_a_visibility_change() {
        set_env();
        let put = mock!(aws_sdk_eventbridge::Client::put_events).then_output(|| {
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
        let clients = clients_with_sqs(vec![any_domain_rule("ACTIVE")], vec![put.clone()], vec![]);

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event_with_deadline(vec![throttle_record("msg-t", 1)], deadline_in_30s()),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-t"]);
        assert_eq!(
            put.num_calls(),
            1,
            "non-throttle failures are not retried in the invocation"
        );
    }

    #[tokio::test]
    async fn a_failed_visibility_change_still_reports_the_message() {
        set_env();
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| throttled_entry_output(1));
        let visibility = mock!(aws_sdk_sqs::Client::change_message_visibility_batch).then_error(|| {
            aws_sdk_sqs::operation::change_message_visibility_batch::ChangeMessageVisibilityBatchError::generic(
                aws_sdk_sqs::error::ErrorMetadata::builder().message("sqs down").build(),
            )
        });
        let clients =
            clients_with_sqs(vec![any_domain_rule("ACTIVE")], vec![put], vec![visibility]);

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event_with_deadline(vec![throttle_record("msg-t", 1)], deadline_in_30s()),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-t"]);
    }

    #[test]
    fn latency_fields_measure_from_the_sent_timestamp() {
        let (queue_wait_ms, enqueue_to_delivery_ms) = delivery_latency(1_000, 1_200, 1_450);
        assert_eq!(queue_wait_ms, 200, "invocation start minus SentTimestamp");
        assert_eq!(
            enqueue_to_delivery_ms, 450,
            "EventBridge acceptance minus SentTimestamp"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn each_request_records_its_own_acceptance_time() {
        set_env();
        let slow_put = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| req.entries()[0].event_bus_name() == Some("acme-prod-slow-bus"))
            .then_output(|| put_events_success_output(1));
        let fast_put = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| req.entries()[0].event_bus_name() == Some("acme-prod-swift-bus"))
            .then_output(|| put_events_success_output(1));
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, &[]),
            eb: mock_client!(
                aws_sdk_eventbridge,
                RuleMode::MatchAny,
                &[slow_put, fast_put],
                |conf| conf.http_client(DelayedHttpClient::delaying(
                    "acme-prod-slow-bus",
                    Duration::from_millis(300)
                ))
            ),
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, &[]),
        };

        let outcomes = send::deliver(
            &clients,
            &no_liveness_cache(),
            &ZeroJitter,
            vec![
                prepared_entry_for(
                    "msg-slow",
                    "acme-prod-slow-bus",
                    1_785_407_400_100,
                    "orders",
                ),
                prepared_entry_for(
                    "msg-fast",
                    "acme-prod-swift-bus",
                    1_785_407_400_200,
                    "orders",
                ),
            ],
            &|| Duration::from_secs(30),
        )
        .await;

        let mut accepted: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
        for outcome in outcomes {
            if let send::DeliveryOutcome::Delivered {
                sent_timestamp_ms,
                accepted_ms,
                ..
            } = outcome
            {
                accepted.insert(sent_timestamp_ms, accepted_ms);
            }
        }
        let fast = accepted[&1_785_407_400_200];
        let slow = accepted[&1_785_407_400_100];
        assert!(
            fast + 150 <= slow,
            "the fast request's acceptance ({fast}) must not inherit the slow request's completion time ({slow})"
        );
    }

    /// Holds back responses to requests whose body contains `needle` with an
    /// async sleep in the HTTP layer. Mock rule closures run under the
    /// interceptor's rules mutex, so a delay there would also stall every
    /// concurrent request.
    #[derive(Debug, Clone)]
    struct DelayedHttpClient {
        inner: aws_sdk_eventbridge::config::SharedHttpClient,
        needle: &'static str,
        delay: Duration,
    }

    impl DelayedHttpClient {
        fn delaying(needle: &'static str, delay: Duration) -> Self {
            Self {
                inner: aws_smithy_mocks::create_mock_http_client(),
                needle,
                delay,
            }
        }
    }

    impl aws_smithy_runtime_api::client::http::HttpClient for DelayedHttpClient {
        fn http_connector(
            &self,
            settings: &aws_smithy_runtime_api::client::http::HttpConnectorSettings,
            components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        ) -> aws_smithy_runtime_api::client::http::SharedHttpConnector {
            aws_smithy_runtime_api::client::http::SharedHttpConnector::new(DelayedConnector {
                inner: self.inner.http_connector(settings, components),
                needle: self.needle,
                delay: self.delay,
            })
        }
    }

    #[derive(Debug)]
    struct DelayedConnector {
        inner: aws_smithy_runtime_api::client::http::SharedHttpConnector,
        needle: &'static str,
        delay: Duration,
    }

    impl aws_smithy_runtime_api::client::http::HttpConnector for DelayedConnector {
        fn call(
            &self,
            request: aws_smithy_runtime_api::client::orchestrator::HttpRequest,
        ) -> aws_smithy_runtime_api::client::http::HttpConnectorFuture {
            let delay = request
                .body()
                .bytes()
                .is_some_and(|body| String::from_utf8_lossy(body).contains(self.needle))
                .then_some(self.delay);
            let inner = self.inner.clone();
            aws_smithy_runtime_api::client::http::HttpConnectorFuture::new(async move {
                if let Some(delay) = delay {
                    tokio::time::sleep(delay).await;
                }
                inner.call(request).await
            })
        }
    }

    fn prepared_entry_for(
        message_id: &str,
        event_bus_name: &str,
        sent_ms: i64,
        domain: &str,
    ) -> PreparedEntry {
        PreparedEntry {
            message_id: message_id.to_string(),
            receipt_handle: None,
            receive_count: 1,
            sent_timestamp_ms: sent_ms,
            domain: domain.to_string(),
            request_entry: PutEventsRequestEntry::builder()
                .event_bus_name(event_bus_name)
                .source("acme-orders")
                .detail_type("OrderNotification")
                .detail("{}")
                .build(),
            size_bytes: 100,
        }
    }

    #[tokio::test]
    async fn no_retry_starts_when_the_invocation_is_out_of_time() {
        set_env();
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| throttled_entry_output(1));
        let visibility = mock!(aws_sdk_sqs::Client::change_message_visibility_batch)
            .then_output(|| visibility_output());
        let clients = clients_with_sqs(
            vec![any_domain_rule("ACTIVE")],
            vec![put.clone()],
            vec![visibility.clone()],
        );

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event_with_deadline(vec![throttle_record("msg-t", 2)], deadline_in_3s()),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-t"]);
        assert_eq!(
            put.num_calls(),
            1,
            "no in-invocation retry under the time margin"
        );
        assert_eq!(
            visibility.num_calls(),
            1,
            "the message still gets its backoff while time remains"
        );
    }

    #[tokio::test]
    async fn an_out_of_time_invocation_skips_the_visibility_change_and_still_reports() {
        set_env();
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| throttled_entry_output(1));
        let visibility = mock!(aws_sdk_sqs::Client::change_message_visibility_batch)
            .then_output(|| visibility_output());
        let clients = clients_with_sqs(
            vec![any_domain_rule("ACTIVE")],
            vec![put.clone()],
            vec![visibility.clone()],
        );

        let response = handle(
            &clients,
            &validators(),
            &no_liveness_cache(),
            &ZeroJitter,
            sqs_event_with_deadline(vec![throttle_record("msg-t", 2)], past_deadline()),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-t"]);
        assert_eq!(
            visibility.num_calls(),
            0,
            "with no invocation time left there is no budget for a visibility change; the standard timeout applies"
        );
    }

    #[derive(Clone)]
    struct ManualClock(std::sync::Arc<std::sync::Mutex<std::time::Instant>>);

    impl ManualClock {
        fn new() -> Self {
            Self(std::sync::Arc::new(std::sync::Mutex::new(
                std::time::Instant::now(),
            )))
        }

        fn advance(&self, delta: Duration) {
            *self.0.lock().unwrap() += delta;
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> std::time::Instant {
            *self.0.lock().unwrap()
        }
    }

    #[tokio::test]
    async fn a_resend_rechecks_an_expired_liveness_confirmation() {
        set_env();
        let clock = ManualClock::new();
        let liveness = DomainLivenessCache::with_clock(Duration::from_secs(1), clock.clone());
        liveness
            .confirm_active("orders", || async { Ok::<_, anyhow::Error>(true) })
            .await
            .unwrap();
        let throttled = mock!(aws_sdk_eventbridge::Client::put_events).then_output(move || {
            clock.advance(Duration::from_secs(2));
            throttled_entry_output(1)
        });
        let rechecked = any_domain_rule("INACTIVE");
        let clients = Clients {
            ddb: mock_client!(
                aws_sdk_dynamodb,
                RuleMode::MatchAny,
                std::slice::from_ref(&rechecked)
            ),
            eb: mock_client!(
                aws_sdk_eventbridge,
                RuleMode::MatchAny,
                std::slice::from_ref(&throttled)
            ),
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, &[]),
        };

        let outcomes = send::deliver(
            &clients,
            &liveness,
            &ZeroJitter,
            vec![prepared_entry_for(
                "msg-t",
                "acme-prod-orders-event-bus",
                1_785_407_400_100,
                "orders",
            )],
            &|| Duration::from_secs(30),
        )
        .await;

        assert_eq!(
            outcomes,
            vec![send::DeliveryOutcome::Failed {
                message_id: "msg-t".to_string(),
                error: "domain 'orders' is not ACTIVE or does not exist".to_string(),
            }],
            "the resend must not run once the confirmation expired and the domain is disabled"
        );
        assert_eq!(
            throttled.num_calls(),
            1,
            "only the first send happened; the resend was blocked at the recheck"
        );
        assert_eq!(
            rechecked.num_calls(),
            1,
            "the expired confirmation was re-read, not served from the cache"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_send_round_past_its_time_budget_reports_the_round_instead_of_timing_out() {
        set_env();
        let mixed = mock!(aws_sdk_eventbridge::Client::put_events).then_output(|| {
            aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                .failed_entry_count(1)
                .entries(ok_entry("evt-1"))
                .entries(
                    aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
                        .error_code("ThrottlingException")
                        .error_message("rate exceeded")
                        .build(),
                )
                .build()
        });
        let slow = mock!(aws_sdk_eventbridge::Client::put_events).then_output(|| {
            std::thread::sleep(Duration::from_secs(5));
            put_events_success_output(1)
        });
        let clients = Clients {
            ddb: mock_client!(
                aws_sdk_dynamodb,
                RuleMode::MatchAny,
                &[any_domain_rule("ACTIVE")]
            ),
            eb: mock_client!(
                aws_sdk_eventbridge,
                RuleMode::Sequential,
                &[mixed.clone(), slow.clone()]
            ),
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, &[]),
        };

        let outcomes = send::deliver(
            &clients,
            &no_liveness_cache(),
            &ZeroJitter,
            vec![
                prepared_entry_for(
                    "msg-ok",
                    "acme-prod-orders-event-bus",
                    1_785_407_400_100,
                    "orders",
                ),
                prepared_entry_for(
                    "msg-t",
                    "acme-prod-orders-event-bus",
                    1_785_407_400_200,
                    "orders",
                ),
            ],
            &|| Duration::from_millis(5_500),
        )
        .await;

        let mut delivered = 0;
        for outcome in &outcomes {
            match outcome {
                send::DeliveryOutcome::Delivered {
                    sent_timestamp_ms, ..
                } => {
                    assert_eq!(*sent_timestamp_ms, 1_785_407_400_100);
                    delivered += 1;
                }
                send::DeliveryOutcome::Failed { message_id, error } => {
                    assert_eq!(message_id, "msg-t");
                    assert_eq!(error, "the invocation ran out of time during the send");
                }
                send::DeliveryOutcome::Throttled { .. } => {
                    panic!("a timed-out round is reported for retry, not rescheduled")
                }
            }
        }
        assert_eq!(
            delivered, 1,
            "the entry accepted before the budget ran out is still reported as delivered"
        );
        assert_eq!(mixed.num_calls(), 1);
        assert_eq!(
            slow.num_calls(),
            1,
            "the retry round started, and its answer was abandoned at the budget"
        );
    }

    #[tokio::test]
    async fn an_expired_confirmation_is_rechecked_before_the_first_send() {
        set_env();
        let clock = ManualClock::new();
        let liveness = DomainLivenessCache::with_clock(Duration::from_secs(1), clock.clone());
        liveness
            .confirm_active("orders", || async { Ok::<_, anyhow::Error>(true) })
            .await
            .unwrap();
        clock.advance(Duration::from_secs(2));
        let put = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| put_events_success_output(1));
        let rechecked = any_domain_rule("INACTIVE");
        let clients = clients_with_sqs(vec![rechecked.clone()], vec![put.clone()], vec![]);

        let outcomes = send::deliver(
            &clients,
            &liveness,
            &ZeroJitter,
            vec![prepared_entry_for(
                "msg-1",
                "acme-prod-orders-event-bus",
                1_785_407_400_100,
                "orders",
            )],
            &|| Duration::from_secs(30),
        )
        .await;

        assert_eq!(
            outcomes,
            vec![send::DeliveryOutcome::Failed {
                message_id: "msg-1".to_string(),
                error: "domain 'orders' is not ACTIVE or does not exist".to_string(),
            }],
            "a confirmation that expired while the batch was prepared is re-read before sending"
        );
        assert_eq!(put.num_calls(), 0);
        assert_eq!(rechecked.num_calls(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_slow_liveness_recheck_reports_the_round_instead_of_timing_out() {
        set_env();
        let clock = ManualClock::new();
        let liveness = DomainLivenessCache::with_clock(Duration::from_secs(1), clock.clone());
        liveness
            .confirm_active("orders", || async { Ok::<_, anyhow::Error>(true) })
            .await
            .unwrap();
        let mixed_then_expire =
            mock!(aws_sdk_eventbridge::Client::put_events).then_output(move || {
                clock.advance(Duration::from_secs(2));
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(1)
                    .entries(ok_entry("evt-1"))
                    .entries(
                        aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
                            .error_code("ThrottlingException")
                            .error_message("rate exceeded")
                            .build(),
                    )
                    .build()
            });
        let clients = Clients {
            ddb: mock_client!(
                aws_sdk_dynamodb,
                RuleMode::MatchAny,
                &[any_domain_rule("ACTIVE")],
                |conf| conf.http_client(DelayedHttpClient::delaying(
                    "domains-test",
                    Duration::from_secs(10)
                ))
            ),
            eb: mock_client!(
                aws_sdk_eventbridge,
                RuleMode::MatchAny,
                std::slice::from_ref(&mixed_then_expire)
            ),
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, &[]),
        };
        let deadline = std::time::Instant::now() + Duration::from_millis(5_500);
        let started = std::time::Instant::now();

        let outcomes = send::deliver(
            &clients,
            &liveness,
            &ZeroJitter,
            vec![
                prepared_entry_for(
                    "msg-ok",
                    "acme-prod-orders-event-bus",
                    1_785_407_400_100,
                    "orders",
                ),
                prepared_entry_for(
                    "msg-t",
                    "acme-prod-orders-event-bus",
                    1_785_407_400_200,
                    "orders",
                ),
            ],
            &move || deadline.saturating_duration_since(std::time::Instant::now()),
        )
        .await;

        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the recheck is bounded by the round's budget, not the slow read"
        );
        assert_eq!(
            outcomes,
            vec![
                send::DeliveryOutcome::Delivered {
                    sent_timestamp_ms: 1_785_407_400_100,
                    accepted_ms: match outcomes.first() {
                        Some(send::DeliveryOutcome::Delivered { accepted_ms, .. }) => *accepted_ms,
                        _ => 0,
                    },
                },
                send::DeliveryOutcome::Failed {
                    message_id: "msg-t".to_string(),
                    error: "the invocation ran out of time during the send".to_string(),
                },
            ],
            "the accepted entry keeps its outcome and the unchecked resend is reported for retry"
        );
    }
}
