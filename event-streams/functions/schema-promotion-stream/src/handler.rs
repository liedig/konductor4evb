//! Consumes the promotions table stream (`sk = PROMOTION` only):
//!
//! - INSERT of a `VALIDATING` head item starts the replay workflow as
//!   `promote-{promotionId}`; `ExecutionAlreadyExists` counts as success, so
//!   Lambda redelivery never double-starts a promotion.
//! - `VALIDATING → REPLAYING` emits `SchemaPromotionStarted`, `→ COMPLETED`
//!   emits `SchemaPromotionCompleted` (with re-pin counts), and `→ FAILED`
//!   emits `SchemaPromotionFailed` (with the reason). A divergence moves
//!   `VALIDATING → FAILED` directly, so it emits only the failure.
//! - `REPIN` items, `REMOVE` records, and same-phase modifies are ignored.
//!
//! Any error fails the invocation; the event source mapping's
//! bisect-batch-on-error isolates the failing record.

use std::collections::HashMap;
use std::env;

use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_eventbridge::types::PutEventsRequestEntry;
use aws_sdk_eventbridge::Client as EventBridgeClient;
use aws_sdk_sfn::Client as SfnClient;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use konductor_common::promotion::{Promotion, PromotionStatus, PROMOTION_SK};
use konductor_common::promotion_store::promotion_from_item;

pub const EVENT_SOURCE: &str = "konductor.schema-management";

pub struct Clients {
    pub sfn: SfnClient,
    pub eb: EventBridgeClient,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            sfn: SfnClient::new(config),
            eb: EventBridgeClient::new(config),
        }
    }
}

pub async fn handle(clients: &Clients, event: LambdaEvent<Value>) -> Result<Value, Error> {
    let state_machine =
        env::var("PROMOTE_STATE_MACHINE_ARN").context("PROMOTE_STATE_MACHINE_ARN not set")?;
    let bus = env::var("KONDUKTOR_EVENT_BUS").context("KONDUKTOR_EVENT_BUS not set")?;
    let records = event
        .payload
        .get("Records")
        .and_then(|r| r.as_array())
        .context("missing Records array")?;
    for record in records {
        process_record(clients, &state_machine, &bus, record).await?;
    }
    Ok(Value::Null)
}

async fn process_record(
    clients: &Clients,
    state_machine: &str,
    bus: &str,
    record: &Value,
) -> Result<()> {
    let dynamodb = record.get("dynamodb").context("record missing dynamodb")?;
    let keys = dynamodb
        .get("Keys")
        .context("record missing dynamodb.Keys")?;
    if stream_field(keys, "sk").as_deref() != Some(PROMOTION_SK) {
        return Ok(());
    }
    let event_name = record.get("eventName").and_then(|v| v.as_str());
    let new = match dynamodb.get("NewImage") {
        Some(image) => Some(image_promotion(image).context("unreadable NewImage promotion")?),
        None => None,
    };
    let old = match dynamodb.get("OldImage") {
        Some(image) => Some(image_promotion(image).context("unreadable OldImage promotion")?),
        None => None,
    };
    match (event_name, old.as_ref(), new.as_ref()) {
        (Some("INSERT"), None, Some(p)) if p.phase == PromotionStatus::Validating => {
            start_workflow(clients, state_machine, p).await
        }
        // A re-promotion overwrites the terminal head item: the conditional
        // put replaces it, which the stream reports as MODIFY into VALIDATING.
        // The deterministic execution name keeps a redundant start harmless.
        (Some("MODIFY"), Some(before), Some(after))
            if after.phase == PromotionStatus::Validating && !before.phase.is_active() =>
        {
            start_workflow(clients, state_machine, after).await
        }
        (Some("MODIFY"), Some(before), Some(after)) => {
            emit_transition(clients, bus, before, after).await
        }
        _ => Ok(()),
    }
}

/// The workflow start is the one effect that must happen exactly once; the
/// deterministic execution name is the idempotency key.
async fn start_workflow(
    clients: &Clients,
    state_machine: &str,
    promotion: &Promotion,
) -> Result<()> {
    let input = json!({
        "promotionId": promotion.promotion_id,
        "domain": promotion.domain,
        "schemaName": promotion.schema_name,
        "sourceEnvironment": promotion.source_environment,
        "targetEnvironment": promotion.target_environment,
        "requestedVersion": promotion.requested_version,
    });
    let result = clients
        .sfn
        .start_execution()
        .state_machine_arn(state_machine)
        .name(format!("promote-{}", promotion.promotion_id))
        .input(input.to_string())
        .send()
        .await;
    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            let err = e.into_service_error();
            if err.is_execution_already_exists() {
                tracing::info!(promotion = %promotion.promotion_id, "workflow already running");
                Ok(())
            } else {
                Err(anyhow::Error::new(err).context("start promote workflow failed"))
            }
        }
    }
}

