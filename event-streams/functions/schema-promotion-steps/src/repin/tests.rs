use std::collections::HashMap;

use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
use aws_sdk_dynamodb::operation::put_item::PutItemOutput;
use aws_sdk_dynamodb::operation::query::QueryOutput;
use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
use chrono::Duration;
use serde_json::{json, Value};

use konductor_common::dynamodb::s;
use konductor_common::promotion::{promotion_pk, RepinState, REPIN_SK};
use konductor_common::promotion_store::repin_to_item;
use konductor_common::streams::{EnvStream, EventStream, StreamStatus};
use konductor_common::streams_db::stream_to_item;

use super::*;
use crate::testsupport::{at, set_env};

fn widen_input(through: i64) -> Value {
    json!({
        "action": "widen",
        "promotionId": "promo-1",
        "domain": "orders",
        "schemaName": "OrderNotification",
        "environment": "staging",
        "through": through,
        "drainSeconds": 300,
    })
}

fn narrow_input(to: i64, generation: i64) -> Value {
    json!({
        "action": "narrow",
        "promotionId": "promo-1",
        "domain": "orders",
        "schemaName": "OrderNotification",
        "environment": "staging",
        "to": to,
        "generation": generation,
    })
}

fn repin_state(floor: i64, target: i64, generation: i64, widened_by: Option<&str>) -> RepinState {
    RepinState {
        floor_version: floor,
        target_version: target,
        generation,
        widened_by: widened_by.map(str::to_string),
        pinned_before: widened_by.map(|_| 3),
        drain_until: Some(at() + Duration::seconds(300)),
        updated_at: at(),
    }
}

fn repin_item(state: &RepinState) -> HashMap<String, AttributeValue> {
    let mut item = repin_to_item(state);
    item.insert(
        "pk".to_string(),
        s(&promotion_pk("orders", "OrderNotification", "staging")),
    );
    item.insert("sk".to_string(), s(REPIN_SK));
    item
}

fn get_repin_rule(item: Option<HashMap<String, AttributeValue>>) -> Rule {
    mock!(aws_sdk_dynamodb::Client::get_item)
        .match_requests(|req| {
            req.table_name() == Some("promotions-test")
                && req
                    .key()
                    .and_then(|k| k.get("sk"))
                    .and_then(|v| v.as_s().ok())
                    .map(String::as_str)
                    == Some(REPIN_SK)
        })
        .then_output(move || {
            let builder = GetItemOutput::builder();
            match &item {
                Some(item) => builder.set_item(Some(item.clone())).build(),
                None => builder.build(),
            }
        })
}

fn put_repin_rule(floor: i64, target: i64, generation: i64, old_generation: i64) -> Rule {
    mock!(aws_sdk_dynamodb::Client::put_item)
        .match_requests(move |req| {
            let item = req.item();
            let attr = |name: &str| item.and_then(|i| i.get(name)).and_then(|v| v.as_n().ok());
            req.table_name() == Some("promotions-test")
                && req.condition_expression()
                    == Some("attribute_not_exists(pk) OR generation = :gen")
                && attr("floorVersion") == Some(&floor.to_string())
                && attr("targetVersion") == Some(&target.to_string())
                && attr("generation") == Some(&generation.to_string())
                && req
                    .expression_attribute_values()
                    .and_then(|v| v.get(":gen"))
                    .and_then(|v| v.as_n().ok())
                    == Some(&old_generation.to_string())
        })
        .then_output(|| PutItemOutput::builder().build())
}

fn narrow_update_rule(to: i64, generation: i64) -> Rule {
    mock!(aws_sdk_dynamodb::Client::update_item)
        .match_requests(move |req| {
            req.table_name() == Some("promotions-test")
                && req.update_expression()
                    == Some(
                        "SET floorVersion = :to, targetVersion = :to, updatedAt = :now \
                         REMOVE drainUntil, pinnedBefore",
                    )
                && req.condition_expression()
                    == Some("generation = :gen AND widenedBy = :pid AND drainUntil <= :now")
                && req
                    .expression_attribute_values()
                    .and_then(|v| v.get(":to"))
                    .and_then(|v| v.as_n().ok())
                    == Some(&to.to_string())
                && req
                    .expression_attribute_values()
                    .and_then(|v| v.get(":gen"))
                    .and_then(|v| v.as_n().ok())
                    == Some(&generation.to_string())
        })
        .then_output(|| UpdateItemOutput::builder().build())
}

