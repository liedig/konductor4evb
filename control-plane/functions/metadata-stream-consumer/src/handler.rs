//! Translates committed metadata mutation records on the
//! MetadataConfigurationTable stream into `MetadataRevisionCreated` and
//! `MetadataRevisionActivated` lifecycle events on the control-plane bus.
//!
//! Only committed records emit: the stream's new image is the transaction's
//! committed state, and nothing here ever writes configuration. Duplicate
//! delivery preserves the deterministic mutation identity — each stream
//! event id is claimed with a conditional marker item (TTL'd) in the
//! metadata table before emission, so Lambda redelivery (including
//! bisect-batch retries) never duplicates an event. If emission fails the
//! marker is released (best effort) so the retry can re-claim; bounded
//! retries then land records in the failure destination, where the
//! permanent audit records support recovery beyond the stream window.
//!
//! Non-mutation items (the global head, audit records, marker TTL deletes)
//! and bootstrap-seeded pointers (active revision 0) are ignored.

use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use konductor_common::dynamodb::s;
use konductor_common::error::ControlPlaneError;
use konductor_common::metadata::store::{ENVIRONMENT_PARTITION, REVISION_PARTITION};

pub const EVENT_SOURCE: &str = "konductor.control-plane";
const DETAIL_TYPE_REVISION_CREATED: &str = "MetadataRevisionCreated";
const DETAIL_TYPE_ENVIRONMENT_ACTIVATED: &str = "MetadataRevisionActivated";
const MARKER_ITEM_TYPE: &str = "MetadataStreamMarker";
const MARKER_TTL_SECS: i64 = 7 * 24 * 60 * 60;

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

pub struct ConsumerConfig {
    pub table: String,
    pub bus: String,
}

impl ConsumerConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            table: std::env::var("METADATA_TABLE").context("METADATA_TABLE not set")?,
            bus: std::env::var("KONDUCTOR_EVENT_BUS").context("KONDUKTOR_EVENT_BUS not set")?,
        })
    }
}

pub async fn handle(
    clients: &Clients,
    config: &ConsumerConfig,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let records = event
        .payload
        .get("Records")
        .and_then(|r| r.as_array())
        .context("missing Records array")?;

    // Any failure errors the invocation; the event-source mapping's
    // bisect-batch-on-error then isolates the failing record, and claimed
    // markers make the retried halves idempotent.
    for record in records {
        process_record(clients, config, record).await?;
    }
    Ok(Value::Null)
}

async fn process_record(clients: &Clients, config: &ConsumerConfig, record: &Value) -> Result<()> {
    let event_name = record.get("eventName").and_then(|v| v.as_str());
    if event_name == Some("REMOVE") {
        return Ok(());
    }
    let new_image = record
        .get("dynamodb")
        .and_then(|d| d.get("NewImage"))
        .with_context(|| format!("{event_name:?} record missing dynamodb.NewImage"))?;

    let mutation = match s_field(new_image, "PK").as_deref() {
        Some(REVISION_PARTITION) => {
            Mutation::RevisionCreated(CreatedRevision::from_new_image(new_image)?)
        }
        Some(ENVIRONMENT_PARTITION) => match ActivatedEnvironment::from_new_image(new_image) {
            Some(activated) => Mutation::EnvironmentActivated(activated),
            None => return Ok(()),
        },
        _ => return Ok(()),
    };

    let event_id = record
        .get("eventID")
        .and_then(|v| v.as_str())
        .context("mutation record missing eventID")?;

    if !claim_marker(clients, &config.table, event_id).await? {
        tracing::info!(
            event_id,
            mutation = %mutation.detail_type(),
            "stream record already processed; skipping"
        );
        return Ok(());
    }

    if let Err(e) = emit(clients, &config.bus, &mutation).await {
        release_marker(clients, &config.table, event_id).await;
        return Err(e);
    }
    Ok(())
}

/// A committed mutation recovered from the stream's new image, with its
/// deterministic identity derived from record content — never the stream
/// event id, which differs per delivery.
enum Mutation {
    RevisionCreated(CreatedRevision),
    EnvironmentActivated(ActivatedEnvironment),
}

impl Mutation {
    fn detail_type(&self) -> &'static str {
        match self {
            Mutation::RevisionCreated(_) => DETAIL_TYPE_REVISION_CREATED,
            Mutation::EnvironmentActivated(_) => DETAIL_TYPE_ENVIRONMENT_ACTIVATED,
        }
    }

    fn payload(&self) -> Value {
        match self {
            Mutation::RevisionCreated(revision) => revision.payload(),
            Mutation::EnvironmentActivated(activated) => activated.payload(),
        }
    }
}

struct CreatedRevision {
    revision: u64,
    predecessor: u64,
    definition_digest: String,
    created_by: String,
    created_at: String,
    operation_id: String,
}