async fn emit_transition(
    clients: &Clients,
    bus: &str,
    before: &Promotion,
    after: &Promotion,
) -> Result<()> {
    let detail_type = match (before.phase, after.phase) {
        (PromotionStatus::Validating, PromotionStatus::Replaying) => Some("SchemaPromotionStarted"),
        (_, PromotionStatus::Completed) => Some("SchemaPromotionCompleted"),
        (_, PromotionStatus::Failed) => Some("SchemaPromotionFailed"),
        _ => None,
    };
    let Some(detail_type) = detail_type else {
        return Ok(());
    };
    let detail = event_payload(after);
    let entry = PutEventsRequestEntry::builder()
        .source(EVENT_SOURCE)
        .detail_type(detail_type)
        .event_bus_name(bus)
        .detail(detail.to_string())
        .build();
    let output = clients
        .eb
        .put_events()
        .entries(entry)
        .send()
        .await
        .map_err(|e| anyhow::Error::new(e.into_service_error()))
        .with_context(|| format!("emit {detail_type} failed"))?;
    if output.failed_entry_count() > 0 {
        anyhow::bail!("{detail_type} entry failed: {output:?}");
    }
    Ok(())
}

fn event_payload(p: &Promotion) -> Value {
    let mut detail = json!({
        "promotionId": p.promotion_id,
        "domain": p.domain,
        "schemaName": p.schema_name,
        "sourceEnvironment": p.source_environment,
        "targetEnvironment": p.target_environment,
        "toVersion": p.requested_version,
    });
    if let Some(from) = p.from_version {
        detail["fromVersion"] = json!(from);
    }
    match p.phase {
        PromotionStatus::Completed => {
            if let (Some(matched), Some(repinned), Some(failed)) =
                (p.repin_matched, p.repin_repinned, p.repin_failed)
            {
                detail["repin"] =
                    json!({ "matched": matched, "repinned": repinned, "failed": failed });
            }
        }
        PromotionStatus::Failed => {
            if let Some(reason) = p.reason {
                detail["reason"] = json!(reason);
            }
            if let Some(version) = p.failed_version {
                detail["failedVersion"] = json!(version);
            }
        }
        _ => {}
    }
    detail
}

fn image_promotion(image: &Value) -> Result<Promotion> {
    let item = parse_image(image).context("NewImage is not an object")?;
    promotion_from_item(&item).ok_or_else(|| anyhow::anyhow!("promotion image is malformed"))
}

/// DynamoDB stream images are the service's JSON form (`{"S": ...}`); the
/// store module works on SDK attribute values.
fn parse_image(image: &Value) -> Option<HashMap<String, AttributeValue>> {
    let object = image.as_object()?;
    let mut item = HashMap::new();
    for (key, value) in object {
        let attribute = if let Some(s) = value.get("S").and_then(|v| v.as_str()) {
            AttributeValue::S(s.to_string())
        } else if let Some(n) = value.get("N").and_then(|v| v.as_str()) {
            AttributeValue::N(n.to_string())
        } else {
            continue;
        };
        item.insert(key.clone(), attribute);
    }
    Some(item)
}

