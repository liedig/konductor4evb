use super::*;
use crate::testsupport::*;

fn content(version: i64) -> String {
    format!("{{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"v\":{version}}}")
}

#[tokio::test]
async fn catch_up_replays_the_missing_tail() {
    set_env();
    let list = list_versions_rule(TARGET_REGISTRY, &["1", "2"]);
    let s1 = get_record_rule(
        "dev",
        1,
        Some(record_item(&record("dev", 1, &content(1), "schema-admin"))),
    );
    let s2 = get_record_rule(
        "dev",
        2,
        Some(record_item(&record("dev", 2, &content(2), "schema-admin"))),
    );
    let t1 = get_record_rule(
        "staging",
        1,
        Some(record_item(&record("staging", 1, &content(1), "promo-0"))),
    );
    let t2 = get_record_rule(
        "staging",
        2,
        Some(record_item(&record("staging", 2, &content(2), "promo-0"))),
    );
    let clients = clients(&[&s1, &s2, &t1, &t2], &[&list]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], true);
    assert_eq!(out["fromVersion"], 2);
    assert_eq!(out["versions"], serde_json::json!([3]));
}

#[tokio::test]
async fn non_adjacent_pair_is_refused_before_any_registry_read() {
    set_env();
    let clients = clients(&[], &[]);
    let mut event = input("plan");
    event["targetEnvironment"] = serde_json::json!("prod");

    let out = run(&clients, &event).await.unwrap();

    assert_eq!(out["ok"], false);
    assert_eq!(out["skipped"], true);
    assert!(out.get("divergedAt").is_none());
}

#[tokio::test]
async fn authoring_environment_as_target_is_refused() {
    set_env();
    let clients = clients(&[], &[]);
    let mut event = input("plan");
    event["sourceEnvironment"] = serde_json::json!("staging");
    event["targetEnvironment"] = serde_json::json!("dev");

    let out = run(&clients, &event).await.unwrap();

    assert_eq!(out["ok"], false);
    assert_eq!(out["skipped"], true);
}

#[tokio::test]
async fn unknown_target_environment_is_refused() {
    set_env();
    let clients = clients(&[], &[]);
    let mut event = input("plan");
    event["targetEnvironment"] = serde_json::json!("qa");

    let out = run(&clients, &event).await.unwrap();

    assert_eq!(out["ok"], false);
    assert_eq!(out["skipped"], true);
}

#[tokio::test]
async fn first_arrival_replays_everything() {
    set_env();
    let missing = list_not_found_rule(TARGET_REGISTRY);
    let clients = clients(&[], &[&missing]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], true);
    assert_eq!(out["fromVersion"], 0);
    assert_eq!(out["versions"], serde_json::json!([1, 2, 3]));
}

#[tokio::test]
async fn in_sync_replays_nothing() {
    set_env();
    let list = list_versions_rule(TARGET_REGISTRY, &["1", "2", "3"]);
    let s3 = get_record_rule(
        "dev",
        3,
        Some(record_item(&record("dev", 3, &content(3), "schema-admin"))),
    );
    let t1 = get_record_rule(
        "staging",
        1,
        Some(record_item(&record("staging", 1, &content(1), "promo-0"))),
    );
    let t2 = get_record_rule(
        "staging",
        2,
        Some(record_item(&record("staging", 2, &content(2), "promo-0"))),
    );
    let t3 = get_record_rule(
        "staging",
        3,
        Some(record_item(&record("staging", 3, &content(3), "promo-0"))),
    );
    let s1 = get_record_rule(
        "dev",
        1,
        Some(record_item(&record("dev", 1, &content(1), "schema-admin"))),
    );
    let s2 = get_record_rule(
        "dev",
        2,
        Some(record_item(&record("dev", 2, &content(2), "schema-admin"))),
    );
    let clients = clients(&[&s1, &s2, &s3, &t1, &t2, &t3], &[&list]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], true);
    assert_eq!(out["fromVersion"], 3);
    assert_eq!(out["versions"], serde_json::json!([]));
}

#[tokio::test]
async fn an_unrecorded_source_version_verifies_against_the_source_registry() {
    set_env();
    let list = list_versions_rule(TARGET_REGISTRY, &["1"]);
    let s1 = get_record_rule("dev", 1, None);
    let source = describe_version_rule(SOURCE_REGISTRY, "1", content(1));
    let t1 = get_record_rule(
        "staging",
        1,
        Some(record_item(&record("staging", 1, &content(1), "promo-0"))),
    );
    let clients = clients(&[&s1, &t1], &[&list, &source]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], true);
    assert_eq!(out["fromVersion"], 1);
    assert_eq!(out["versions"], serde_json::json!([2, 3]));
}

#[tokio::test]
async fn a_fallback_source_still_catches_a_tampered_target() {
    set_env();
    let list = list_versions_rule(TARGET_REGISTRY, &["1"]);
    let s1 = get_record_rule("dev", 1, None);
    let source = describe_version_rule(SOURCE_REGISTRY, "1", content(1));
    let tampered = "{\"v\":1,\"extra\":true}".to_string();
    let t1 = get_record_rule(
        "staging",
        1,
        Some(record_item(&record("staging", 1, &tampered, "promo-0"))),
    );
    let clients = clients(&[&s1, &t1], &[&list, &source]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], false);
    assert_eq!(out["divergedAt"], 1);
}

