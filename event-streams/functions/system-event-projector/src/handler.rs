//! Projects committed upgrade phase snapshots from the event-streams table
//! stream into public system events on the system bus (`konductor.system`).
//!
//! For every INSERT/MODIFY image the projector diffs the old and new
//! `upgradeEvents` maps: a new phase identity (new idempotency key at a
//! higher resource revision) is projected; unchanged snapshots, ordinary
//! subscription mutations, legacy records, and REMOVE records emit nothing.
//! The projection reads only the committed snapshot and the enclosing
//! record's identity fields — never current subscription, schema, or
//! metadata state — so retries reproduce the exact public payload.
//!
//! Failures are reported per stream record as partial batch failures: the
//! event source mapping retries from the record that failed, and after the
//! configured attempts the full invocation payload lands in the on-failure
//! S3 destination.

use std::collections::HashMap;
use std::env;

use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_eventbridge::types::PutEventsRequestEntry;
use aws_sdk_eventbridge::Client as EventBridgeClient;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use konductor_common::streams_db::item_to_stream;
use konductor_common::system_events::store::{attr_to_upgrade_snapshot, UPGRADE_EVENTS};
use konductor_common::system_events::{SnapshotScope, UpgradeEventSnapshot};

/// PutEvents accepts at most ten entries per call.
const PUT_EVENTS_BATCH: usize = 10;

pub struct Clients {
    pub eb: EventBridgeClient,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            eb: EventBridgeClient::new(config),
        }
    }
}

pub async fn handle(clients: &Clients, event: LambdaEvent<Value>) -> Result<Value, Error> {
    let bus = env::var("SYSTEM_EVENT_BUS").context("SYSTEM_EVENT_BUS not set")?;
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let records = event
        .payload
        .get("Records")
        .and_then(|r| r.as_array())
        .context("missing Records array")?;
    let mut batch_item_failures = Vec::new();
    for record in records {
        if let Err(error) = process_record(clients, &bus, &organization, record).await {
            let sequence = sequence_number(record);
            tracing::error!(
                error = %format!("{error:#}"),
                sequence_number = sequence.unwrap_or("<unknown>"),
                "failed to project stream record"
            );
            match sequence {
                Some(sequence) => batch_item_failures.push(json!({ "itemIdentifier": sequence })),
                // Without the sequence number the retry boundary is unknown;
                // fail the whole batch rather than risk skipping a record.
                None => return Err(error.into()),
            }
        }
    }
    Ok(json!({ "batchItemFailures": batch_item_failures }))
}

fn sequence_number(record: &Value) -> Option<&str> {
    record
        .get("dynamodb")
        .and_then(|d| d.get("SequenceNumber"))
        .and_then(Value::as_str)
}

/// Snapshots from the record's OldImage. A damaged old image must not fail
/// the record: re-emission is idempotency-key-safe, while a poison record
/// would block the shard — so it degrades to "everything changed" with a
/// warning instead of a batch failure.
fn old_image_snapshots(dynamodb: &Value) -> Option<HashMap<String, UpgradeEventSnapshot>> {
    match dynamodb.get("OldImage") {
        Some(image) => match image_to_item(image).map(|item| image_snapshots(&item)) {
            Some(Ok(snapshots)) => Some(snapshots),
            Some(Err(error)) => {
                tracing::warn!(
                    error = %format!("{error:#}"),
                    "OldImage snapshots unparseable; projecting all new snapshots as changed"
                );
                None
            }
            None => {
                tracing::warn!(
                    "OldImage is not an object; projecting all new snapshots as changed"
                );
                None
            }
        },
        None => None,
    }
}

