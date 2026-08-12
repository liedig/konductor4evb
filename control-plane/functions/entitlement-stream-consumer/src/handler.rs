//! Translates entitlement `REMOVE` change records on the entitlements-table
//! stream into (a) a revocation message on the worker queue and (b) the
//! `EntitlementRevoked` business event on the control-plane bus.
//!
//! Idempotency: each stream event id is claimed with a conditional marker
//! item in the entitlements table before any effect fires, so Lambda
//! redelivery (including bisect-batch retries) never duplicates the message
//! or the event. The marker is claimed *first* because a lost revocation is
//! worse than a duplicate — a crash between claim and effects is bounded by
//! the hourly reconciliation sweep, and the durable history of the revocation
//! itself remains the audit record committed with the delete. If an effect
//! fails, the marker is released (best effort) so the retry can re-claim.
//!
//! Non-REMOVE records and audit inserts are ignored; marker TTL deletes are
//! `REMOVE` records whose old image is a marker, filtered by item type.

use std::env;

use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use konductor_common::dynamodb::s;
use konductor_common::error::ControlPlaneError;

pub const EVENT_SOURCE: &str = "nexus.control-plane";
const MARKER_ITEM_TYPE: &str = "RevocationMarker";
const MARKER_TTL_SECS: i64 = 7 * 24 * 60 * 60;

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub sqs: aws_sdk_sqs::Client,
    pub eb: aws_sdk_eventbridge::Client,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: aws_sdk_dynamodb::Client::new(config),
            sqs: aws_sdk_sqs::Client::new(config),
            eb: aws_sdk_eventbridge::Client::new(config),
        }
    }
}

pub async fn handle(clients: &Clients, event: LambdaEvent<Value>) -> Result<Value, Error> {
    let table = env::var("ENTITLEMENTS_TABLE").context("ENTITLEMENTS_TABLE not set")?;
    let queue_url = env::var("REVOCATION_QUEUE_URL").context("REVOCATION_QUEUE_URL not set")?;
    let bus = env::var("KONDUKTOR_EVENT_BUS").context("KONDUKTOR_EVENT_BUS not set")?;

    let records = event
        .payload
        .get("Records")
        .and_then(|r| r.as_array())
        .context("missing Records array")?;

    // Any failure errors the invocation; the event-source mapping's
    // bisect-batch-on-error then isolates the failing record, and claimed
    // markers make the retried halves idempotent.
    for record in records {
        process_record(clients, &table, &queue_url, &bus, record).await?;
    }
    Ok(Value::Null)
}

async fn process_record(
    clients: &Clients,
    table: &str,
    queue_url: &str,
    bus: &str,
    record: &Value,
) -> Result<()> {
    if record.get("eventName").and_then(|v| v.as_str()) != Some("REMOVE") {
        return Ok(());
    }
    let old_image = record
        .get("dynamodb")
        .and_then(|d| d.get("OldImage"))
        .context("REMOVE record missing dynamodb.OldImage")?;
    if s_field(old_image, "itemType").as_deref() != Some("Entitlement") {
        return Ok(());
    }

    let event_id = record
        .get("eventID")
        .and_then(|v| v.as_str())
        .context("REMOVE record missing eventID")?;
    let revoked = RevokedEntitlement::from_old_image(old_image, event_id, record)?;

    if !claim_marker(clients, table, event_id).await? {
        tracing::info!(
            event_id,
            entitlement_id = revoked.entitlement_id,
            "stream record already processed; skipping"
        );
        return Ok(());
    }

    if let Err(e) = forward(clients, queue_url, bus, &revoked).await {
        release_marker(clients, table, event_id).await;
        return Err(e);
    }
    Ok(())
}

/// The revoked entitlement's full prior scope, recovered from the old image.
struct RevokedEntitlement {
    revocation_id: String,
    entitlement_id: String,
    consumer_domain: String,
    producer_domain: String,
    scope_pattern: String,
    revoked_at: String,
}

impl RevokedEntitlement {
    fn from_old_image(image: &Value, event_id: &str, record: &Value) -> Result<Self> {
        let field = |name: &str| {
            s_field(image, name).with_context(|| format!("entitlement old image missing {name}"))
        };
        let revoked_at = record
            .get("dynamodb")
            .and_then(|d| d.get("ApproximateCreationDateTime"))
            .and_then(|v| v.as_f64())
            .and_then(|epoch| chrono::DateTime::from_timestamp(epoch as i64, 0))
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
        Ok(Self {
            revocation_id: event_id.to_string(),
            entitlement_id: field("entitlementId")?,
            consumer_domain: field("consumerDomain")?,
            producer_domain: field("producerDomain")?,
            scope_pattern: field("scopePattern")?,
            revoked_at,
        })
    }

