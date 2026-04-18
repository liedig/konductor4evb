use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;

use nexus_common::api;
use nexus_common::dynamodb::{deserialize_pagination_token, get_s, s, serialize_pagination_token};

struct QueryResult {
    items: Vec<HashMap<String, AttributeValue>>,
    last_key: Option<HashMap<String, AttributeValue>>,
}

async fn handler(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let ddb = aws_sdk_dynamodb::Client::new(&config);
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;

    let owner_filter = api::query_param(&event.payload, "ownerAccountId");
    let status_filter = api::query_param(&event.payload, "status");
    let next_token = api::query_param(&event.payload, "nextToken");
    let max_results: i32 = api::query_param(&event.payload, "maxResults")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
        .min(100);

    let start_key = next_token.as_deref().and_then(deserialize_pagination_token);

    let result = if let Some(owner) = &owner_filter {
        query_by_index(
            &ddb,
            &table,
            "by-owner",
            "ownerAccountId",
            owner,
            max_results,
            start_key,
        )
        .await
    } else if let Some(status) = &status_filter {
        query_by_index(
            &ddb,
            &table,
            "by-status",
            "status",
            status,
            max_results,
            start_key,
        )
        .await
    } else {
        scan_all(&ddb, &table, max_results, start_key).await
    };

    match result {
        Ok(qr) => {
            let items: Vec<Value> = qr
                .items
                .iter()
                .map(|item| {
                    json!({
                        "domainId": get_s(item, "domainId"),
                        "domainName": get_s(item, "domainName"),
                        "description": get_s(item, "description"),
                        "ownerAccountId": get_s(item, "ownerAccountId"),
                        "status": get_s(item, "status"),
                        "createdAt": get_s(item, "createdAt"),
                    })
                })
                .collect();

            let pagination_token = qr.last_key.as_ref().map(serialize_pagination_token);

            Ok(api::success_response(
                200,
                &json!({
                    "items": items,
                    "nextToken": pagination_token,
                }),
            ))
        }
        Err(e) => {
            tracing::error!("DynamoDB error: {e:?}");
            Ok(api::error_response(
                500,
                nexus_common::models::ApiError {
                    code: "InternalServerException".to_string(),
                    message: "Failed to list domains".to_string(),
                    request_id: None,
                    reason: None,
                    field_list: None,
                },
            ))
        }
    }
}

async fn query_by_index(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    index: &str,
    key_name: &str,
    key_value: &str,
    limit: i32,
    start_key: Option<HashMap<String, AttributeValue>>,
) -> anyhow::Result<QueryResult> {
    let is_reserved = key_name == "status";
    let key_expr = if is_reserved {
        "#k = :val".to_string()
    } else {
        format!("{key_name} = :val")
    };

    let mut query = ddb
        .query()
        .table_name(table)
        .index_name(index)
        .key_condition_expression(&key_expr)
        .expression_attribute_values(":val", s(key_value))
        .limit(limit);

    if is_reserved {
        query = query.expression_attribute_names("#k", key_name);
    }
    if let Some(key) = start_key {
        for (k, v) in key {
            query = query.exclusive_start_key(k, v);
        }
    }

    let output = query.send().await?;
    Ok(QueryResult {
        items: output.items.unwrap_or_default(),
        last_key: output.last_evaluated_key,
    })
}

async fn scan_all(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    limit: i32,
    start_key: Option<HashMap<String, AttributeValue>>,
) -> anyhow::Result<QueryResult> {
    let mut scan = ddb.scan().table_name(table).limit(limit);
    if let Some(key) = start_key {
        for (k, v) in key {
            scan = scan.exclusive_start_key(k, v);
        }
    }
    let output = scan.send().await?;
    Ok(QueryResult {
        items: output.items.unwrap_or_default(),
        last_key: output.last_evaluated_key,
    })
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    lambda_runtime::run(service_fn(handler)).await
}
