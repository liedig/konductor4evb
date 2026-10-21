//! Offline integration fixture for domain system events: a committed
//! per-environment upgrade-phase snapshot flows through DynamoDB-stream
//! projection, system-bus routing to the domain topic, and public-contract
//! validation — across promotion, authoring, paused entries, partial replay,
//! entry failure, duplicate delivery, and supersession. No AWS calls: every
//! client is mocked, and the fixtures exercise the real commit, projector,
//! router, and notifier handlers in sequence.

use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};

use aws_sdk_dynamodb::types::AttributeValue;
use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
use chrono::{DateTime, TimeZone, Utc};
use lambda_runtime::LambdaEvent;
use serde_json::{json, Value};

use konductor_common::streams::{EnvStream, EventStream, StreamStatus};
use konductor_common::streams_db::stream_to_item;
use konductor_common::system_events::store::{
    commit_upgrade_phase, commit_upgrade_terminal, PhaseCommitOutcome, TerminalWrite, UpgradeTarget,
};
use konductor_common::system_events::{
    ContractSelector, ContractSelectors, PartialReason, SnapshotScope, UpgradeEventSnapshot,
    UpgradeOutcome, UpgradePhase, UpgradeTrigger,
};

const ORG: &str = "acme";
const DOMAIN: &str = "checkout";
const ACCOUNT: &str = "999988887777";
const REGION: &str = "eu-west-1";
const BUS: &str = "acme-konductor-system-bus";
const TOPIC_ARN: &str = "arn:aws:sns:eu-west-1:999988887777:acme-checkout-system-events";
const TABLE: &str = "konductor-event-streams";

fn set_env() {
    env::set_var("SYSTEM_EVENT_BUS", BUS);
    env::set_var("DOMAINS_TABLE", "konductor-domains");
    env::set_var("ORGANIZATION", ORG);
    env::set_var("PLATFORM_PARTITION", "aws");
    env::set_var("PLATFORM_ACCOUNT_ID", ACCOUNT);
    env::set_var("AWS_REGION", REGION);
    env::set_var("NOTIFICATION_SENDER_EMAIL", "noreply@example.com");
}

fn at() -> DateTime<Utc> {
    Utc.timestamp_millis_opt(1785407400123).unwrap()
}

fn scope() -> SnapshotScope<'static> {
    SnapshotScope {
        organization: ORG,
        consumer_domain: DOMAIN,
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

fn start_snapshot(trigger: UpgradeTrigger, generation: u64, enabled: bool) -> UpgradeEventSnapshot {
    UpgradeEventSnapshot::new(
        &scope(),
        UpgradePhase::Started,
        "promotion-42",
        trigger,
        generation,
        at(),
        3,
        4,
        None,
        None,
        enabled,
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
    .unwrap()
}

fn terminal_snapshot(
    generation: u64,
    outcome: UpgradeOutcome,
    reason: Option<PartialReason>,
    resulting: Option<i64>,
) -> UpgradeEventSnapshot {
    let mut snapshot = UpgradeEventSnapshot::new(
        &scope(),
        UpgradePhase::Terminal,
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
        Some(outcome),
        reason,
    )
    .unwrap();
    snapshot.resulting_version = resulting;
    snapshot.contracts.resulting = resulting.map(selector);
    snapshot
}

fn stream_with(snapshot: Option<&UpgradeEventSnapshot>, pinned: Option<i64>) -> EventStream {
    EventStream {
        producer_domain: "orders".to_string(),
        consumer_domain: DOMAIN.to_string(),
        source: "acme-orders".to_string(),
        detail_type: "OrderNotification".to_string(),
        schema_name: "acme-orders@OrderNotification".to_string(),
        status: StreamStatus::Active,
        error_detail: None,
        environments: HashMap::from([(
            "prod".to_string(),
            EnvStream {
                pinned_version: pinned,
                rule_name: "checkout-OrderNotification".to_string(),
                rule_arn: None,
                enabled: true,
                status: StreamStatus::Active,
                error_detail: None,
            },
        )]),
        upgrade_events: snapshot.map(|s| HashMap::from([("prod".to_string(), s.clone())])),
        request_id: None,
        created_at: at(),
        updated_at: at(),
        removed_at: None,
        expires_at: None,
    }
}

/// Committed item image the store's conditional write leaves behind.
fn committed_item(stream: &EventStream) -> HashMap<String, AttributeValue> {
    stream_to_item(stream)
}

async fn commit_start(snapshot: &UpgradeEventSnapshot) -> HashMap<String, AttributeValue> {
    let item = committed_item(&stream_with(Some(snapshot), Some(3)));
    let update = mock!(aws_sdk_dynamodb::Client::update_item)
        .match_requests(|req| {
            req.condition_expression().is_some_and(|c| {
                c.contains("attribute_not_exists(upgradeEvents") || c.contains("resourceRevision")
            })
        })
        .then_output(move || {
            aws_sdk_dynamodb::operation::update_item::UpdateItemOutput::builder()
                .set_attributes(Some(item.clone()))
                .build()
        });
    let client = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&update]);
    let outcome = commit_upgrade_phase(
        &client,
        TABLE,
        "orders",
        "acme-orders@OrderNotification",
        DOMAIN,
        "prod",
        snapshot,
    )
    .await
    .unwrap();
    assert_eq!(outcome, PhaseCommitOutcome::Committed);
    assert_eq!(update.num_calls(), 1);
    committed_item(&stream_with(Some(snapshot), Some(3)))
}