fn stream_item(
    consumer: &str,
    env: Option<(&str, StreamStatus, Option<i64>)>,
) -> HashMap<String, AttributeValue> {
    let mut environments = HashMap::new();
    if let Some((name, status, pinned)) = env {
        environments.insert(
            name.to_string(),
            EnvStream {
                pinned_version: pinned,
                rule_name: format!("{consumer}-OrderNotification"),
                rule_arn: None,
                enabled: true,
                status,
                error_detail: None,
            },
        );
    }
    let stream = EventStream {
        producer_domain: "orders".to_string(),
        consumer_domain: consumer.to_string(),
        source: "acme-orders".to_string(),
        detail_type: "OrderNotification".to_string(),
        schema_name: "acme-orders@OrderNotification".to_string(),
        status: StreamStatus::Active,
        error_detail: None,
        environments,
        request_id: None,
        created_at: at(),
        updated_at: at(),
        removed_at: None,
        expires_at: None,
    };
    stream_to_item(&stream)
}

fn streams_rule(items: Vec<HashMap<String, AttributeValue>>) -> Rule {
    mock!(aws_sdk_dynamodb::Client::query)
        .match_requests(|req| req.table_name() == Some("streams-test"))
        .then_output(move || {
            QueryOutput::builder()
                .set_items(Some(items.clone()))
                .build()
        })
}

fn clients_with(ddb_rules: &[&Rule]) -> Clients {
    Clients {
        ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
        schemas: mock_client!(aws_sdk_schemas, RuleMode::MatchAny, &[]),
    }
}

