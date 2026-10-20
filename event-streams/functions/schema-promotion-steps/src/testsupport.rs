//! Mock fixtures shared by the step tests.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use aws_smithy_mocks::{mock, Rule};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use konductor_common::dynamodb::s;
use konductor_common::promotion::{
    canonical_content_hash, promotion_pk, version_sk, SchemaVersionContent,
};
use konductor_common::promotion_store::version_content_to_item;

use crate::store::Clients;

pub const TARGET_REGISTRY: &str = "acme-staging-orders-schema-registry";
pub const FULL_SCHEMA: &str = "acme-orders@OrderNotification";

pub fn set_env() {
    std::env::set_var("ORGANIZATION", "acme");
    std::env::set_var("SCHEMA_VERSIONS_TABLE", "versions-test");
    std::env::set_var("PROMOTIONS_TABLE", "promotions-test");
    std::env::set_var("EVENT_STREAMS_TABLE", "streams-test");
    std::env::set_var("ENVIRONMENTS", "dev,staging,prod");
}

pub fn input(action: &str) -> Value {
    json!({
        "action": action,
        "promotionId": "promo-1",
        "domain": "orders",
        "schemaName": "OrderNotification",
        "sourceEnvironment": "dev",
        "targetEnvironment": "staging",
        "requestedVersion": 3,
    })
}

pub fn at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
        .unwrap()
        .into()
}

pub fn record(
    environment: &str,
    version: i64,
    content: &str,
    written_by: &str,
) -> SchemaVersionContent {
    SchemaVersionContent {
        domain: "orders".to_string(),
        schema_name: "OrderNotification".to_string(),
        environment: environment.to_string(),
        version,
        content: content.to_string(),
        content_hash: canonical_content_hash(content).unwrap(),
        written_by: written_by.to_string(),
        written_at: at(),
    }
}

pub fn record_item(r: &SchemaVersionContent) -> HashMap<String, AttributeValue> {
    let mut item = version_content_to_item(r);
    item.insert(
        "pk".to_string(),
        s(&promotion_pk(&r.domain, &r.schema_name, &r.environment)),
    );
    item.insert("sk".to_string(), s(&version_sk(r.version)));
    item
}

pub fn get_record_rule(
    environment: &'static str,
    version: i64,
    item: Option<HashMap<String, AttributeValue>>,
) -> Rule {
    let pk = promotion_pk("orders", "OrderNotification", environment);
    let sk = version_sk(version);
    let rule = mock!(aws_sdk_dynamodb::Client::get_item).match_requests(move |req| {
        req.table_name() == Some("versions-test")
            && req
                .key()
                .and_then(|k| k.get("pk"))
                .and_then(|v| v.as_s().ok())
                .map(String::as_str)
                == Some(pk.as_str())
            && req
                .key()
                .and_then(|k| k.get("sk"))
                .and_then(|v| v.as_s().ok())
                .map(String::as_str)
                == Some(sk.as_str())
    });
    rule.then_output(move || {
        let builder = aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder();
        match &item {
            Some(item) => builder.set_item(Some(item.clone())).build(),
            None => builder.build(),
        }
    })
}

pub fn list_versions_rule(registry: &'static str, versions: &'static [&'static str]) -> Rule {
    mock!(aws_sdk_schemas::Client::list_schema_versions)
        .match_requests(move |req| {
            req.registry_name() == Some(registry) && req.schema_name() == Some(FULL_SCHEMA)
        })
        .then_output(move || {
            let entries: Vec<_> = versions
                .iter()
                .map(|v| {
                    aws_sdk_schemas::types::SchemaVersionSummary::builder()
                        .schema_version(*v)
                        .build()
                })
                .collect();
            aws_sdk_schemas::operation::list_schema_versions::ListSchemaVersionsOutput::builder()
                .set_schema_versions(Some(entries))
                .build()
        })
}

pub fn list_not_found_rule(registry: &'static str) -> Rule {
    mock!(aws_sdk_schemas::Client::list_schema_versions)
        .match_requests(move |req| req.registry_name() == Some(registry))
        .then_error(|| {
            aws_sdk_schemas::operation::list_schema_versions::ListSchemaVersionsError::NotFoundException(
                aws_sdk_schemas::types::error::NotFoundException::builder()
                    .message("no schema")
                    .build(),
            )
        })
}

pub fn describe_rule(registry: &'static str, version: &'static str, content: String) -> Rule {
    mock!(aws_sdk_schemas::Client::describe_schema)
        .match_requests(move |req| req.registry_name() == Some(registry))
        .then_output(move || {
            aws_sdk_schemas::operation::describe_schema::DescribeSchemaOutput::builder()
                .schema_version(version)
                .content(&content)
                .build()
        })
}

pub fn put_record_rule(environment: &'static str, version: i64) -> Rule {
    let pk = promotion_pk("orders", "OrderNotification", environment);
    let sk = version_sk(version);
    mock!(aws_sdk_dynamodb::Client::put_item)
        .match_requests(move |req| {
            req.table_name() == Some("versions-test")
                && req
                    .item()
                    .and_then(|i| i.get("pk"))
                    .and_then(|v| v.as_s().ok())
                    .map(String::as_str)
                    == Some(pk.as_str())
                && req
                    .item()
                    .and_then(|i| i.get("sk"))
                    .and_then(|v| v.as_s().ok())
                    .map(String::as_str)
                    == Some(sk.as_str())
        })
        .then_output(|| aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build())
}

pub fn clients(ddb_rules: &[&Rule], schemas_rules: &[&Rule]) -> Clients {
    use aws_smithy_mocks::{mock_client, RuleMode};
    Clients {
        ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
        schemas: mock_client!(aws_sdk_schemas, RuleMode::MatchAny, schemas_rules),
    }
}