async fn commit_terminal(
    write: TerminalWrite<'static>,
    snapshot: &UpgradeEventSnapshot,
    pinned: Option<i64>,
    update_counter: &Rule,
    response_item: HashMap<String, AttributeValue>,
) -> HashMap<String, AttributeValue> {
    let client = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [update_counter]);
    let target = UpgradeTarget {
        producer_domain: "orders",
        schema_name: "acme-orders@OrderNotification",
        consumer_domain: DOMAIN,
        environment: "prod",
    };
    let outcome = commit_upgrade_terminal(&client, TABLE, &target, &write, snapshot)
        .await
        .unwrap();
    assert_eq!(outcome, PhaseCommitOutcome::Committed);
    let _ = pinned;
    response_item
}

fn update_rule(item: HashMap<String, AttributeValue>) -> Rule {
    mock!(aws_sdk_dynamodb::Client::update_item).then_output(move || {
        aws_sdk_dynamodb::operation::update_item::UpdateItemOutput::builder()
            .set_attributes(Some(item.clone()))
            .build()
    })
}

// ---- stream projection helpers ----

fn attr_to_json(value: &AttributeValue) -> Value {
    match value {
        AttributeValue::S(s) => json!({ "S": s }),
        AttributeValue::N(n) => json!({ "N": n }),
        AttributeValue::Bool(b) => json!({ "BOOL": b }),
        AttributeValue::Null(b) => json!({ "NULL": b }),
        AttributeValue::M(m) => json!({ "M": Value::Object(
            m.iter().map(|(k, v)| (k.clone(), attr_to_json(v))).collect(),
        ) }),
        AttributeValue::L(l) => json!({ "L": l.iter().map(attr_to_json).collect::<Vec<_>>() }),
        other => panic!("unexpected attribute {other:?}"),
    }
}

fn image(item: &HashMap<String, AttributeValue>) -> Value {
    Value::Object(
        item.iter()
            .map(|(k, v)| (k.clone(), attr_to_json(v)))
            .collect(),
    )
}

fn record(sequence: &str, old: Option<Value>, new: Value) -> Value {
    let mut dynamodb = json!({ "SequenceNumber": sequence });
    if let Some(old) = old {
        dynamodb["OldImage"] = old;
    }
    dynamodb["NewImage"] = new;
    json!({ "eventID": sequence, "eventName": "MODIFY", "dynamodb": dynamodb })
}