async fn process_record(
    clients: &Clients,
    bus: &str,
    organization: &str,
    record: &Value,
) -> Result<()> {
    let event_name = record.get("eventName").and_then(Value::as_str);
    if !matches!(event_name, Some("INSERT") | Some("MODIFY")) {
        return Ok(());
    }
    let dynamodb = record.get("dynamodb").context("record missing dynamodb")?;
    let new_image = dynamodb
        .get("NewImage")
        .context("record missing NewImage")?;
    let new_item = image_to_item(new_image).context("NewImage is not an object")?;
    let Some(stream) = item_to_stream(&new_item) else {
        // Legacy or unrelated record shape: no projection is derivable.
        return Ok(());
    };
    let old_snapshots = old_image_snapshots(dynamodb);
    let new_snapshots = image_snapshots(&new_item)?;
    let changed: Vec<(&String, &UpgradeEventSnapshot)> = new_snapshots
        .iter()
        .filter(|(environment, snapshot)| {
            old_snapshots
                .as_ref()
                .and_then(|old| old.get(environment.as_str()))
                .is_none_or(|old| {
                    old.idempotency_key != snapshot.idempotency_key
                        && old.resource_revision < snapshot.resource_revision
                })
        })
        .collect();
    if changed.is_empty() {
        return Ok(());
    }
    tracing::info!(
        producer = %stream.producer_domain,
        consumer = %stream.consumer_domain,
        schema = %stream.schema_name,
        changed = changed.len(),
        "projecting upgrade phase snapshots"
    );
    for chunk in changed.chunks(PUT_EVENTS_BATCH) {
        let mut entries = Vec::with_capacity(chunk.len());
        for (environment, snapshot) in chunk {
            let scope = SnapshotScope {
                organization,
                consumer_domain: &stream.consumer_domain,
                producer_domain: &stream.producer_domain,
                schema_name: &stream.schema_name,
                detail_type: &stream.detail_type,
                environment,
            };
            let event = snapshot.to_system_event(&scope, bus).map_err(|e| {
                anyhow::anyhow!("committed snapshot fails contract validation: {e}")
            })?;
            entries.push(
                PutEventsRequestEntry::builder()
                    .source(event.source)
                    .detail_type(event.detail_type)
                    .event_bus_name(event.event_bus_name)
                    .detail(
                        serde_json::to_string(&event.detail)
                            .context("system event detail is not serializable")?,
                    )
                    .build(),
            );
        }
        let output = clients
            .eb
            .put_events()
            .set_entries(Some(entries))
            .send()
            .await
            .map_err(|e| anyhow::Error::new(e.into_service_error()))
            .context("PutEvents to the system bus failed")?;
        if output.failed_entry_count() > 0 {
            let causes: Vec<String> = output
                .entries()
                .iter()
                .filter(|e| e.error_code().is_some())
                .map(|e| {
                    format!(
                        "{}: {}",
                        e.error_code().unwrap_or("?"),
                        e.error_message().unwrap_or("?")
                    )
                })
                .collect();
            anyhow::bail!(
                "PutEvents rejected {} entr(y/ies): {causes:?}",
                output.failed_entry_count()
            );
        }
    }
    Ok(())
}

/// Parses the `upgradeEvents` map of a stream image into typed snapshots.
/// A present-but-malformed entry is an error, not a skip: silently dropping
/// it would lose a committed phase without a trace.
fn image_snapshots(
    item: &HashMap<String, AttributeValue>,
) -> Result<HashMap<String, UpgradeEventSnapshot>> {
    let mut snapshots = HashMap::new();
    let Some(AttributeValue::M(entries)) = item.get(UPGRADE_EVENTS) else {
        return Ok(snapshots);
    };
    for (environment, value) in entries {
        let map = value
            .as_m()
            .map_err(|_| anyhow::anyhow!("upgradeEvents.{environment} is not a map"))?;
        let snapshot = attr_to_upgrade_snapshot(map)
            .with_context(|| format!("upgradeEvents.{environment} is malformed"))?;
        snapshots.insert(environment.clone(), snapshot);
    }
    Ok(snapshots)
}

/// DynamoDB stream images are the service's JSON form (`{"S": ...}`); the
/// store module works on SDK attribute values. Handles every variant the
/// event-stream record uses, including nested snapshot maps.
fn image_to_item(image: &Value) -> Option<HashMap<String, AttributeValue>> {
    let object = image.as_object()?;
    object
        .iter()
        .map(|(key, value)| json_to_attr(value).map(|attr| (key.clone(), attr)))
        .collect()
}

