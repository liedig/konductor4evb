use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;

use control_plane_common::api;
use control_plane_common::dynamodb::{deserialize_pagination_token, get_s, s, serialize_pagination_token};

struct QueryResult {
    items: Vec<HashMap<String, AttributeValue>>,
    last_key: Option<HashMap<String, AttributeValue>>,
}

async fn handler(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let ddb = aws_sdk_dynamodb::Client::new(&config);
    let table = env::var("REGISTRATION_REQUESTS_TABLE")
        .context("REGISTRATION_REQUESTS_TABLE not set")?;

    let status_filter = api::query_param(&event.payload, "status");
    let next_token = api::query_param(&event.payload, "nextToken");
    let max_results: i32 = api::query_param(&event.payload, "maxResults")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
        .min(100);

    let start_key = next_token.as_deref().and_then(deserialize_pagination_token);

    let result = if let Some(status) = &status_filter {
        let mut query = ddb
            .query()
            .table_name(&table)
            .index_name("by-status")
            .key_condition_expression("#s = :status")
            .expression_attribute_names("#s", "status")
            .expression_attribute_values(":status", s(status))
            .limit(max_results);

        if let Some(key) = start_key {
            for (k, v) in key {
                query = query.exclusive_start_key(k, v);
            }
        }

        let output = query.send().await?;
        QueryResult {
            items: output.items.unwrap_or_default(),
            last_key: output.last_evaluated_key,
        }
    } else {
        let mut scan = ddb
            .scan()
            .table_name(&table)
            .filter_expression("#s <> :active")
            .expression_attribute_names("#s", "status")
            .expression_attribute_values(":active", s("ACTIVE"))
            .limit(max_results);

        if let Some(key) = start_key {
            for (k, v) in key {
                scan = scan.exclusive_start_key(k, v);
            }
        }

        let output = scan.send().await?;
        QueryResult {
            items: output.items.unwrap_or_default(),
            last_key: output.last_evaluated_key,
        }
    };

    let items: Vec<Value> = result.items.iter().map(|item| {
        json!({
            "requestId": get_s(item, "requestId"),
            "domainId": get_s(item, "domainId"),
            "domainName": get_s(item, "domainName"),
            "ownerAccountId": get_s(item, "ownerAccountId"),
            "status": get_s(item, "status"),
            "requestedAt": get_s(item, "requestedAt"),
            "requestedBy": get_s(item, "requestedBy"),
        })
    }).collect();

    let pagination_token = result.last_key.as_ref().map(serialize_pagination_token);

    Ok(api::success_response(200, &json!({
        "items": items,
        "nextToken": pagination_token,
    })))
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    lambda_runtime::run(service_fn(handler)).await
}