/// Runs the projector and returns every published entry as (detail-type, detail).
async fn project(seq: &str, old: Option<Value>, new: Value) -> Vec<(String, Value)> {
    let published = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&published);
    let put = mock!(aws_sdk_eventbridge::Client::put_events)
        .match_requests(move |req| {
            for entry in req.entries() {
                captured.lock().unwrap().push((
                    entry.detail_type().unwrap_or_default().to_string(),
                    serde_json::from_str::<Value>(entry.detail().unwrap_or("{}")).unwrap(),
                ));
            }
            true
        })
        .then_output(|| {
            aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                .failed_entry_count(0)
                .build()
        });
    let clients = system_event_projector::handler::Clients {
        eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, [&put]),
    };
    let event = LambdaEvent::new(
        json!({ "Records": [record(seq, old, new)] }),
        Default::default(),
    );
    let out = system_event_projector::handler::handle(&clients, event)
        .await
        .unwrap();
    assert_eq!(out["batchItemFailures"], json!([]));
    let published = published.lock().unwrap().clone();
    published
}

// ---- routing helpers ----

fn bus_event(id: &str, detail_type: &str, detail: Value) -> Value {
    json!({
        "version": "0",
        "id": id,
        "detail-type": detail_type,
        "source": "konductor.system",
        "account": ACCOUNT,
        "time": "2026-07-30T10:30:00.123Z",
        "region": REGION,
        "resources": [],
        "detail": detail
    })
}

struct Routed {
    message: Value,
    attributes: HashMap<String, String>,
}

async fn route(event: Value) -> Routed {
    let published = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&published);
    let get = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
        aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
            .set_item(Some(HashMap::from([
                (
                    "domainName".to_string(),
                    AttributeValue::S(DOMAIN.to_string()),
                ),
                (
                    "status".to_string(),
                    AttributeValue::S("ACTIVE".to_string()),
                ),
                (
                    "systemEventsTopicArn".to_string(),
                    AttributeValue::S(TOPIC_ARN.to_string()),
                ),
            ])))
            .build()
    });
    let publish = mock!(aws_sdk_sns::Client::publish)
        .match_requests(move |req| {
            assert_eq!(req.topic_arn(), Some(TOPIC_ARN));
            let attrs = req
                .message_attributes()
                .map(|a| {
                    a.iter()
                        .map(|(k, v)| (k.clone(), v.string_value().unwrap_or_default().to_string()))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            captured.lock().unwrap().push((
                serde_json::from_str::<Value>(req.message().unwrap_or("{}")).unwrap(),
                attrs,
            ));
            true
        })
        .then_output(|| {
            aws_sdk_sns::operation::publish::PublishOutput::builder()
                .message_id("m-1")
                .build()
        });
    let stamp = mock!(aws_sdk_dynamodb::Client::update_item)
        .match_requests(|req| {
            req.condition_expression()
                .is_some_and(|c| c.contains("systemEventsLastEventAt"))
        })
        .then_output(|| {
            aws_sdk_dynamodb::operation::update_item::UpdateItemOutput::builder().build()
        });
    let clients = system_event_router::handler::Clients {
        ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&get, &stamp]),
        sns: mock_client!(aws_sdk_sns, RuleMode::MatchAny, [&publish]),
    };
    system_event_router::handler::handle(&clients, LambdaEvent::new(event, Default::default()))
        .await
        .unwrap();
    let published = published.lock().unwrap().clone();
    assert_eq!(published.len(), 1, "exactly one publish per event");
    let (message, attributes) = published.into_iter().next().unwrap();
    Routed {
        message,
        attributes,
    }
}

// ---- contract selectors ----