    fn payload(&self) -> Value {
        json!({
            "revocationId": self.revocation_id,
            "entitlementId": self.entitlement_id,
            "consumerDomain": self.consumer_domain,
            "producerDomain": self.producer_domain,
            "scopePattern": self.scope_pattern,
            "revokedAt": self.revoked_at,
        })
    }
}

fn s_field(image: &Value, name: &str) -> Option<String> {
    image.get(name)?.get("S")?.as_str().map(|s| s.to_string())
}

/// Claims the stream event id with a conditional marker write. `false` means
/// the record was already processed (marker exists) and the caller skips it.
async fn claim_marker(clients: &Clients, table: &str, event_id: &str) -> Result<bool> {
    let key = format!("MARKER#{event_id}");
    let expires_at = (chrono::Utc::now() + chrono::Duration::seconds(MARKER_TTL_SECS)).timestamp();
    let result = clients
        .ddb
        .put_item()
        .table_name(table)
        .item("PK", s(&key))
        .item("SK", s(&key))
        .item("itemType", s(MARKER_ITEM_TYPE))
        .item("expiresAt", AttributeValue::N(expires_at.to_string()))
        .condition_expression("attribute_not_exists(PK)")
        .send()
        .await;
    match result {
        Ok(_) => Ok(true),
        Err(e) => {
            let service = e.into_service_error();
            if service.is_conditional_check_failed_exception() {
                Ok(false)
            } else {
                Err(ControlPlaneError::DynamoDb(service.to_string()).into())
            }
        }
    }
}

/// Best-effort marker release so a failed forward can be retried.
async fn release_marker(clients: &Clients, table: &str, event_id: &str) {
    let key = format!("MARKER#{event_id}");
    if let Err(e) = clients
        .ddb
        .delete_item()
        .table_name(table)
        .key("PK", s(&key))
        .key("SK", s(&key))
        .send()
        .await
    {
        tracing::warn!(event_id, error = %e, "failed to release revocation marker");
    }
}

