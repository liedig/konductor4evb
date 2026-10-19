use aws_smithy_mocks::Rule;

use super::*;
use crate::testsupport::*;

fn content(version: i64) -> String {
    format!("{{\"v\":{version}}}")
}

fn replay_input(version: i64) -> Value {
    let mut input = input("replay");
    input["version"] = serde_json::json!(version);
    input
}

fn update_rule(version: &'static str) -> Rule {
    use aws_smithy_mocks::mock;
    let expected = content(3);
    mock!(aws_sdk_schemas::Client::update_schema)
        .match_requests(move |req| {
            req.registry_name() == Some(TARGET_REGISTRY) && req.content() == Some(expected.as_str())
        })
        .then_output(move || {
            aws_sdk_schemas::operation::update_schema::UpdateSchemaOutput::builder()
                .schema_version(version)
                .build()
        })
}

fn no_change_conflict_rule() -> Rule {
    use aws_smithy_mocks::mock;
    mock!(aws_sdk_schemas::Client::update_schema).then_error(|| {
        aws_sdk_schemas::operation::update_schema::UpdateSchemaError::generic(
            aws_sdk_schemas::error::ErrorMetadata::builder()
                .code("ConflictException")
                .message("no change since the previous schema version")
                .build(),
        )
    })
}

fn head_commit_rule(version: i64) -> Rule {
    use aws_smithy_mocks::mock;
    let needle = version.to_string();
    mock!(aws_sdk_dynamodb::Client::update_item)
        .match_requests(move |req| {
            req.table_name() == Some("promotions-test")
                && req
                    .update_expression()
                    .is_some_and(|e| e.contains("lastWrittenVersion"))
                && req.condition_expression() == Some("promotionId = :pid")
                && req
                    .expression_attribute_values()
                    .and_then(|v| v.get(":v"))
                    .and_then(|v| v.as_n().ok())
                    .map(String::as_str)
                    == Some(needle.as_str())
        })
        .then_output(|| {
            aws_sdk_dynamodb::operation::update_item::UpdateItemOutput::builder().build()
        })
}

#[tokio::test]
async fn writes_the_next_version_and_records_it() {
    set_env();
    let source = get_record_rule(
        "dev",
        3,
        Some(record_item(&record("dev", 3, &content(3), "schema-admin"))),
    );
    let describe = describe_rule(TARGET_REGISTRY, "2", content(2));
    let write = update_rule("3");
    let record_write = put_record_rule("staging", 3);
    let commit = head_commit_rule(3);
    let clients = clients(&[&source, &record_write, &commit], &[&describe, &write]);

    let out = run(&clients, &replay_input(3)).await.unwrap();

    assert_eq!(out["version"], 3);
    assert_eq!(out["wrote"], true);
    assert_eq!(write.num_calls(), 1, "exactly one replay write");
    assert_eq!(record_write.num_calls(), 1);
    assert_eq!(commit.num_calls(), 1, "progress commits on the head item");
}

#[tokio::test]
async fn first_arrival_writes_version_one() {
    set_env();
    use aws_smithy_mocks::mock;
    let source = get_record_rule(
        "dev",
        1,
        Some(record_item(&record("dev", 1, &content(1), "schema-admin"))),
    );
    let missing = mock!(aws_sdk_schemas::Client::describe_schema).then_error(|| {
        aws_sdk_schemas::operation::describe_schema::DescribeSchemaError::NotFoundException(
            aws_sdk_schemas::types::error::NotFoundException::builder()
                .message("none")
                .build(),
        )
    });
    let write = mock!(aws_sdk_schemas::Client::create_schema)
        .match_requests(|req| req.registry_name() == Some(TARGET_REGISTRY))
        .then_output(|| {
            aws_sdk_schemas::operation::create_schema::CreateSchemaOutput::builder()
                .schema_version("1")
                .build()
        });
    let record_write = put_record_rule("staging", 1);
    let commit = head_commit_rule(1);
    let clients = clients(&[&source, &record_write, &commit], &[&missing, &write]);

    let out = run(&clients, &replay_input(1)).await.unwrap();

    assert_eq!(out["version"], 1);
    assert_eq!(write.num_calls(), 1);
}