fn validator_for(detail_type: &str) -> jsonschema::Validator {
    let file = match detail_type {
        "SubscriptionUpgradeStarted" => "subscription-upgrade-started.schema.json",
        "SubscriptionUpgraded" => "subscription-upgraded.schema.json",
        other => panic!("no contract for {other}"),
    };
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../api/system-events")
        .join(file);
    let content = std::fs::read_to_string(path).unwrap();
    let schema = serde_json::from_str(&content).unwrap();
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft4)
        .build(&schema)
        .unwrap()
}

fn assert_contract(detail_type: &str, message: &Value) {
    let validator = validator_for(detail_type);
    if let Err(error) = validator.validate(message) {
        panic!("{detail_type} message fails the public contract: {error}");
    }
}

// ---- notifier helpers ----

fn notifier_clients(ses_rules: &[&Rule]) -> system_event_notifier::handler::Clients {
    let get = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
        aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
            .set_item(Some(HashMap::from([
                (
                    "domainName".to_string(),
                    AttributeValue::S(DOMAIN.to_string()),
                ),
                (
                    "ownerEmail".to_string(),
                    AttributeValue::S("owner@example.com".to_string()),
                ),
            ])))
            .build()
    });
    system_event_notifier::handler::Clients {
        ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&get]),
        ses: mock_client!(aws_sdk_sesv2, RuleMode::MatchAny, ses_rules),
    }
}

// ---- journeys ----

#[tokio::test]
async fn promotion_start_and_completed_flow_from_commit_to_topic() {
    set_env();

    let start = start_snapshot(UpgradeTrigger::Promotion, 1, true);
    let start_item = commit_start(&start).await;

    let started = project(
        "100",
        Some(image(&committed_item(&stream_with(None, Some(3))))),
        image(&start_item),
    )
    .await;
    assert_eq!(started.len(), 1);
    let (detail_type, detail) = &started[0];
    assert_eq!(detail_type, "SubscriptionUpgradeStarted");
    assert_eq!(detail["data"]["trigger"], "PROMOTION");

    let event = bus_event("evt-start", detail_type, detail.clone());
    let routed = route(event.clone()).await;
    assert_contract("SubscriptionUpgradeStarted", &routed.message);
    assert_eq!(routed.message["detail"], event["detail"]);
    assert_eq!(routed.attributes["trigger"], "PROMOTION");

    let terminal = terminal_snapshot(2, UpgradeOutcome::Completed, None, Some(4));
    let terminal_item = committed_item(&stream_with(Some(&terminal), Some(4)));
    let completed = project("101", Some(image(&start_item)), image(&terminal_item)).await;
    assert_eq!(completed.len(), 1);
    let (detail_type, detail) = &completed[0];
    assert_eq!(detail_type, "SubscriptionUpgraded");
    assert_eq!(detail["data"]["outcome"], "COMPLETED");

    let routed = route(bus_event("evt-end", detail_type, detail.clone())).await;
    assert_contract("SubscriptionUpgraded", &routed.message);
    assert_eq!(routed.attributes["outcome"], "COMPLETED");
}

#[tokio::test]
async fn authoring_and_paused_events_route_but_never_email() {
    set_env();

    let paused_authoring = start_snapshot(UpgradeTrigger::Authoring, 7, false);
    let item = commit_start(&paused_authoring).await;
    let emitted = project(
        "200",
        Some(image(&committed_item(&stream_with(None, Some(3))))),
        image(&item),
    )
    .await;
    assert_eq!(emitted.len(), 1, "paused entries still publish a start");
    let (detail_type, detail) = &emitted[0];
    assert_eq!(detail["data"]["trigger"], "AUTHORING");
    assert_eq!(detail["data"]["enabled"], false);

    let event = bus_event("evt-authoring", detail_type, detail.clone());
    let routed = route(event.clone()).await;
    assert_contract("SubscriptionUpgradeStarted", &routed.message);

    let silent = mock!(aws_sdk_sesv2::Client::send_email)
        .then_output(|| unreachable!("authoring re-pins never email"));
    let clients = notifier_clients(&[&silent]);
    system_event_notifier::handler::handle(&clients, LambdaEvent::new(event, Default::default()))
        .await
        .unwrap();
    assert_eq!(silent.num_calls(), 0);
}