async fn forward(
    clients: &Clients,
    queue_url: &str,
    bus: &str,
    revoked: &RevokedEntitlement,
) -> Result<()> {
    clients
        .sqs
        .send_message()
        .queue_url(queue_url)
        .message_body(revoked.payload().to_string())
        .send()
        .await
        .map_err(|e| ControlPlaneError::Sqs(e.into_service_error().to_string()))?;

    let result = clients
        .eb
        .put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(bus)
                .source(EVENT_SOURCE)
                .detail_type("EntitlementRevoked")
                .detail(revoked.payload().to_string())
                .build(),
        )
        .send()
        .await
        .map_err(|e| ControlPlaneError::EventBridge(e.into_service_error().to_string()))?;
    if result.failed_entry_count > 0 {
        return Err(ControlPlaneError::EventBridge(
            "EntitlementRevoked event entry failed to publish".to_string(),
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::delete_item::DeleteItemOutput;
    use aws_sdk_dynamodb::operation::put_item::{PutItemError, PutItemOutput};
    use aws_sdk_dynamodb::types::error::ConditionalCheckFailedException;
    use aws_sdk_eventbridge::operation::put_events::PutEventsOutput;
    use aws_sdk_sqs::operation::send_message::SendMessageOutput;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use lambda_runtime::Context as LambdaContext;

    fn set_env() {
        std::env::set_var("ENTITLEMENTS_TABLE", "entitlements");
        std::env::set_var(
            "REVOCATION_QUEUE_URL",
            "https://sqs.us-east-1.amazonaws.com/123/revocations",
        );
        std::env::set_var("KONDUKTOR_EVENT_BUS", "nexus-bus");
    }

    fn remove_record(event_id: &str, scope_pattern: &str) -> Value {
        json!({
            "eventID": event_id,
            "eventName": "REMOVE",
            "dynamodb": {
                "ApproximateCreationDateTime": 1700000000.0,
                "OldImage": {
                    "PK": { "S": "CONSUMER#checkout" },
                    "SK": { "S": "ENTITLEMENT#ent-1" },
                    "itemType": { "S": "Entitlement" },
                    "entitlementId": { "S": "ent-1" },
                    "consumerDomain": { "S": "checkout" },
                    "producerDomain": { "S": "orders" },
                    "scopePattern": { "S": scope_pattern },
                    "createdAt": { "S": "2026-01-01T00:00:00Z" },
                    "createdBy": { "S": "admin" }
                }
            }
        })
    }

    fn sqs_rule() -> Rule {
        mock!(aws_sdk_sqs::Client::send_message)
            .then_output(|| SendMessageOutput::builder().message_id("m-1").build())
    }

    /// Matches only when the revocation message carries the revoked scope
    /// recovered from the old image; a wrong body fails to match and the mock
    /// client panics, failing the test.
    fn sqs_rule_asserting_payload(event_id: &str, scope_pattern: &str) -> Rule {
        let event_id = event_id.to_string();
        let scope_pattern = scope_pattern.to_string();
        mock!(aws_sdk_sqs::Client::send_message)
            .match_requests(move |req| {
                let body: Value =
                    serde_json::from_str(req.message_body().unwrap_or("")).unwrap_or_default();
                body["revocationId"] == event_id
                    && body["entitlementId"] == "ent-1"
                    && body["consumerDomain"] == "checkout"
                    && body["producerDomain"] == "orders"
                    && body["scopePattern"] == scope_pattern
            })
            .then_output(|| SendMessageOutput::builder().message_id("m-1").build())
    }

    fn event_rule() -> Rule {
        mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| PutEventsOutput::builder().build())
    }

    fn marker_available_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::put_item).then_output(|| PutItemOutput::builder().build())
    }

    fn marker_conflict_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::put_item).then_error(|| {
            PutItemError::ConditionalCheckFailedException(
                ConditionalCheckFailedException::builder()
                    .message("marker exists")
                    .build(),
            )
        })
    }

    fn event(payload: Value) -> LambdaEvent<Value> {
        LambdaEvent::new(payload, LambdaContext::default())
    }

    #[tokio::test]
    async fn entitlement_remove_enqueues_message_and_emits_event() {
        set_env();
        let marker = marker_available_rule();
        let send = sqs_rule_asserting_payload("evt-1", "*");
        let put_events = event_rule();
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&marker]),
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, [&send]),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, [&put_events]),
        };
        let batch = json!({ "Records": [remove_record("evt-1", "*")] });

        handle(&clients, event(batch)).await.unwrap();

        assert_eq!(marker.num_calls(), 1);
        assert_eq!(send.num_calls(), 1);
        assert_eq!(put_events.num_calls(), 1);
    }

    #[tokio::test]
    async fn duplicate_stream_delivery_is_skipped_by_the_marker() {
        set_env();
        let conflict = marker_conflict_rule();
        let send = sqs_rule();
        let put_events = event_rule();
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&conflict]),
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, [&send]),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, [&put_events]),
        };
        let batch = json!({ "Records": [remove_record("evt-1", "*")] });

        handle(&clients, event(batch)).await.unwrap();

        assert_eq!(
            send.num_calls(),
            0,
            "claimed marker means no duplicate message"
        );
        assert_eq!(
            put_events.num_calls(),
            0,
            "claimed marker means no duplicate event"
        );
    }

    #[tokio::test]
    async fn audit_inserts_and_marker_ttl_deletes_are_ignored() {
        set_env();
        let marker = marker_available_rule();
        let send = sqs_rule();
        let put_events = event_rule();
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&marker]),
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, [&send]),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, [&put_events]),
        };
        let audit_insert = json!({
            "eventID": "evt-audit",
            "eventName": "INSERT",
            "dynamodb": { "NewImage": { "itemType": { "S": "EntitlementAuditRecord" } } }
        });
        let marker_delete = json!({
            "eventID": "evt-ttl",
            "eventName": "REMOVE",
            "dynamodb": {
                "OldImage": {
                    "PK": { "S": "MARKER#evt-1" },
                    "itemType": { "S": "RevocationMarker" }
                }
            }
        });
        let batch = json!({ "Records": [audit_insert, marker_delete] });

        handle(&clients, event(batch)).await.unwrap();

        assert_eq!(marker.num_calls(), 0);
        assert_eq!(send.num_calls(), 0);
        assert_eq!(put_events.num_calls(), 0);
    }

    #[tokio::test]
    async fn failed_forward_releases_marker_and_errors_for_bisect() {
        set_env();
        let marker = marker_available_rule();
        let release = mock!(aws_sdk_dynamodb::Client::delete_item)
            .then_output(|| DeleteItemOutput::builder().build());
        let failing_send = mock!(aws_sdk_sqs::Client::send_message).then_error(|| {
            aws_sdk_sqs::operation::send_message::SendMessageError::generic(
                aws_sdk_sqs::error::ErrorMetadata::builder()
                    .message("queue unavailable")
                    .build(),
            )
        });
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&marker, &release]),
            sqs: mock_client!(aws_sdk_sqs, RuleMode::MatchAny, [&failing_send]),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, []),
        };
        let batch = json!({ "Records": [remove_record("evt-9", "Order*")] });

        let result = handle(&clients, event(batch)).await;

        assert!(result.is_err(), "forward failure must error the invocation");
        assert_eq!(
            release.num_calls(),
            1,
            "marker released so the retry can re-claim"
        );
    }
}
