use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use lambda_runtime::{Error, LambdaEvent};
use nexus_common::api;
use nexus_common::dynamodb::{
    deserialize_pagination_token, get_s, get_string_set, s, serialize_pagination_token,
};
use nexus_common::error::ControlPlaneError;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;

struct QueryResult {
    items: Vec<HashMap<String, AttributeValue>>,
    last_key: Option<HashMap<String, AttributeValue>>,
}

pub async fn handle(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let ddb = aws_sdk_dynamodb::Client::new(&config);
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;

    let owner_filter = api::query_param(&event.payload, "ownerAccountId");
    let status_filter = api::query_param(&event.payload, "status");
    let next_token = api::query_param(&event.payload, "nextToken");
    let max_results = parse_max_results(&event.payload);

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
                    let secondary_owner_emails = get_string_set(item, "secondaryOwnerEmails");
                    json!({
                        "domainName": get_s(item, "domainName"),
                        "description": get_s(item, "description"),
                        "ownerAccountId": get_s(item, "ownerAccountId"),
                        "secondaryOwnerEmails": secondary_owner_emails,
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
            tracing::error!(error = %e, "DynamoDB query failed");
            Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                e.to_string(),
            )))
        }
    }
}

fn parse_max_results(event: &Value) -> i32 {
    api::query_param(event, "maxResults")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
        .clamp(1, 100)
}

async fn query_by_index(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    index: &str,
    key_name: &str,
    key_value: &str,
    limit: i32,
    start_key: Option<HashMap<String, AttributeValue>>,
) -> Result<QueryResult> {
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

    let output = query.send().await.context("failed to query by-index GSI")?;
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
) -> Result<QueryResult> {
    let mut scan = ddb.scan().table_name(table).limit(limit);
    if let Some(key) = start_key {
        for (k, v) in key {
            scan = scan.exclusive_start_key(k, v);
        }
    }
    let output = scan.send().await.context("failed to scan domains table")?;
    Ok(QueryResult {
        items: output.items.unwrap_or_default(),
        last_key: output.last_evaluated_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_param_present_returns_value() {
        let event = json!({"queryStringParameters": {"ownerAccountId": "111122223333"}});
        assert_eq!(
            api::query_param(&event, "ownerAccountId"),
            Some("111122223333".to_string())
        );
    }

    #[test]
    fn query_param_absent_returns_none() {
        let event = json!({});
        assert_eq!(api::query_param(&event, "ownerAccountId"), None);
        assert_eq!(api::query_param(&event, "status"), None);
        assert_eq!(api::query_param(&event, "nextToken"), None);
    }

    #[test]
    fn max_results_valid_value_is_used() {
        let event = json!({"queryStringParameters": {"maxResults": "50"}});
        assert_eq!(parse_max_results(&event), 50);
    }

    #[test]
    fn max_results_invalid_falls_back_to_default() {
        let event = json!({"queryStringParameters": {"maxResults": "abc"}});
        assert_eq!(parse_max_results(&event), 100);
    }

    #[test]
    fn max_results_negative_is_clamped_to_one() {
        let event = json!({"queryStringParameters": {"maxResults": "-5"}});
        assert_eq!(parse_max_results(&event), 1);
    }

    #[test]
    fn max_results_over_100_is_clamped_to_100() {
        let event = json!({"queryStringParameters": {"maxResults": "500"}});
        assert_eq!(parse_max_results(&event), 100);
    }

    #[test]
    fn pagination_token_round_trips() {
        let mut key = HashMap::new();
        key.insert("domainName".to_string(), s("billing"));
        key.insert("status".to_string(), s("ACTIVE"));

        let token = serialize_pagination_token(&key);
        let restored = deserialize_pagination_token(&token).unwrap();

        assert_eq!(
            restored.get("domainName").unwrap().as_s().unwrap(),
            "billing"
        );
        assert_eq!(restored.get("status").unwrap().as_s().unwrap(), "ACTIVE");
    }
}