fn json_to_attr(value: &Value) -> Option<AttributeValue> {
    let object = value.as_object()?;
    let (kind, raw) = object.iter().next()?;
    match kind.as_str() {
        "S" => Some(AttributeValue::S(raw.as_str()?.to_string())),
        "N" => Some(AttributeValue::N(raw.as_str()?.to_string())),
        "BOOL" => Some(AttributeValue::Bool(raw.as_bool()?)),
        "NULL" => Some(AttributeValue::Null(raw.as_bool().unwrap_or(true))),
        "M" => Some(AttributeValue::M(image_to_item(raw)?)),
        "L" => Some(AttributeValue::L(
            raw.as_array()?
                .iter()
                .map(json_to_attr)
                .collect::<Option<Vec<_>>>()?,
        )),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::TimeZone;

    use konductor_common::streams::{EnvStream, EventStream, StreamStatus};
    use konductor_common::streams_db::stream_to_item;
    use konductor_common::system_events::{
        ContractSelector, ContractSelectors, UpgradeOutcome, UpgradePhase, UpgradeTrigger,
    };

    const BUS: &str = "acme-konductor-system-bus";

    fn set_env() {
        env::set_var("SYSTEM_EVENT_BUS", BUS);
        env::set_var("ORGANIZATION", "acme");
    }

    fn at() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.timestamp_millis_opt(1785407400123).unwrap()
    }

    fn scope() -> SnapshotScope<'static> {
        SnapshotScope {
            organization: "acme",
            consumer_domain: "checkout",
            producer_domain: "orders",
            schema_name: "acme-orders@OrderNotification",
            detail_type: "OrderNotification",
            environment: "prod",
        }
    }

    fn selector(version: i64) -> ContractSelector {
        ContractSelector {
            producer_domain: "orders".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            environment: "prod".to_string(),
            schema_version: version,
            metadata_revision: 0,
        }
    }

    fn snapshot(phase: UpgradePhase, generation: u64) -> UpgradeEventSnapshot {
        let mut snapshot = UpgradeEventSnapshot::new(
            &scope(),
            phase,
            "promotion-42",
            UpgradeTrigger::Promotion,
            generation,
            at(),
            3,
            4,
            None,
            None,
            true,
            StreamStatus::Active,
            None,
            ContractSelectors {
                from: selector(3),
                requested: selector(4),
                resulting: None,
            },
            None,
            None,
        )
        .unwrap();
        if matches!(phase, UpgradePhase::Terminal) {
            snapshot.resulting_version = Some(4);
            snapshot.contracts.resulting = Some(selector(4));
            snapshot.outcome = Some(UpgradeOutcome::Completed);
        }
        snapshot
    }

    fn stream_with_snapshots(snapshots: Option<UpgradeEventSnapshot>) -> EventStream {
        EventStream {
            producer_domain: "orders".to_string(),
            consumer_domain: "checkout".to_string(),
            source: "acme-orders".to_string(),
            detail_type: "OrderNotification".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            status: StreamStatus::Active,
            error_detail: None,
            environments: HashMap::from([(
                "prod".to_string(),
                EnvStream {
                    pinned_version: Some(3),
                    rule_name: "checkout-OrderNotification".to_string(),
                    rule_arn: None,
                    enabled: true,
                    status: StreamStatus::Active,
                    error_detail: None,
                },
            )]),
            upgrade_events: snapshots.map(|s| HashMap::from([("prod".to_string(), s)])),
            request_id: None,
            created_at: at(),
            updated_at: at(),
            removed_at: None,
            expires_at: None,
        }
    }

    fn attr_to_json(value: &AttributeValue) -> Value {
        match value {
            AttributeValue::S(s) => json!({ "S": s }),
            AttributeValue::N(n) => json!({ "N": n }),
            AttributeValue::Bool(b) => json!({ "BOOL": b }),
            AttributeValue::Null(b) => json!({ "NULL": b }),
            AttributeValue::M(m) => json!({ "M": Value::Object(
                m.iter().map(|(k, v)| (k.clone(), attr_to_json(v))).collect(),
            ) }),
            AttributeValue::L(l) => {
                json!({ "L": l.iter().map(attr_to_json).collect::<Vec<_>>() })
            }
            other => panic!("unexpected attribute {other:?}"),
        }
    }

    fn stream_image(stream: &EventStream) -> Value {
        Value::Object(
            stream_to_item(stream)
                .iter()
                .map(|(k, v)| (k.clone(), attr_to_json(v)))
                .collect(),
        )
    }

    fn record(event_name: &str, sequence: &str, old: Option<Value>, new: Option<Value>) -> Value {
        let mut dynamodb = json!({ "SequenceNumber": sequence });
        if let Some(old) = old {
            dynamodb["OldImage"] = old;
        }
        if let Some(new) = new {
            dynamodb["NewImage"] = new;
        }
        json!({ "eventID": sequence, "eventName": event_name, "dynamodb": dynamodb })
    }

    fn batch(records: Vec<Value>) -> LambdaEvent<Value> {
        LambdaEvent::new(json!({ "Records": records }), Default::default())
    }

    fn quiet_events() -> Rule {
        mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| unreachable!("nothing may be published"))
    }

    fn clients(rules: &[&Rule]) -> Clients {
        Clients {
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, rules),
        }
    }

    #[tokio::test]
    async fn changed_phase_emits_the_public_event() {
        set_env();
        let publish = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                req.entries().len() == 1
                    && req.entries()[0].source() == Some("konductor.system")
                    && req.entries()[0].detail_type() == Some("SubscriptionUpgradeStarted")
                    && req.entries()[0].event_bus_name() == Some(BUS)
                    && req.entries()[0].detail().is_some_and(|d| {
                        d.contains("\"resourceRevision\":5")
                            && d.contains("\"correlationId\":\"promotion-42\"")
                            && d.contains("\"publishedAt\":\"2026-07-30T10:30:00.123Z\"")
                            && d.contains("\"idempotencyKey\":\"system-event/v1/")
                            && d.contains("\"schemaVersion\":1")
                    })
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .build()
            });
        let clients = clients(&[&publish]);
        let record = record(
            "MODIFY",
            "111",
            Some(stream_image(&stream_with_snapshots(None))),
            Some(stream_image(&stream_with_snapshots(Some(snapshot(
                UpgradePhase::Started,
                3,
            ))))),
        );

        let out = handle(&clients, batch(vec![record])).await.unwrap();

        assert_eq!(out["batchItemFailures"].as_array().unwrap().len(), 0);
        assert_eq!(publish.num_calls(), 1);
    }

    #[tokio::test]
    async fn unchanged_snapshot_and_legacy_and_remove_records_emit_nothing() {
        set_env();
        let quiet = quiet_events();
        let clients = clients(&[&quiet]);
        let committed = stream_with_snapshots(Some(snapshot(UpgradePhase::Started, 3)));
        let unchanged = record(
            "MODIFY",
            "111",
            Some(stream_image(&committed)),
            Some(stream_image(&committed)),
        );
        let legacy = record(
            "MODIFY",
            "222",
            Some(stream_image(&stream_with_snapshots(None))),
            Some(stream_image(&stream_with_snapshots(None))),
        );
        let removed = record("REMOVE", "333", Some(stream_image(&committed)), None);

        let out = handle(&clients, batch(vec![unchanged, legacy, removed]))
            .await
            .unwrap();

        assert_eq!(out["batchItemFailures"].as_array().unwrap().len(), 0);
        assert_eq!(quiet.num_calls(), 0);
    }

    #[tokio::test]
    async fn retries_reproduce_the_identical_payload() {
        set_env();
        let publish = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                req.entries()[0]
                    .detail()
                    .is_some_and(|d| d.contains("\"publishedAt\":\"2026-07-30T10:30:00.123Z\""))
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .build()
            });
        let clients = clients(&[&publish]);
        let make = || {
            record(
                "MODIFY",
                "111",
                Some(stream_image(&stream_with_snapshots(None))),
                Some(stream_image(&stream_with_snapshots(Some(snapshot(
                    UpgradePhase::Started,
                    3,
                ))))),
            )
        };

        handle(&clients, batch(vec![make()])).await.unwrap();
        handle(&clients, batch(vec![make()])).await.unwrap();

        assert_eq!(
            publish.num_calls(),
            2,
            "both attempts publish the same detail"
        );
    }

    #[tokio::test]
    async fn per_entry_failure_marks_only_the_failing_record() {
        set_env();
        let publish = mock!(aws_sdk_eventbridge::Client::put_events).then_output(|| {
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
        let clients = clients(&[&publish]);
        let failing = record(
            "MODIFY",
            "111",
            Some(stream_image(&stream_with_snapshots(None))),
            Some(stream_image(&stream_with_snapshots(Some(snapshot(
                UpgradePhase::Started,
                3,
            ))))),
        );
        let unrelated = record(
            "MODIFY",
            "222",
            Some(stream_image(&stream_with_snapshots(None))),
            Some(stream_image(&stream_with_snapshots(None))),
        );

        let out = handle(&clients, batch(vec![failing, unrelated]))
            .await
            .unwrap();

        let failures = out["batchItemFailures"].as_array().unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0]["itemIdentifier"], "111");
    }

    #[tokio::test]
    async fn malformed_snapshot_fails_the_record_loudly() {
        set_env();
        let quiet = quiet_events();
        let clients = clients(&[&quiet]);
        let mut new_image = stream_image(&stream_with_snapshots(None));
        new_image["upgradeEvents"] = json!({
            "M": { "prod": { "M": { "phase": { "S": "STARTED" } } } }
        });
        let record = record("MODIFY", "111", None, Some(new_image));

        let out = handle(&clients, batch(vec![record])).await.unwrap();

        let failures = out["batchItemFailures"].as_array().unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(quiet.num_calls(), 0);
    }

    #[tokio::test]
    async fn an_old_image_at_a_newer_revision_emits_nothing() {
        set_env();
        let quiet = quiet_events();
        let clients = clients(&[&quiet]);
        let record = record(
            "MODIFY",
            "111",
            Some(stream_image(&stream_with_snapshots(Some(snapshot(
                UpgradePhase::Terminal,
                3,
            ))))),
            Some(stream_image(&stream_with_snapshots(Some(snapshot(
                UpgradePhase::Started,
                3,
            ))))),
        );

        let out = handle(&clients, batch(vec![record])).await.unwrap();

        assert_eq!(out["batchItemFailures"].as_array().unwrap().len(), 0);
        assert_eq!(quiet.num_calls(), 0);
    }

    #[tokio::test]
    async fn changed_environments_of_one_record_publish_in_one_call() {
        set_env();
        let mut stream = stream_with_snapshots(Some(snapshot(UpgradePhase::Terminal, 3)));
        let mut staging = snapshot(UpgradePhase::Started, 2);
        staging.idempotency_key = format!("{}-staging", staging.idempotency_key);
        stream
            .upgrade_events
            .as_mut()
            .unwrap()
            .insert("staging".to_string(), staging);
        let publish = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                req.entries().len() == 2
                    && req
                        .entries()
                        .iter()
                        .any(|e| e.detail_type() == Some("SubscriptionUpgradeStarted"))
                    && req
                        .entries()
                        .iter()
                        .any(|e| e.detail_type() == Some("SubscriptionUpgraded"))
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .build()
            });
        let clients = clients(&[&publish]);
        let record = record(
            "MODIFY",
            "111",
            Some(stream_image(&stream_with_snapshots(None))),
            Some(stream_image(&stream)),
        );

        let out = handle(&clients, batch(vec![record])).await.unwrap();

        assert_eq!(out["batchItemFailures"].as_array().unwrap().len(), 0);
        assert_eq!(publish.num_calls(), 1);
    }

    #[tokio::test]
    async fn redrive_of_a_failed_payload_preserves_event_identity() {
        set_env();
        let attempted = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        let captured = std::sync::Arc::clone(&attempted);
        let expected = std::sync::Arc::clone(&attempted);
        let fail = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(move |req| {
                *captured.lock().unwrap() = req.entries()[0].detail().map(str::to_string);
                true
            })
            .then_error(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsError::generic(
                    aws_sdk_eventbridge::error::ErrorMetadata::builder()
                        .message("bus unavailable")
                        .build(),
                )
            });
        let succeed = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(move |req| {
                req.entries()[0].detail().map(str::to_string) == *expected.lock().unwrap()
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .build()
            });
        let clients = Clients {
            eb: mock_client!(
                aws_sdk_eventbridge,
                RuleMode::Sequential,
                &[&fail, &succeed]
            ),
        };
        let make_records = || {
            vec![record(
                "MODIFY",
                "111",
                Some(stream_image(&stream_with_snapshots(None))),
                Some(stream_image(&stream_with_snapshots(Some(snapshot(
                    UpgradePhase::Started,
                    3,
                ))))),
            )]
        };

        let failed = handle(&clients, batch(make_records())).await.unwrap();
        assert_eq!(failed["batchItemFailures"][0]["itemIdentifier"], "111");
        assert!(
            attempted.lock().unwrap().is_some(),
            "the failed attempt captured the payload destined for the failure bucket"
        );

        // Recovery replays the original records (retained in S3 beyond the
        // 24-hour stream window); the projected event must be identical.
        let recovered = handle(&clients, batch(make_records())).await.unwrap();
        assert_eq!(
            recovered["batchItemFailures"].as_array().unwrap().len(),
            0,
            "the redrive publishes byte-identical event detail"
        );
        assert_eq!(fail.num_calls(), 1);
        assert_eq!(succeed.num_calls(), 1);
    }
}