fn stream_field(image: &Value, key: &str) -> Option<String> {
    image
        .get(key)
        .and_then(|v| v.get("S"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::{DateTime, Utc};
    use konductor_common::promotion_store::promotion_to_item;

    const MACHINE: &str =
        "arn:aws:states:ap-southeast-2:123456789012:stateMachine:konductor-promote-schema";

    fn set_env() {
        env::set_var("PROMOTE_STATE_MACHINE_ARN", MACHINE);
        env::set_var("KONDUKTOR_EVENT_BUS", "konductor-bus");
    }

    fn at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
            .unwrap()
            .into()
    }

    fn promotion(phase: PromotionStatus) -> Promotion {
        Promotion {
            promotion_id: "promo-1".to_string(),
            domain: "orders".to_string(),
            schema_name: "OrderNotification".to_string(),
            source_environment: "dev".to_string(),
            target_environment: "staging".to_string(),
            from_version: Some(1),
            requested_version: 3,
            last_written_version: None,
            phase,
            reason: None,
            failed_version: None,
            actor: "oauth:sub-1".to_string(),
            repin_matched: None,
            repin_repinned: None,
            repin_failed: None,
            drain_until: None,
            created_at: at(),
            updated_at: at(),
            expires_at: Some(1_777_777_777),
        }
    }

    fn stream_image(p: &Promotion) -> Value {
        let mut image = serde_json::Map::new();
        for (key, value) in promotion_to_item(p) {
            let cell = match value {
                AttributeValue::S(s) => json!({ "S": s }),
                AttributeValue::N(n) => json!({ "N": n }),
                other => panic!("unexpected attribute {other:?}"),
            };
            image.insert(key, cell);
        }
        image.insert(
            "pk".to_string(),
            json!({ "S": "orders#OrderNotification#staging" }),
        );
        image.insert("sk".to_string(), json!({ "S": PROMOTION_SK }));
        Value::Object(image)
    }

    fn record(event_name: &str, old: Option<Value>, new: Value) -> Value {
        let mut dynamodb = json!({
            "Keys": { "pk": { "S": "orders#OrderNotification#staging" }, "sk": { "S": PROMOTION_SK } },
            "NewImage": new,
        });
        if let Some(old) = old {
            dynamodb["OldImage"] = old;
        }
        json!({ "eventID": "evt-1", "eventName": event_name, "dynamodb": dynamodb })
    }

    fn batch(records: Vec<Value>) -> LambdaEvent<Value> {
        LambdaEvent::new(json!({ "Records": records }), Default::default())
    }

    fn start_rule(expect_id: &'static str) -> Rule {
        let name = format!("promote-{expect_id}");
        mock!(aws_sdk_sfn::Client::start_execution)
            .match_requests(move |req| {
                req.state_machine_arn() == Some(MACHINE)
                    && req.name() == Some(name.as_str())
                    && req.input().is_some_and(|i| i.contains(expect_id))
            })
            .then_output(|| {
                aws_sdk_sfn::operation::start_execution::StartExecutionOutput::builder()
                    .execution_arn("arn:aws:states:::execution:x")
                    .start_date(aws_sdk_sfn::primitives::DateTime::from_secs(0))
                    .build()
                    .unwrap()
            })
    }

    fn already_exists_rule() -> Rule {
        mock!(aws_sdk_sfn::Client::start_execution).then_error(|| {
            aws_sdk_sfn::operation::start_execution::StartExecutionError::ExecutionAlreadyExists(
                aws_sdk_sfn::types::error::ExecutionAlreadyExists::builder()
                    .message("exists")
                    .build(),
            )
        })
    }

    fn event_rule(detail_type: &'static str) -> Rule {
        mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(move |req| {
                req.entries().iter().any(|e| {
                    e.detail_type() == Some(detail_type)
                        && e.source() == Some(EVENT_SOURCE)
                        && e.event_bus_name() == Some("konductor-bus")
                })
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .build()
            })
    }

    fn clients(sfn_rules: &[&Rule], eb_rules: &[&Rule]) -> Clients {
        Clients {
            sfn: mock_client!(aws_sdk_sfn, RuleMode::MatchAny, sfn_rules),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, eb_rules),
        }
    }

    #[tokio::test]
    async fn new_validating_promotion_starts_the_workflow() {
        set_env();
        let start = start_rule("promo-1");
        let clients = clients(&[&start], &[]);
        let record = record(
            "INSERT",
            None,
            stream_image(&promotion(PromotionStatus::Validating)),
        );

        let result = handle(&clients, batch(vec![record])).await;

        assert!(result.is_ok());
        assert_eq!(start.num_calls(), 1);
    }

    #[tokio::test]
    async fn re_promotion_overwrite_starts_the_workflow_without_an_event() {
        set_env();
        let start = start_rule("promo-1");
        let clients = clients(&[&start], &[]);
        // Replacing a terminal head item surfaces as MODIFY, not INSERT.
        let record = record(
            "MODIFY",
            Some(stream_image(&promotion(PromotionStatus::Completed))),
            stream_image(&promotion(PromotionStatus::Validating)),
        );

        let result = handle(&clients, batch(vec![record])).await;

        assert!(result.is_ok());
        assert_eq!(start.num_calls(), 1);
    }

    #[tokio::test]
    async fn active_to_active_modify_does_not_restart_the_workflow() {
        set_env();
        let events = event_rule("SchemaPromotionStarted");
        let clients = clients(&[], &[&events]);
        let record = record(
            "MODIFY",
            Some(stream_image(&promotion(PromotionStatus::Validating))),
            stream_image(&promotion(PromotionStatus::Replaying)),
        );

        let result = handle(&clients, batch(vec![record])).await;

        assert!(result.is_ok());
        assert_eq!(events.num_calls(), 1);
    }

    #[tokio::test]
    async fn execution_already_exists_counts_as_success() {
        set_env();
        let exists = already_exists_rule();
        let clients = clients(&[&exists], &[]);
        let record = record(
            "INSERT",
            None,
            stream_image(&promotion(PromotionStatus::Validating)),
        );

        let result = handle(&clients, batch(vec![record])).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn replaying_transition_emits_started() {
        set_env();
        let started = event_rule("SchemaPromotionStarted");
        let clients = clients(&[], &[&started]);
        let record = record(
            "MODIFY",
            Some(stream_image(&promotion(PromotionStatus::Validating))),
            stream_image(&promotion(PromotionStatus::Replaying)),
        );

        handle(&clients, batch(vec![record])).await.unwrap();

        assert_eq!(started.num_calls(), 1);
    }

    #[tokio::test]
    async fn completed_transition_emits_completed_with_repin_counts() {
        set_env();
        let mut done = promotion(PromotionStatus::Completed);
        done.repin_matched = Some(4);
        done.repin_repinned = Some(3);
        done.repin_failed = Some(1);
        let completed = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                req.entries().iter().any(|e| {
                    e.detail_type() == Some("SchemaPromotionCompleted")
                        && e.detail().is_some_and(|d| {
                            d.contains("\"promotionId\":\"promo-1\"")
                                && d.contains("\"fromVersion\":1")
                                && d.contains("\"toVersion\":3")
                                && d.contains("\"repinned\":3")
                                && d.contains("\"failed\":1")
                        })
                })
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .build()
            });
        let clients = clients(&[], &[&completed]);
        let record = record(
            "MODIFY",
            Some(stream_image(&promotion(PromotionStatus::Replaying))),
            stream_image(&done),
        );

        handle(&clients, batch(vec![record])).await.unwrap();

        assert_eq!(completed.num_calls(), 1);
    }

    #[tokio::test]
    async fn validating_to_failed_emits_only_the_failure() {
        set_env();
        let mut failed = promotion(PromotionStatus::Failed);
        failed.reason = Some(konductor_common::promotion::PromotionFailureReason::Diverged);
        let failed_rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                req.entries().iter().all(|e| {
                    e.detail_type() == Some("SchemaPromotionFailed")
                        && e.detail()
                            .is_some_and(|d| d.contains("\"reason\":\"DIVERGED\""))
                })
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .build()
            });
        let sfn_quiet = mock!(aws_sdk_sfn::Client::start_execution)
            .then_output(|| unreachable!("a transition must not start the workflow"));
        let clients = clients(&[&sfn_quiet], &[&failed_rule]);
        let record = record(
            "MODIFY",
            Some(stream_image(&promotion(PromotionStatus::Validating))),
            stream_image(&failed),
        );

        handle(&clients, batch(vec![record])).await.unwrap();

        assert_eq!(failed_rule.num_calls(), 1);
        assert_eq!(sfn_quiet.num_calls(), 0);
    }

    #[tokio::test]
    async fn repin_items_are_ignored() {
        set_env();
        let sfn_quiet = mock!(aws_sdk_sfn::Client::start_execution)
            .then_output(|| unreachable!("REPIN items start nothing"));
        let eb_quiet = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| unreachable!("REPIN items emit nothing"));
        let clients = clients(&[&sfn_quiet], &[&eb_quiet]);
        let mut record = record(
            "INSERT",
            None,
            stream_image(&promotion(PromotionStatus::Validating)),
        );
        record["dynamodb"]["Keys"]["sk"] = json!({ "S": "REPIN" });

        handle(&clients, batch(vec![record])).await.unwrap();

        assert_eq!(sfn_quiet.num_calls(), 0);
        assert_eq!(eb_quiet.num_calls(), 0);
    }

    #[tokio::test]
    async fn same_phase_modify_and_remove_are_ignored() {
        set_env();
        let sfn_quiet = mock!(aws_sdk_sfn::Client::start_execution)
            .then_output(|| unreachable!("same-phase modifies start nothing"));
        let eb_quiet = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| unreachable!("same-phase modifies emit nothing"));
        let clients = clients(&[&sfn_quiet], &[&eb_quiet]);
        let mut done = promotion(PromotionStatus::Replaying);
        done.last_written_version = Some(2);
        let progress = record(
            "MODIFY",
            Some(stream_image(&promotion(PromotionStatus::Replaying))),
            stream_image(&done),
        );
        let mut removed = record(
            "REMOVE",
            None,
            stream_image(&promotion(PromotionStatus::Completed)),
        );
        removed["dynamodb"]
            .as_object_mut()
            .unwrap()
            .remove("NewImage");
        removed["dynamodb"]["OldImage"] = stream_image(&promotion(PromotionStatus::Completed));

        handle(&clients, batch(vec![progress, removed]))
            .await
            .unwrap();

        assert_eq!(sfn_quiet.num_calls(), 0);
        assert_eq!(eb_quiet.num_calls(), 0);
    }
}