impl CreatedRevision {
    fn from_new_image(image: &Value) -> Result<Self> {
        let field = |name: &str| {
            s_field(image, name).with_context(|| format!("revision image missing {name}"))
        };
        Ok(Self {
            revision: n_field(image, "revision").context("revision image missing revision")?,
            predecessor: n_field(image, "predecessor")
                .context("revision image missing predecessor")?,
            definition_digest: field("definitionDigest")?,
            created_by: field("createdBy")?,
            created_at: field("createdAt")?,
            operation_id: field("operationId")?,
        })
    }

    fn payload(&self) -> Value {
        json!({
            "mutationId": format!("revision-created:{}", self.revision),
            "revision": self.revision,
            "predecessor": self.predecessor,
            "definitionDigest": self.definition_digest,
            "createdBy": self.created_by,
            "createdAt": self.created_at,
            "operationId": self.operation_id,
        })
    }
}

struct ActivatedEnvironment {
    environment: String,
    active_revision: u64,
    generation: u64,
    definition_digest: String,
    activated_by: String,
    activated_at: String,
}

impl ActivatedEnvironment {
    /// `None` means the pointer write seeded baseline state (active
    /// revision 0), not an activation.
    fn from_new_image(image: &Value) -> Option<Self> {
        let active_revision = n_field(image, "activeRevision")?;
        if active_revision == 0 {
            return None;
        }
        Some(Self {
            environment: s_field(image, "SK")?,
            active_revision,
            generation: n_field(image, "generation")?,
            definition_digest: s_field(image, "definitionDigest")?,
            activated_by: s_field(image, "activatedBy")?,
            activated_at: s_field(image, "activatedAt")?,
        })
    }

    fn payload(&self) -> Value {
        json!({
            "mutationId": format!(
                "environment-activated:{}:{}",
                self.environment, self.generation
            ),
            "environment": self.environment,
            "activeRevision": self.active_revision,
            "definitionDigest": self.definition_digest,
            "generation": self.generation,
            "activatedBy": self.activated_by,
            "activatedAt": self.activated_at,
        })
    }
}

fn s_field(image: &Value, name: &str) -> Option<String> {
    image.get(name)?.get("S")?.as_str().map(|s| s.to_string())
}

fn n_field(image: &Value, name: &str) -> Option<u64> {
    image.get(name)?.get("N")?.as_str()?.parse().ok()
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

/// Best-effort marker release so a failed emission can be retried.
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
        tracing::warn!(event_id, error = %e, "failed to release metadata stream marker");
    }
}

