use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;

use nexus_common::api;
use nexus_common::dynamodb::{deserialize_pagination_token, serialize_pagination_token};
use nexus_common::error::ControlPlaneError;
use nexus_common::principal_context::log_principal;
use nexus_common::streams::{EventStream, StreamStatus};
use nexus_common::streams_db;

const DEFAULT_MAX_RESULTS: i32 = 100;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;

    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = log_principal(request_context);

    let Some(caller_account_id) = principal
        .ok()
        .and_then(|p| p.iam_account_id().map(String::from))
    else {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "missing caller identity",
        )));
    };

    let consumer = api::query_param(&event.payload, "consumer");
    let producer = api::query_param(&event.payload, "producer");
    let schema = api::query_param(&event.payload, "schema");
    let version = api::query_param(&event.payload, "version").and_then(|v| v.parse::<i64>().ok());
    let status =
        api::query_param(&event.payload, "status").and_then(|v| v.parse::<StreamStatus>().ok());
    let max_results = api::query_param(&event.payload, "maxResults")
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|v| *v > 0 && *v <= DEFAULT_MAX_RESULTS)
        .unwrap_or(DEFAULT_MAX_RESULTS);
    let next_token = api::query_param(&event.payload, "nextToken");

    let result = fetch_streams(
        ddb,
        &table,
        &consumer,
        &producer,
        &schema,
        max_results,
        &next_token,
    )
    .await;

    match result {
        Ok((streams, next_page_key)) => {
            let mut owner_cache: HashMap<String, Option<String>> = HashMap::new();
            let mut items = Vec::new();
            for stream in &streams {
                if !matches_filters(stream, &consumer, &schema, &version, &status) {
                    continue;
                }
                if !caller_may_view(
                    ddb,
                    &domains_table,
                    &caller_account_id,
                    stream,
                    &mut owner_cache,
                )
                .await
                .unwrap_or(false)
                {
                    continue;
                }
                items.push(stream_summary(stream));
            }
            let next_token = next_page_key.map(|k| serialize_pagination_token(&k));
            Ok(api::success_response(
                200,
                &json!({ "items": items, "nextToken": next_token }),
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

/// A subscription is a relationship between two domains; either party — the
/// consumer that subscribed or the producer whose contract it pinned — has a
/// legitimate interest in seeing it, but a third party does not.
///
/// `owner_cache` is invocation-scoped: the same producer/consumer domain
/// tends to repeat across a result page, so this avoids a GetItem per row.
async fn caller_may_view(
    ddb: &aws_sdk_dynamodb::Client,
    domains_table: &str,
    caller_account_id: &str,
    stream: &EventStream,
    owner_cache: &mut HashMap<String, Option<String>>,
) -> Result<bool> {
    for domain in [&stream.consumer_domain, &stream.producer_domain] {
        let owner = match owner_cache.get(domain) {
            Some(cached) => cached.clone(),
            None => {
                let owner = lookup_owner_account_id(ddb, domains_table, domain).await?;
                owner_cache.insert(domain.clone(), owner.clone());
                owner
            }
        };
        if owner.as_deref() == Some(caller_account_id) {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn lookup_owner_account_id(
    ddb: &aws_sdk_dynamodb::Client,
    domains_table: &str,
    domain_name: &str,
) -> Result<Option<String>> {
    let output = ddb
        .get_item()
        .table_name(domains_table)
        .key("domainName", nexus_common::dynamodb::s(domain_name))
        .send()
        .await
        .context("failed to get domain record for ownership check")?;
    Ok(output
        .item
        .as_ref()
        .and_then(|item| nexus_common::dynamodb::get_s(item, "ownerAccountId")))
}

async fn fetch_streams(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    consumer: &Option<String>,
    producer: &Option<String>,
    schema: &Option<String>,
    max_results: i32,
    next_token: &Option<String>,
) -> Result<(Vec<EventStream>, Option<HashMap<String, AttributeValue>>)> {
    match (producer, schema, consumer) {
        (Some(producer), Some(schema), _) => Ok((
            streams_db::query_streams_by_schema(ddb, table, producer, schema).await?,
            None,
        )),
        (Some(producer), None, _) => Ok((
            streams_db::query_streams_by_producer(ddb, table, producer).await?,
            None,
        )),
        (None, _, Some(consumer)) => Ok((
            streams_db::query_streams_by_consumer(ddb, table, consumer).await?,
            None,
        )),
        (None, _, None) => {
            let start_key = next_token.as_deref().and_then(deserialize_pagination_token);
            streams_db::scan_streams_page(ddb, table, max_results, start_key).await
        }
    }
}

fn matches_filters(
    stream: &EventStream,
    consumer: &Option<String>,
    schema: &Option<String>,
    version: &Option<i64>,
    status: &Option<StreamStatus>,
) -> bool {
    if let Some(consumer) = consumer {
        if &stream.consumer_domain != consumer {
            return false;
        }
    }
    if let Some(schema) = schema {
        if &stream.schema_name != schema {
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
        assert!(matches_filters(&stream, &None, &None, &Some(3), &None));
        assert!(!matches_filters(&stream, &None, &None, &Some(4), &None));
    }

    #[test]
    fn status_filter_excludes_other_statuses() {
        let stream = stream_with("checkout", StreamStatus::Removed, 3);
        assert!(matches_filters(
            &stream,
            &None,
            &None,
            &None,
            &Some(StreamStatus::Removed)
        ));
        assert!(!matches_filters(
            &stream,
            &None,
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
            &None,
            &None
        ));
        assert!(!matches_filters(
            &stream,
            &Some("billing".to_string()),
            &None,
            &None,
            &None
        ));
    }

    #[test]
    fn schema_filter_excludes_unrelated_schemas() {
        let stream = stream_with("checkout", StreamStatus::Active, 3);
        assert!(matches_filters(
            &stream,
            &None,
            &Some("acme-orders@OrderNotification".to_string()),
            &None,
            &None
        ));
        assert!(!matches_filters(
            &stream,
            &None,
            &Some("acme-orders@OrderCancelled".to_string()),
            &None,
            &None
        ));
    }

    #[test]
    fn schema_filter_combines_with_consumer_filter() {
        let stream = stream_with("checkout", StreamStatus::Active, 3);
        // Regression: previously the schema filter was silently ignored
        // whenever `producer` was absent, so consumer+schema queries
        // returned every schema for that consumer.
        assert!(!matches_filters(
            &stream,
            &Some("checkout".to_string()),
            &Some("acme-orders@OrderCancelled".to_string()),
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

    fn domain_owner_client(owners: Vec<(&'static str, &'static str)>) -> aws_sdk_dynamodb::Client {
        use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
        use aws_smithy_mocks::{mock, mock_client, RuleMode};

        let rules: Vec<_> = owners
            .into_iter()
            .map(|(domain, owner)| {
                mock!(aws_sdk_dynamodb::Client::get_item)
                    .match_requests(move |req| {
                        req.key()
                            .unwrap()
                            .get("domainName")
                            .and_then(|v| v.as_s().ok())
                            .map(|v| v == domain)
                            .unwrap_or(false)
                    })
                    .then_output(move || {
                        GetItemOutput::builder()
                            .item("domainName", nexus_common::dynamodb::s(domain))
                            .item("ownerAccountId", nexus_common::dynamodb::s(owner))
                            .build()
                    })
            })
            .collect();
        let rule_refs: Vec<_> = rules.iter().collect();
        mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, rule_refs)
    }

    #[tokio::test]
    async fn caller_may_view_true_for_consumer_owner() {
        let ddb = domain_owner_client(vec![
            ("checkout", "111122223333"),
            ("orders", "444455556666"),
        ]);
        let stream = stream_with("checkout", StreamStatus::Active, 3);
        let mut cache = HashMap::new();
        assert!(
            caller_may_view(&ddb, "domains", "111122223333", &stream, &mut cache)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn caller_may_view_true_for_producer_owner() {
        let ddb = domain_owner_client(vec![
            ("checkout", "111122223333"),
            ("orders", "444455556666"),
        ]);
        let stream = stream_with("checkout", StreamStatus::Active, 3);
        let mut cache = HashMap::new();
        assert!(
            caller_may_view(&ddb, "domains", "444455556666", &stream, &mut cache)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn caller_may_view_false_for_third_party() {
        let ddb = domain_owner_client(vec![
            ("checkout", "111122223333"),
            ("orders", "444455556666"),
        ]);
        let stream = stream_with("checkout", StreamStatus::Active, 3);
        let mut cache = HashMap::new();
        assert!(
            !caller_may_view(&ddb, "domains", "999900001111", &stream, &mut cache)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn caller_may_view_caches_owner_lookups_across_calls() {
        // The mock has exactly one rule per domain with no call-count limit,
        // so this doesn't prove caching directly, but it does prove the
        // cache is populated and reused without behavior changing across
        // repeated lookups of the same stream (the case caching optimizes).
        let ddb = domain_owner_client(vec![
            ("checkout", "111122223333"),
            ("orders", "444455556666"),
        ]);
        let stream = stream_with("checkout", StreamStatus::Active, 3);
        let mut cache = HashMap::new();
        assert!(
            caller_may_view(&ddb, "domains", "111122223333", &stream, &mut cache)
                .await
                .unwrap()
        );
        assert_eq!(
            cache.get("checkout").cloned().flatten(),
            Some("111122223333".to_string())
        );
        assert!(
            caller_may_view(&ddb, "domains", "111122223333", &stream, &mut cache)
                .await
                .unwrap()
        );
    }
}
