use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};
use std::env;

use nexus_common::api;
use nexus_common::error::ControlPlaneError;
use nexus_common::streams::{EventStream, StreamStatus};
use nexus_common::streams_db;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;

    let consumer = api::query_param(&event.payload, "consumer");
    let producer = api::query_param(&event.payload, "producer");
    let schema = api::query_param(&event.payload, "schema");
    let version = api::query_param(&event.payload, "version").and_then(|v| v.parse::<i64>().ok());
    let status =
        api::query_param(&event.payload, "status").and_then(|v| v.parse::<StreamStatus>().ok());

    let result = fetch_streams(ddb, &table, &consumer, &producer, &schema).await;

    match result {
        Ok(streams) => {
            let items: Vec<Value> = streams
                .iter()
                .filter(|stream| matches_filters(stream, &consumer, &version, &status))
                .map(stream_summary)
                .collect();
            Ok(api::success_response(
                200,
                &json!({ "items": items, "nextToken": null }),
            ))
        }
        Err(e) => {
            tracing::error!(error = %e, "event streams query failed");
            Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                e.to_string(),
            )))
        }
    }
}

async fn fetch_streams(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    consumer: &Option<String>,
    producer: &Option<String>,
    schema: &Option<String>,
) -> Result<Vec<EventStream>> {
    match (producer, schema, consumer) {
        (Some(producer), Some(schema), _) => {
            streams_db::query_streams_by_schema(ddb, table, producer, schema).await
        }
        (Some(producer), None, _) => {
            streams_db::query_streams_by_producer(ddb, table, producer).await
        }
        (None, _, Some(consumer)) => {
            streams_db::query_streams_by_consumer(ddb, table, consumer).await
        }
        (None, _, None) => scan_all(ddb, table).await,
    }
}

async fn scan_all(ddb: &aws_sdk_dynamodb::Client, table: &str) -> Result<Vec<EventStream>> {
    let output = ddb
        .scan()
        .table_name(table)
        .send()
        .await
        .context("failed to scan event streams table")?;
    Ok(output
        .items
        .unwrap_or_default()
        .iter()
        .filter_map(streams_db::item_to_stream)
        .collect())
}

fn matches_filters(
    stream: &EventStream,
    consumer: &Option<String>,
    version: &Option<i64>,
    status: &Option<StreamStatus>,
) -> bool {
    if let Some(consumer) = consumer {
        if &stream.consumer_domain != consumer {
            return false;
        }
    }
    if let Some(status) = status {
        if &stream.status != status {
            return false;
        }
    }
    if let Some(version) = version {
        let pinned_anywhere = stream
            .environments
            .values()
            .any(|env| env.pinned_version == Some(*version));
        if !pinned_anywhere {
            return false;
        }
    }
    true
}

fn stream_summary(stream: &EventStream) -> Value {
    json!({
        "subscriptionId": stream.subscription_id().to_string(),
        "consumerDomain": stream.consumer_domain,
        "producerDomain": stream.producer_domain,
        "schemaName": stream.schema_name,
        "status": stream.status,
        "environments": stream.environments,
        "createdAt": stream.created_at.to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use nexus_common::streams::EnvStream;
    use std::collections::HashMap;

    fn stream_with(consumer: &str, status: StreamStatus, prod_version: i64) -> EventStream {
        let mut environments = HashMap::new();
        environments.insert(
            "prod".to_string(),
            EnvStream {
                pinned_version: Some(prod_version),
                rule_name: format!("{consumer}-OrderNotification"),
                rule_arn: None,
                enabled: true,
                status: StreamStatus::Active,
            },
        );
        EventStream {
            producer_domain: "orders".to_string(),
            consumer_domain: consumer.to_string(),
            source: "acme-orders".to_string(),
            detail_type: "OrderNotification".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            status,
            environments,
            request_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
            expires_at: None,
        }
    }

    #[test]
    fn version_filter_matches_any_environment_pin() {
        let stream = stream_with("checkout", StreamStatus::Active, 3);
        assert!(matches_filters(&stream, &None, &Some(3), &None));
        assert!(!matches_filters(&stream, &None, &Some(4), &None));
    }

    #[test]
    fn status_filter_excludes_other_statuses() {
        let stream = stream_with("checkout", StreamStatus::Removed, 3);
        assert!(matches_filters(
            &stream,
            &None,
            &None,
            &Some(StreamStatus::Removed)
        ));
        assert!(!matches_filters(
            &stream,
            &None,
            &None,
            &Some(StreamStatus::Active)
        ));
    }

    #[test]
    fn consumer_filter_applies_after_producer_query() {
        let stream = stream_with("checkout", StreamStatus::Active, 3);
        assert!(matches_filters(
            &stream,
            &Some("checkout".to_string()),
            &None,
            &None
        ));
        assert!(!matches_filters(
            &stream,
            &Some("billing".to_string()),
            &None,
            &None
        ));
    }

    #[test]
    fn summary_exposes_subscription_id() {
        let stream = stream_with("checkout", StreamStatus::Active, 3);
        let summary = stream_summary(&stream);
        assert_eq!(
            summary["subscriptionId"],
            "checkout.orders.OrderNotification"
        );
        assert_eq!(summary["environments"]["prod"]["pinnedVersion"], 3);
    }
}