#[tokio::test]
async fn partial_replay_and_entry_failure_stay_truthful_end_to_end() {
    set_env();
    let base = image(&committed_item(&stream_with(
        Some(&start_snapshot(UpgradeTrigger::Promotion, 1, true)),
        Some(3),
    )));

    let partial = terminal_snapshot(
        2,
        UpgradeOutcome::Partial,
        Some(PartialReason::ReplayPartial),
        Some(3),
    );
    let partial_item = committed_item(&stream_with(Some(&partial), Some(3)));
    let emitted = project("300", Some(base.clone()), image(&partial_item)).await;
    assert_eq!(emitted.len(), 1);
    let (detail_type, detail) = &emitted[0];
    assert_eq!(detail["data"]["outcome"], "PARTIAL");
    assert_eq!(detail["data"]["reason"], "REPLAY_PARTIAL");
    assert_eq!(detail["data"]["resultingVersion"], 3);
    let routed = route(bus_event("evt-partial", detail_type, detail.clone())).await;
    assert_contract("SubscriptionUpgraded", &routed.message);

    let failure = terminal_snapshot(
        2,
        UpgradeOutcome::Partial,
        Some(PartialReason::RepinFailed),
        Some(3),
    );
    let failure_item = committed_item(&stream_with(Some(&failure), Some(3)));
    let update = update_rule(failure_item.clone());
    let target_snapshot = failure.clone();
    let committed = commit_terminal(
        TerminalWrite::Failure("rule evaluation failed"),
        &target_snapshot,
        Some(3),
        &update,
        failure_item,
    )
    .await;
    assert_eq!(
        update.num_calls(),
        1,
        "the failure marker lands in one commit"
    );

    let emitted = project("301", Some(base), image(&committed)).await;
    assert_eq!(emitted.len(), 1);
    let (detail_type, detail) = &emitted[0];
    assert_eq!(detail["data"]["reason"], "REPIN_FAILED");
    let routed = route(bus_event("evt-failure", detail_type, detail.clone())).await;
    assert_contract("SubscriptionUpgraded", &routed.message);

    let email = mock!(aws_sdk_sesv2::Client::send_email)
        .match_requests(|req| {
            req.content()
                .and_then(|c| c.simple())
                .is_some_and(|m| m.subject().is_some_and(|s| s.data().contains("Partially")))
        })
        .then_output(|| aws_sdk_sesv2::operation::send_email::SendEmailOutput::builder().build());
    let clients = notifier_clients(&[&email]);
    system_event_notifier::handler::handle(
        &clients,
        LambdaEvent::new(
            bus_event("evt-failure", detail_type, detail.clone()),
            Default::default(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        email.num_calls(),
        1,
        "consumer owners hear about the partial outcome"
    );
}

#[tokio::test]
async fn duplicate_delivery_is_byte_identical_from_projection_to_topic() {
    set_env();
    let start = start_snapshot(UpgradeTrigger::Promotion, 1, true);
    let new_image = image(&committed_item(&stream_with(Some(&start), Some(3))));
    let old_image = image(&committed_item(&stream_with(None, Some(3))));

    let first = project("400", Some(old_image.clone()), new_image.clone()).await;
    let second = project("400", Some(old_image), new_image).await;
    assert_eq!(
        first, second,
        "retries republish the exact committed payload"
    );

    let (detail_type, detail) = &first[0];
    let a = route(bus_event("evt-dup", detail_type, detail.clone())).await;
    let b = route(bus_event("evt-dup", detail_type, detail.clone())).await;
    assert_eq!(
        a.message, b.message,
        "consumers deduplicate on idempotencyKey"
    );
    assert_eq!(
        a.message["detail"]["metadata"]["idempotencyKey"],
        b.message["detail"]["metadata"]["idempotencyKey"]
    );
}

#[tokio::test]
async fn superseded_and_repeat_records_emit_nothing() {
    set_env();
    let start = start_snapshot(UpgradeTrigger::Promotion, 1, true);
    let terminal = terminal_snapshot(2, UpgradeOutcome::Completed, None, Some(4));
    let start_image = image(&committed_item(&stream_with(Some(&start), Some(3))));
    let terminal_image = image(&committed_item(&stream_with(Some(&terminal), Some(4))));

    let repeated = project("500", Some(start_image.clone()), start_image.clone()).await;
    assert!(repeated.is_empty(), "an unchanged snapshot emits nothing");

    let regressed = project("501", Some(terminal_image), start_image).await;
    assert!(regressed.is_empty(), "an older revision never re-emits");
}

#[tokio::test]
async fn notification_failure_never_reverses_the_committed_pin() {
    set_env();
    let terminal = terminal_snapshot(2, UpgradeOutcome::Completed, None, Some(4));
    let terminal_item = committed_item(&stream_with(Some(&terminal), Some(4)));
    let update = update_rule(terminal_item.clone());
    let committed = commit_terminal(
        TerminalWrite::Pin(4),
        &terminal,
        Some(4),
        &update,
        terminal_item,
    )
    .await;
    assert_eq!(update.num_calls(), 1);
    let writes_before = update.num_calls();

    let emitted = project("600", None, image(&committed)).await;
    let (detail_type, detail) = &emitted[0];
    let event = bus_event("evt-pin", detail_type, detail.clone());

    let throttled = mock!(aws_sdk_sesv2::Client::send_email).then_error(|| {
        aws_sdk_sesv2::operation::send_email::SendEmailError::generic(
            aws_sdk_sesv2::error::ErrorMetadata::builder()
                .code("Throttling")
                .message("slow down")
                .build(),
        )
    });
    let clients = notifier_clients(&[&throttled]);
    let error = system_event_notifier::handler::handle(
        &clients,
        LambdaEvent::new(event, Default::default()),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("failed to send"), "{error}");

    assert_eq!(
        update.num_calls(),
        writes_before,
        "a failed notification performs no compensating write"
    );
    let envs = committed["environments"].as_m().unwrap();
    assert_eq!(
        envs["prod"].as_m().unwrap()["pinnedVersion"],
        AttributeValue::N("4".to_string()),
        "the committed pin stands"
    );
}

#[tokio::test]
async fn a_failed_projection_keeps_the_full_payload_recoverable() {
    set_env();
    let mut new_image = image(&committed_item(&stream_with(None, Some(3))));
    new_image["upgradeEvents"] = json!({
        "M": { "prod": { "M": { "phase": { "S": "STARTED" } } } }
    });
    let record = record("700", None, new_image);

    let quiet = mock!(aws_sdk_eventbridge::Client::put_events)
        .then_output(|| unreachable!("nothing is published for a malformed snapshot"));
    let clients = system_event_projector::handler::Clients {
        eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, [&quiet]),
    };
    let out = system_event_projector::handler::handle(
        &clients,
        LambdaEvent::new(json!({ "Records": [record.clone()] }), Default::default()),
    )
    .await
    .unwrap();

    assert_eq!(
        out["batchItemFailures"],
        json!([{ "itemIdentifier": "700" }]),
        "the record fails loudly at its own boundary"
    );
    assert_eq!(quiet.num_calls(), 0);

    let new_image = &record["dynamodb"]["NewImage"];
    for field in [
        "producerDomain",
        "schemaConsumer",
        "consumerDomain",
        "detailType",
        "schemaName",
        "upgradeEvents",
    ] {
        assert!(
            new_image.get(field).is_some(),
            "the failed payload retains {field}; recovery needs no archive replay"
        );
    }
}