async fn emit(clients: &Clients, bus: &str, mutation: &Mutation) -> Result<()> {
    let result = clients
        .eb
        .put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(bus)
                .source(EVENT_SOURCE)
                .detail_type(mutation.detail_type())
                .detail(mutation.payload().to_string())
                .build(),
        )
        .send()
        .await
        .map_err(|e| ControlPlaneError::EventBridge(e.into_service_error().to_string()))?;
    if result.failed_entry_count > 0 {
        return Err(ControlPlaneError::EventBridge(format!(
            "{} event entry failed to publish",
            mutation.detail_type()
        ))
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
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use lambda_runtime::Context as LambdaContext;

    fn config() -> ConsumerConfig {
        ConsumerConfig {
            table: "metadata".to_string(),
            bus: "konductor-bus".to_string(),
        }
    }

    fn revision_record(event_id: &str) -> Value {
        json!({
            "eventID": event_id,
            "eventName": "INSERT",
            "dynamodb": {
                "NewImage": {
                    "PK": { "S": "REVISION" },
                    "SK": { "S": "r0000000003" },
                    "revision": { "N": "3" },
                    "predecessor": { "N": "2" },
                    "definitionDigest": { "S": "digest-3" },
                    "createdBy": { "S": "admin-sub" },
                    "createdAt": { "S": "2026-01-01T00:00:00.000Z" },
                    "operationId": { "S": "op-create-3" }
                }
            }
        })
    }

    fn activation_record(event_id: &str, active_revision: &str, generation: &str) -> Value {
        json!({
            "eventID": event_id,
            "eventName": "MODIFY",
            "dynamodb": {
                "NewImage": {
                    "PK": { "S": "ENV" },
                    "SK": { "S": "production" },
                    "activeRevision": { "N": active_revision },
                    "generation": { "N": generation },
                    "definitionDigest": { "S": "digest-3" },
                    "activatedBy": { "S": "admin-sub" },
                    "activatedAt": { "S": "2026-01-01T00:00:00.000Z" }
                }
            }
        })
    }

    fn event_rule_asserting_detail(detail_type: &str, mutation_id: &str) -> Rule {
        let detail_type = detail_type.to_string();
        let mutation_id = mutation_id.to_string();
        mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(move |req| {
                let Some(entry) = req.entries().first() else {
                    return false;
                };
                let detail: Value =
                    serde_json::from_str(entry.detail().unwrap_or("")).unwrap_or_default();
                entry.source() == Some(EVENT_SOURCE)
                    && entry.detail_type() == Some(detail_type.as_str())
                    && detail["mutationId"] == mutation_id
            })
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
    async fn revision_insert_emits_created_event_with_deterministic_identity() {
        let marker = marker_available_rule();
        let put_events =
            event_rule_asserting_detail(DETAIL_TYPE_REVISION_CREATED, "revision-created:3");
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&marker]),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, [&put_events]),
        };
        let batch = json!({ "Records": [revision_record("evt-1")] });

        handle(&clients, &config(), event(batch)).await.unwrap();

        assert_eq!(marker.num_calls(), 1);
        assert_eq!(put_events.num_calls(), 1);
    }

    #[tokio::test]
    async fn activation_update_emits_activated_event_with_deterministic_identity() {
        let marker = marker_available_rule();
        let put_events = event_rule_asserting_detail(
            DETAIL_TYPE_ENVIRONMENT_ACTIVATED,
            "environment-activated:production:2",
        );
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&marker]),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, [&put_events]),
        };
        let batch = json!({ "Records": [activation_record("evt-2", "3", "2")] });

        handle(&clients, &config(), event(batch)).await.unwrap();

        assert_eq!(marker.num_calls(), 1);
        assert_eq!(put_events.num_calls(), 1);
    }

    #[tokio::test]
    async fn bootstrap_seed_pointers_are_ignored() {
        let marker = marker_available_rule();
        let put_events = event_rule_asserting_detail(DETAIL_TYPE_ENVIRONMENT_ACTIVATED, "unused");
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&marker]),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, [&put_events]),
        };
        let batch = json!({ "Records": [activation_record("evt-seed", "0", "0")] });

        handle(&clients, &config(), event(batch)).await.unwrap();

        assert_eq!(marker.num_calls(), 0, "seed writes claim no marker");
        assert_eq!(put_events.num_calls(), 0, "seed writes emit no event");
    }

    #[tokio::test]
    async fn head_audit_and_marker_records_are_ignored() {
        let marker = marker_available_rule();
        let put_events = event_rule_asserting_detail(DETAIL_TYPE_REVISION_CREATED, "unused");
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&marker]),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, [&put_events]),
        };
        let head_update = json!({
            "eventID": "evt-head",
            "eventName": "MODIFY",
            "dynamodb": { "NewImage": { "PK": { "S": "HEAD" }, "SK": { "S": "HEAD" } } }
        });
        let audit_insert = json!({
            "eventID": "evt-audit",
            "eventName": "INSERT",
            "dynamodb": { "NewImage": { "PK": { "S": "AUDIT" }, "SK": { "S": "1700000000000#op" } } }
        });
        let marker_ttl_delete = json!({
            "eventID": "evt-ttl",
            "eventName": "REMOVE",
            "dynamodb": { "OldImage": { "PK": { "S": "MARKER#evt-1" } } }
        });
        let batch = json!({
            "Records": [head_update, audit_insert, marker_ttl_delete]
        });

        handle(&clients, &config(), event(batch)).await.unwrap();

        assert_eq!(marker.num_calls(), 0);
        assert_eq!(put_events.num_calls(), 0);
    }

    #[tokio::test]
    async fn duplicate_stream_delivery_is_skipped_by_the_marker() {
        let conflict = marker_conflict_rule();
        let put_events = event_rule_asserting_detail(DETAIL_TYPE_REVISION_CREATED, "unused");
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&conflict]),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, [&put_events]),
        };
        let batch = json!({ "Records": [revision_record("evt-1")] });

        handle(&clients, &config(), event(batch)).await.unwrap();

        assert_eq!(
            put_events.num_calls(),
            0,
            "claimed marker means no duplicate event"
        );
    }

    #[tokio::test]
    async fn failed_emission_releases_marker_and_errors_for_bisect() {
        let marker = marker_available_rule();
        let release = mock!(aws_sdk_dynamodb::Client::delete_item)
            .then_output(|| DeleteItemOutput::builder().build());
        let failing_put_events = mock!(aws_sdk_eventbridge::Client::put_events).then_error(|| {
            aws_sdk_eventbridge::operation::put_events::PutEventsError::generic(
                aws_sdk_eventbridge::error::ErrorMetadata::builder()
                    .message("bus unavailable")
                    .build(),
            )
        });
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&marker, &release]),
            eb: mock_client!(
                aws_sdk_eventbridge,
                RuleMode::MatchAny,
                [&failing_put_events]
            ),
        };
        let batch = json!({ "Records": [revision_record("evt-9")] });

        let result = handle(&clients, &config(), event(batch)).await;

        assert!(
            result.is_err(),
            "emission failure must error the invocation"
        );
        assert_eq!(
            release.num_calls(),
            1,
            "marker released so the retry can re-claim"
        );
    }
}