#[tokio::test]
async fn widen_covers_the_current_pin_through_the_target() {
    set_env();
    let streams = streams_rule(vec![stream_item(
        "shipping",
        Some(("staging", StreamStatus::Active, Some(3))),
    )]);
    let no_repin = get_repin_rule(None);
    let put = put_repin_rule(3, 5, 1, 0);
    let clients = clients_with(&[&streams, &no_repin, &put]);

    let out = widen(&clients, &widen_input(5)).await.unwrap();

    assert_eq!(out["generation"], 1);
    assert_eq!(out["oldPin"], 3);
    assert_eq!(out["pattern"], json!([3, 4, 5, 6]));
    assert!(out["drainUntil"].as_str().is_some_and(|d| !d.is_empty()));
    let entries = out["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["consumerDomain"], "shipping");
    assert_eq!(entries[0]["ruleName"], "shipping-OrderNotification");
    assert_eq!(entries[0]["environment"], "staging");
    assert_eq!(put.num_calls(), 1);
}

#[tokio::test]
async fn widen_takes_the_min_floor_and_max_target_and_bumps_the_generation() {
    set_env();
    let streams = streams_rule(vec![stream_item(
        "shipping",
        Some(("staging", StreamStatus::Active, Some(3))),
    )]);
    let existing = get_repin_rule(Some(repin_item(&repin_state(2, 4, 1, Some("promo-0")))));
    let put = put_repin_rule(2, 5, 2, 1);
    let clients = clients_with(&[&streams, &existing, &put]);

    let out = widen(&clients, &widen_input(5)).await.unwrap();

    assert_eq!(out["generation"], 2);
    assert_eq!(out["pattern"], json!([2, 3, 4, 5, 6]));
    assert_eq!(put.num_calls(), 1);
}

#[tokio::test]
async fn widen_retry_short_circuits_on_its_own_widening() {
    set_env();
    let streams = streams_rule(vec![]);
    let own = get_repin_rule(Some(repin_item(&repin_state(2, 5, 2, Some("promo-1")))));
    let put = put_repin_rule(2, 5, 3, 2);
    let clients = clients_with(&[&streams, &own, &put]);

    let out = widen(&clients, &widen_input(5)).await.unwrap();

    assert_eq!(out["generation"], 2);
    assert_eq!(out["pattern"], json!([2, 3, 4, 5, 6]));
    assert_eq!(
        put.num_calls(),
        0,
        "a retried widen must not re-bump the generation"
    );
}

#[tokio::test]
async fn widen_without_subscribers_pins_to_the_requested_version() {
    set_env();
    let streams = streams_rule(vec![]);
    let no_repin = get_repin_rule(None);
    let put = put_repin_rule(5, 5, 1, 0);
    let clients = clients_with(&[&streams, &no_repin, &put]);

    let out = widen(&clients, &widen_input(5)).await.unwrap();

    assert_eq!(out["pattern"], json!([5, 6]));
    assert_eq!(out["entries"], json!([]));
}

#[tokio::test]
async fn widen_ignores_non_active_and_other_env_entries() {
    set_env();
    let streams = streams_rule(vec![
        stream_item("shipping", Some(("staging", StreamStatus::Active, Some(3)))),
        stream_item(
            "finance",
            Some(("staging", StreamStatus::AwaitingPromotion, None)),
        ),
        stream_item("ledger", Some(("prod", StreamStatus::Active, Some(2)))),
    ]);
    let no_repin = get_repin_rule(None);
    let put = put_repin_rule(3, 5, 1, 0);
    let clients = clients_with(&[&streams, &no_repin, &put]);

    let out = widen(&clients, &widen_input(5)).await.unwrap();

    let entries = out["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["consumerDomain"], "shipping");
}

#[tokio::test]
async fn narrow_commits_the_final_pin_on_its_own_generation() {
    set_env();
    let existing = get_repin_rule(Some(repin_item(&repin_state(3, 5, 2, Some("promo-1")))));
    let update = narrow_update_rule(5, 2);
    let streams = streams_rule(vec![stream_item(
        "shipping",
        Some(("staging", StreamStatus::Active, Some(3))),
    )]);
    let clients = clients_with(&[&existing, &update, &streams]);

    let out = narrow(&clients, &narrow_input(5, 2)).await.unwrap();

    assert_eq!(out["narrowed"], true);
    assert_eq!(out["generation"], 2);
    assert_eq!(out["pattern"], json!([5, 6]));
    assert_eq!(out["entries"].as_array().unwrap().len(), 1);
    assert_eq!(update.num_calls(), 1);
}

#[tokio::test]
async fn narrow_exits_when_a_newer_widen_superseded_it() {
    set_env();
    // Two quick appends: promo-1 widened to gen 2, then promo-2's flow (or a
    // re-pin from a second append) widened to gen 3 before promo-1 drained.
    let newer = get_repin_rule(Some(repin_item(&repin_state(3, 6, 3, Some("promo-2")))));
    let update = narrow_update_rule(5, 2);
    let streams = streams_rule(vec![]);
    let clients = clients_with(&[&newer, &update, &streams]);

    let out = narrow(&clients, &narrow_input(5, 2)).await.unwrap();

    assert_eq!(
        out["narrowed"], false,
        "the newer generation owns narrowing"
    );
    assert_eq!(out["generation"], 3);
    assert_eq!(update.num_calls(), 0);
}

#[tokio::test]
async fn narrow_without_a_repin_item_is_vacuous() {
    set_env();
    let missing = get_repin_rule(None);
    let update = narrow_update_rule(5, 2);
    let streams = streams_rule(vec![]);
    let clients = clients_with(&[&missing, &update, &streams]);

    let out = narrow(&clients, &narrow_input(5, 2)).await.unwrap();

    assert_eq!(out["narrowed"], false);
    assert_eq!(out["pattern"], json!([5, 6]));
    assert_eq!(out["entries"], json!([]));
    assert_eq!(update.num_calls(), 0);
}

#[tokio::test]
async fn narrow_condition_loss_to_a_newer_generation_exits_unnarrowed() {
    set_env();
    // Sequential: the pre-check read sees our generation, the conditional
    // update loses a race, and the re-read shows the newer generation.
    let before = get_repin_rule(Some(repin_item(&repin_state(3, 5, 2, Some("promo-1")))));
    let raced = mock!(aws_sdk_dynamodb::Client::update_item).then_error(|| {
        aws_sdk_dynamodb::operation::update_item::UpdateItemError::ConditionalCheckFailedException(
            aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                .message("generation moved")
                .build(),
        )
    });
    let after = get_repin_rule(Some(repin_item(&repin_state(3, 6, 3, Some("promo-2")))));
    let clients = Clients {
        ddb: mock_client!(
            aws_sdk_dynamodb,
            RuleMode::Sequential,
            &[&before, &raced, &after]
        ),
        schemas: mock_client!(aws_sdk_schemas, RuleMode::MatchAny, &[]),
    };

    let out = narrow(&clients, &narrow_input(5, 2)).await.unwrap();

    assert_eq!(out["narrowed"], false);
    assert_eq!(out["generation"], 3);
}

#[tokio::test]
async fn narrow_condition_failure_on_its_own_generation_fails() {
    set_env();
    // drainUntil <= :now tripping on our own generation means the workflow
    // narrowed before the drain elapsed — that is a bug, not a race.
    let ours = get_repin_rule(Some(repin_item(&repin_state(3, 5, 2, Some("promo-1")))));
    let ours_again = get_repin_rule(Some(repin_item(&repin_state(3, 5, 2, Some("promo-1")))));
    let raced = mock!(aws_sdk_dynamodb::Client::update_item).then_error(|| {
        aws_sdk_dynamodb::operation::update_item::UpdateItemError::ConditionalCheckFailedException(
            aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                .message("drain not elapsed")
                .build(),
        )
    });
    let clients = Clients {
        ddb: mock_client!(
            aws_sdk_dynamodb,
            RuleMode::Sequential,
            &[&ours, &raced, &ours_again]
        ),
        schemas: mock_client!(aws_sdk_schemas, RuleMode::MatchAny, &[]),
    };

    let result = narrow(&clients, &narrow_input(5, 2)).await;

    assert!(result.is_err());
}

#[test]
fn pattern_is_the_widened_range_plus_lookahead() {
    assert_eq!(pattern(3, 5), vec![3, 4, 5, 6]);
    assert_eq!(pattern(5, 5), vec![5, 6]);
}