#[tokio::test]
async fn first_arrival_create_conflict_is_accepted_when_v1_landed() {
    set_env();
    use aws_smithy_mocks::mock;
    let source = get_record_rule(
        "dev",
        1,
        Some(record_item(&record("dev", 1, &content(1), "schema-admin"))),
    );
    let missing = mock!(aws_sdk_schemas::Client::describe_schema).then_error(|| {
        aws_sdk_schemas::operation::describe_schema::DescribeSchemaError::NotFoundException(
            aws_sdk_schemas::types::error::NotFoundException::builder()
                .message("none")
                .build(),
        )
    });
    let duplicate = mock!(aws_sdk_schemas::Client::create_schema).then_error(|| {
        aws_sdk_schemas::operation::create_schema::CreateSchemaError::BadRequestException(
            aws_sdk_schemas::types::error::BadRequestException::builder()
                .message("Schema already exists")
                .build(),
        )
    });
    let landed = describe_rule(TARGET_REGISTRY, "1", content(1));
    let record_write = put_record_rule("staging", 1);
    let commit = head_commit_rule(1);
    let schemas = aws_smithy_mocks::mock_client!(
        aws_sdk_schemas,
        aws_smithy_mocks::RuleMode::Sequential,
        &[&missing, &duplicate, &landed]
    );
    let clients = Clients {
        ddb: aws_smithy_mocks::mock_client!(
            aws_sdk_dynamodb,
            aws_smithy_mocks::RuleMode::MatchAny,
            &[&source, &record_write, &commit]
        ),
        schemas,
    };

    let out = run(&clients, &replay_input(1)).await.unwrap();

    assert_eq!(out["wrote"], false, "the ambiguous first attempt landed v1");
}

#[tokio::test]
async fn a_latest_of_n_with_equal_content_completes_without_writing() {
    set_env();
    let source = get_record_rule(
        "dev",
        3,
        Some(record_item(&record("dev", 3, &content(3), "schema-admin"))),
    );
    let pretty = "{\n  \"v\": 3\n}".to_string();
    let describe = describe_rule(TARGET_REGISTRY, "3", pretty);
    let record_write = put_record_rule("staging", 3);
    let commit = head_commit_rule(3);
    let clients = clients(&[&source, &record_write, &commit], &[&describe]);

    let out = run(&clients, &replay_input(3)).await.unwrap();

    assert_eq!(out["version"], 3);
    assert_eq!(out["wrote"], false);
    assert_eq!(
        record_write.num_calls(),
        1,
        "the ambiguous write's record completes"
    );
}

#[tokio::test]
async fn a_latest_of_n_with_different_content_fails() {
    set_env();
    let source = get_record_rule(
        "dev",
        3,
        Some(record_item(&record("dev", 3, &content(3), "schema-admin"))),
    );
    let describe = describe_rule(TARGET_REGISTRY, "3", "{\"v\":99}".to_string());
    let clients = clients(&[&source], &[&describe]);

    let result = run(&clients, &replay_input(3)).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn a_no_change_conflict_is_accepted_when_the_version_landed() {
    set_env();
    let source = get_record_rule(
        "dev",
        3,
        Some(record_item(&record("dev", 3, &content(3), "schema-admin"))),
    );
    let before = describe_rule(TARGET_REGISTRY, "2", content(2));
    let conflict = no_change_conflict_rule();
    let after = describe_rule(TARGET_REGISTRY, "3", content(3));
    let record_write = put_record_rule("staging", 3);
    let commit = head_commit_rule(3);
    // The conflict happens between two identical describes; sequencing is
    // the only way to answer v2 then v3.
    let schemas = aws_smithy_mocks::mock_client!(
        aws_sdk_schemas,
        aws_smithy_mocks::RuleMode::Sequential,
        &[&before, &conflict, &after]
    );
    let clients = Clients {
        ddb: aws_smithy_mocks::mock_client!(
            aws_sdk_dynamodb,
            aws_smithy_mocks::RuleMode::MatchAny,
            &[&source, &record_write, &commit]
        ),
        schemas,
    };

    let out = run(&clients, &replay_input(3)).await.unwrap();

    assert_eq!(out["wrote"], false);
}

#[tokio::test]
async fn a_no_change_conflict_without_the_version_fails() {
    set_env();
    let source = get_record_rule(
        "dev",
        3,
        Some(record_item(&record("dev", 3, &content(3), "schema-admin"))),
    );
    let describe = describe_rule(TARGET_REGISTRY, "2", content(2));
    let conflict = no_change_conflict_rule();
    let clients = clients(&[&source], &[&describe, &conflict]);

    let result = run(&clients, &replay_input(3)).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn a_wrong_minted_version_fails() {
    set_env();
    let source = get_record_rule(
        "dev",
        3,
        Some(record_item(&record("dev", 3, &content(3), "schema-admin"))),
    );
    let describe = describe_rule(TARGET_REGISTRY, "2", content(2));
    let write = update_rule("4");
    let clients = clients(&[&source], &[&describe, &write]);

    let result = run(&clients, &replay_input(3)).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn a_gap_fails_instead_of_skipping() {
    set_env();
    let source = get_record_rule(
        "dev",
        3,
        Some(record_item(&record("dev", 3, &content(3), "schema-admin"))),
    );
    let describe = describe_rule(TARGET_REGISTRY, "1", content(1));
    let clients = clients(&[&source], &[&describe]);

    let result = run(&clients, &replay_input(3)).await;

    assert!(result.is_err(), "latest v1 cannot accept v3 directly");
}

#[tokio::test]
async fn a_missing_source_record_fails() {
    set_env();
    let source = get_record_rule("dev", 3, None);
    let clients = clients(&[&source], &[]);

    let result = run(&clients, &replay_input(3)).await;

    assert!(result.is_err());
}