#[tokio::test]
async fn hash_mismatch_diverges_at_the_first_differing_version() {
    set_env();
    let list = list_versions_rule(TARGET_REGISTRY, &["1", "2"]);
    let tampered = "{\"v\":2,\"extra\":true}".to_string();
    let s1 = get_record_rule(
        "dev",
        1,
        Some(record_item(&record("dev", 1, &content(1), "schema-admin"))),
    );
    let s2 = get_record_rule(
        "dev",
        2,
        Some(record_item(&record("dev", 2, &content(2), "schema-admin"))),
    );
    let t1 = get_record_rule(
        "staging",
        1,
        Some(record_item(&record("staging", 1, &content(1), "promo-0"))),
    );
    let t2 = get_record_rule(
        "staging",
        2,
        Some(record_item(&record("staging", 2, &tampered, "promo-0"))),
    );
    let clients = clients(&[&s1, &s2, &t1, &t2], &[&list]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], false);
    assert_eq!(out["divergedAt"], 2);
}

#[tokio::test]
async fn a_non_promotion_write_record_diverges() {
    set_env();
    let list = list_versions_rule(TARGET_REGISTRY, &["1"]);
    let s1 = get_record_rule(
        "dev",
        1,
        Some(record_item(&record("dev", 1, &content(1), "schema-admin"))),
    );
    let t1 = get_record_rule(
        "staging",
        1,
        Some(record_item(&record(
            "staging",
            1,
            &content(1),
            "schema-admin",
        ))),
    );
    let clients = clients(&[&s1, &t1], &[&list]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], false);
    assert_eq!(out["divergedAt"], 1);
}

#[tokio::test]
async fn a_missing_old_record_diverges_because_only_the_latest_is_readable() {
    set_env();
    let list = list_versions_rule(TARGET_REGISTRY, &["1", "2"]);
    let s1 = get_record_rule(
        "dev",
        1,
        Some(record_item(&record("dev", 1, &content(1), "schema-admin"))),
    );
    let s2 = get_record_rule(
        "dev",
        2,
        Some(record_item(&record("dev", 2, &content(2), "schema-admin"))),
    );
    let t1 = get_record_rule("staging", 1, None);
    let t2 = get_record_rule(
        "staging",
        2,
        Some(record_item(&record("staging", 2, &content(2), "promo-0"))),
    );
    let clients = clients(&[&s1, &s2, &t1, &t2], &[&list]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], false);
    assert_eq!(out["divergedAt"], 1);
}

#[tokio::test]
async fn a_missing_latest_record_is_repaired_by_canonical_comparison() {
    set_env();
    let list = list_versions_rule(TARGET_REGISTRY, &["1", "2"]);
    let s1 = get_record_rule(
        "dev",
        1,
        Some(record_item(&record("dev", 1, &content(1), "schema-admin"))),
    );
    let s2 = get_record_rule(
        "dev",
        2,
        Some(record_item(&record("dev", 2, &content(2), "schema-admin"))),
    );
    let t1 = get_record_rule(
        "staging",
        1,
        Some(record_item(&record("staging", 1, &content(1), "promo-0"))),
    );
    let t2 = get_record_rule("staging", 2, None);
    // The registry normalizes whitespace; canonical equality absorbs it.
    let pretty =
        "{\n  \"$schema\": \"http://json-schema.org/draft-04/schema#\",\n  \"v\": 2\n}".to_string();
    let describe = describe_rule(TARGET_REGISTRY, "2", pretty);
    let repair = put_record_rule("staging", 2);
    let clients = clients(&[&s1, &s2, &t1, &t2, &repair], &[&list, &describe]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], true);
    assert_eq!(out["versions"], serde_json::json!([3]));
    assert_eq!(repair.num_calls(), 1, "the latest record is completed");
}

#[tokio::test]
async fn a_missing_latest_record_with_mismatched_content_diverges() {
    set_env();
    let list = list_versions_rule(TARGET_REGISTRY, &["1", "2"]);
    let s1 = get_record_rule(
        "dev",
        1,
        Some(record_item(&record("dev", 1, &content(1), "schema-admin"))),
    );
    let s2 = get_record_rule(
        "dev",
        2,
        Some(record_item(&record("dev", 2, &content(2), "schema-admin"))),
    );
    let t1 = get_record_rule(
        "staging",
        1,
        Some(record_item(&record("staging", 1, &content(1), "promo-0"))),
    );
    let t2 = get_record_rule("staging", 2, None);
    let describe = describe_rule(TARGET_REGISTRY, "2", "{\"different\":true}".to_string());
    let clients = clients(&[&s1, &s2, &t1, &t2], &[&list, &describe]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], false);
    assert_eq!(out["divergedAt"], 2);
}

#[tokio::test]
async fn a_target_ahead_of_its_source_diverges() {
    set_env();
    let list = list_versions_rule(TARGET_REGISTRY, &["1", "2", "3", "4"]);
    let clients = clients(&[], &[&list]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], false);
    assert_eq!(out["divergedAt"], 4);
}

#[tokio::test]
async fn a_missing_source_record_diverges_because_it_is_unverifiable() {
    set_env();
    let list = list_versions_rule(TARGET_REGISTRY, &["1"]);
    let s1 = get_record_rule("dev", 1, None);
    let missing = describe_not_found_rule(SOURCE_REGISTRY);
    let t1 = get_record_rule(
        "staging",
        1,
        Some(record_item(&record("staging", 1, &content(1), "promo-0"))),
    );
    let clients = clients(&[&s1, &t1], &[&list, &missing]);

    let out = run(&clients, &input("plan")).await.unwrap();

    assert_eq!(out["ok"], false);
    assert_eq!(out["divergedAt"], 1);
}
