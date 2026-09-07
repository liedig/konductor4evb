use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

use crate::dynamodb::{get_s, s};
use crate::streams::{EnvStream, EventStream, StreamStatus, REMOVED_RETENTION_DAYS};

pub const PK: &str = "producerDomain";
pub const SK: &str = "schemaConsumer";
pub const BY_CONSUMER_INDEX: &str = "by-consumer";

fn n(val: i64) -> AttributeValue {
    AttributeValue::N(val.to_string())
}

fn get_n(item: &HashMap<String, AttributeValue>, key: &str) -> Option<i64> {
    item.get(key)
        .and_then(|v| v.as_n().ok())
        .and_then(|v| v.parse().ok())
}

fn get_bool(item: &HashMap<String, AttributeValue>, key: &str) -> Option<bool> {
    item.get(key).and_then(|v| v.as_bool().ok()).copied()
}

fn get_dt(item: &HashMap<String, AttributeValue>, key: &str) -> Option<DateTime<Utc>> {
    get_s(item, key).and_then(|v| v.parse().ok())
}

pub fn env_stream_to_attr(env: &EnvStream) -> AttributeValue {
    let mut map = HashMap::new();
    if let Some(version) = env.pinned_version {
        map.insert("pinnedVersion".to_string(), n(version));
    }
    map.insert("ruleName".to_string(), s(&env.rule_name));
    if let Some(arn) = &env.rule_arn {
        map.insert("ruleArn".to_string(), s(arn));
    }
    map.insert("enabled".to_string(), AttributeValue::Bool(env.enabled));
    map.insert("status".to_string(), s(env.status.as_str()));
    if let Some(detail) = &env.error_detail {
        map.insert("errorDetail".to_string(), s(detail));
    }
    AttributeValue::M(map)
}

fn attr_to_env_stream(inner: &HashMap<String, AttributeValue>) -> Option<EnvStream> {
    Some(EnvStream {
        pinned_version: get_n(inner, "pinnedVersion"),
        rule_name: get_s(inner, "ruleName")?,
        rule_arn: get_s(inner, "ruleArn"),
        enabled: get_bool(inner, "enabled")?,
        status: get_s(inner, "status")?.parse().ok()?,
        error_detail: get_s(inner, "errorDetail"),
    })
}

pub fn stream_to_item(stream: &EventStream) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    item.insert(PK.to_string(), s(&stream.producer_domain));
    item.insert(
        SK.to_string(),
        s(&EventStream::sort_key(
            &stream.schema_name,
            &stream.consumer_domain,
        )),
    );
    item.insert("consumerDomain".to_string(), s(&stream.consumer_domain));
    item.insert("source".to_string(), s(&stream.source));
    item.insert("detailType".to_string(), s(&stream.detail_type));
    item.insert("schemaName".to_string(), s(&stream.schema_name));
    item.insert("status".to_string(), s(stream.status.as_str()));
    if let Some(detail) = &stream.error_detail {
        item.insert("errorDetail".to_string(), s(detail));
    }
    let envs: HashMap<String, AttributeValue> = stream
        .environments
        .iter()
        .map(|(env, es)| (env.clone(), env_stream_to_attr(es)))
        .collect();
    item.insert("environments".to_string(), AttributeValue::M(envs));
    if let Some(request_id) = &stream.request_id {
        item.insert("requestId".to_string(), s(request_id));
    }
    item.insert("createdAt".to_string(), s(&stream.created_at.to_rfc3339()));
    item.insert("updatedAt".to_string(), s(&stream.updated_at.to_rfc3339()));
    if let Some(removed_at) = &stream.removed_at {
        item.insert("removedAt".to_string(), s(&removed_at.to_rfc3339()));
    }
    if let Some(expires_at) = stream.expires_at {
        item.insert("expiresAt".to_string(), n(expires_at));
    }
    item
}

pub fn item_to_stream(item: &HashMap<String, AttributeValue>) -> Option<EventStream> {
    let environments = item
        .get("environments")
        .and_then(|v| v.as_m().ok())
        .map(|m| {
            m.iter()
                .filter_map(|(env, av)| {
                    let inner = av.as_m().ok()?;
                    Some((env.clone(), attr_to_env_stream(inner)?))
                })
                .collect()
        })
        .unwrap_or_default();

    Some(EventStream {
        producer_domain: get_s(item, PK)?,
        consumer_domain: get_s(item, "consumerDomain")?,
        source: get_s(item, "source")?,
        detail_type: get_s(item, "detailType")?,
        schema_name: get_s(item, "schemaName")?,
        status: get_s(item, "status")?.parse().ok()?,
        error_detail: get_s(item, "errorDetail"),
        environments,
        request_id: get_s(item, "requestId"),
        created_at: get_dt(item, "createdAt")?,
        updated_at: get_dt(item, "updatedAt")?,
        removed_at: get_dt(item, "removedAt"),
        expires_at: get_n(item, "expiresAt"),
    })
}

pub async fn put_stream(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    stream: &EventStream,
) -> Result<()> {
    ddb.put_item()
        .table_name(table)
        .set_item(Some(stream_to_item(stream)))
        .send()
        .await
        .context("failed to put event stream record")?;
    Ok(())
}

/// Presence-scoped per-env merge: writes only `environments.<env>` plus
/// `updatedAt`, so environments materialized concurrently by separate
/// workflow map iterations never clobber each other. The entry replaces the
/// whole sub-map, clearing any stale `errorDetail` on success.
pub async fn merge_env_stream(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    producer_domain: &str,
    schema_name: &str,
    consumer_domain: &str,
    environment: &str,
    entry: &EnvStream,
) -> Result<()> {
    ddb.update_item()
        .table_name(table)
        .key(PK, s(producer_domain))
        .key(SK, s(&EventStream::sort_key(schema_name, consumer_domain)))
        .update_expression("SET environments.#env = :entry, updatedAt = :now")
        .expression_attribute_names("#env", environment)
        .expression_attribute_values(":entry", env_stream_to_attr(entry))
        .expression_attribute_values(":now", s(&Utc::now().to_rfc3339()))
        .send()
        .await
        .context("failed to merge per-environment stream entry")?;
    Ok(())
}

pub async fn get_stream(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    producer_domain: &str,
    schema_name: &str,
    consumer_domain: &str,
) -> Result<Option<EventStream>> {
    let output = ddb
        .get_item()
        .table_name(table)
        .key(PK, s(producer_domain))
        .key(SK, s(&EventStream::sort_key(schema_name, consumer_domain)))
        .send()
        .await
        .context("failed to get event stream record")?;
    Ok(output.item.as_ref().and_then(item_to_stream))
}

/// Version census: every subscriber of a schema, with per-env pinned
/// versions, in one logical call — no EventBridge calls. Pages internally
/// past DynamoDB's 1MB-per-`Query` limit so callers always see every match.
pub async fn query_streams_by_schema(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    producer_domain: &str,
    schema_name: &str,
) -> Result<Vec<EventStream>> {
    let prefix = format!("{schema_name}#");
    query_all_pages(|exclusive_start_key| {
        ddb.query()
            .table_name(table)
            .key_condition_expression("#pk = :producer AND begins_with(#sk, :prefix)")
            .expression_attribute_names("#pk", PK)
            .expression_attribute_names("#sk", SK)
            .expression_attribute_values(":producer", s(producer_domain))
            .expression_attribute_values(":prefix", s(&prefix))
            .set_exclusive_start_key(exclusive_start_key)
    })
    .await
    .context("failed to query streams by schema")
}

/// Revocation-candidate census for prefix-pattern revocations: every
/// subscription in a producer domain whose schema name starts with `prefix`.
pub async fn query_streams_by_schema_prefix(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    producer_domain: &str,
    schema_name_prefix: &str,
) -> Result<Vec<EventStream>> {
    query_all_pages(|exclusive_start_key| {
        ddb.query()
            .table_name(table)
            .key_condition_expression("#pk = :producer AND begins_with(#sk, :prefix)")
            .expression_attribute_names("#pk", PK)
            .expression_attribute_names("#sk", SK)
            .expression_attribute_values(":producer", s(producer_domain))
            .expression_attribute_values(":prefix", s(schema_name_prefix))
            .set_exclusive_start_key(exclusive_start_key)
    })
    .await
    .context("failed to query streams by schema-name prefix")
}

pub async fn query_streams_by_producer(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    producer_domain: &str,
) -> Result<Vec<EventStream>> {
    query_all_pages(|exclusive_start_key| {
        ddb.query()
            .table_name(table)
            .key_condition_expression("#pk = :producer")
            .expression_attribute_names("#pk", PK)
            .expression_attribute_values(":producer", s(producer_domain))
            .set_exclusive_start_key(exclusive_start_key)
    })
    .await
    .context("failed to query streams by producer")
}

pub async fn query_streams_by_consumer(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    consumer_domain: &str,
) -> Result<Vec<EventStream>> {
    query_all_pages(|exclusive_start_key| {
        ddb.query()
            .table_name(table)
            .index_name(BY_CONSUMER_INDEX)
            .key_condition_expression("consumerDomain = :consumer")
            .expression_attribute_values(":consumer", s(consumer_domain))
            .set_exclusive_start_key(exclusive_start_key)
    })
    .await
    .context("failed to query streams by consumer")
}

/// Runs `build_query` repeatedly, feeding each page's `LastEvaluatedKey` back
/// in as `ExclusiveStartKey`, until DynamoDB reports no further pages.
/// The error is boxed: SdkError is ~400 bytes, tripping clippy::result_large_err.
async fn query_all_pages<F>(
    build_query: F,
) -> std::result::Result<
    Vec<EventStream>,
    Box<aws_sdk_dynamodb::error::SdkError<aws_sdk_dynamodb::operation::query::QueryError>>,
>
where
    F: Fn(
        Option<HashMap<String, AttributeValue>>,
    ) -> aws_sdk_dynamodb::operation::query::builders::QueryFluentBuilder,
{
    let mut items = Vec::new();
    let mut exclusive_start_key = None;
    loop {
        let output = build_query(exclusive_start_key).send().await?;
        items.extend(collect_streams(output.items));
        exclusive_start_key = output.last_evaluated_key;
        if exclusive_start_key.is_none() {
            break;
        }
    }
    Ok(items)
}

/// One page of a full-table scan (used only when the caller supplies no
/// producer/consumer filter), with an opaque `nextToken` cursor for the
/// caller to continue from.
pub async fn scan_streams_page(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    max_results: i32,
    exclusive_start_key: Option<HashMap<String, AttributeValue>>,
) -> Result<(Vec<EventStream>, Option<HashMap<String, AttributeValue>>)> {
    let output = ddb
        .scan()
        .table_name(table)
        .limit(max_results)
        .set_exclusive_start_key(exclusive_start_key)
        .send()
        .await
        .context("failed to scan event streams table")?;
    Ok((collect_streams(output.items), output.last_evaluated_key))
}

/// Marks a stream REMOVED and stamps `removedAt` plus the TTL that expires
/// the record after the audit retention window.
pub fn removal_timestamps(removed_at: DateTime<Utc>) -> (String, i64) {
    let expires_at = (removed_at + Duration::days(REMOVED_RETENTION_DAYS)).timestamp();
    (removed_at.to_rfc3339(), expires_at)
}

pub async fn mark_stream_removed(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    producer_domain: &str,
    schema_name: &str,
    consumer_domain: &str,
    removed_at: DateTime<Utc>,
) -> Result<()> {
    let (removed_at_str, expires_at) = removal_timestamps(removed_at);
    ddb.update_item()
        .table_name(table)
        .key(PK, s(producer_domain))
        .key(SK, s(&EventStream::sort_key(schema_name, consumer_domain)))
        .update_expression(
            "SET #status = :removed, removedAt = :removedAt, expiresAt = :expiresAt, updatedAt = :removedAt",
        )
        .expression_attribute_names("#status", "status")
        .expression_attribute_values(":removed", s(StreamStatus::Removed.as_str()))
        .expression_attribute_values(":removedAt", s(&removed_at_str))
        .expression_attribute_values(":expiresAt", n(expires_at))
        .send()
        .await
        .context("failed to mark event stream removed")?;
    Ok(())
}

/// Flips one environment's desired-state flag on a stream record. Rule
/// identities, pinned versions, and the record itself are retained — only the
/// per-environment `enabled` flag changes.
pub async fn set_stream_env_enabled(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    stream: &EventStream,
    env_name: &str,
    enabled: bool,
) -> Result<()> {
    ddb.update_item()
        .table_name(table)
        .key(PK, s(&stream.producer_domain))
        .key(
            SK,
            s(&EventStream::sort_key(
                &stream.schema_name,
                &stream.consumer_domain,
            )),
        )
        .update_expression("SET environments.#env.enabled = :enabled, updatedAt = :now")
        .expression_attribute_names("#env", env_name)
        .expression_attribute_values(":enabled", AttributeValue::Bool(enabled))
        .expression_attribute_values(":now", s(&chrono::Utc::now().to_rfc3339()))
        .send()
        .await
        .context("failed to persist env enabled state")?;
    Ok(())
}

fn collect_streams(items: Option<Vec<HashMap<String, AttributeValue>>>) -> Vec<EventStream> {
    items
        .unwrap_or_default()
        .iter()
        .filter_map(item_to_stream)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streams::SubscriptionId;

    fn sample_stream() -> EventStream {
        let mut environments = HashMap::new();
        environments.insert(
            "dev".to_string(),
            EnvStream {
                pinned_version: Some(5),
                rule_name: "checkout-OrderNotification".to_string(),
                rule_arn: Some("arn:aws:events:us-east-1:111122223333:rule/x".to_string()),
                enabled: true,
                status: StreamStatus::Active,
                error_detail: None,
            },
        );
        environments.insert(
            "prod".to_string(),
            EnvStream {
                pinned_version: Some(3),
                rule_name: "checkout-OrderNotification".to_string(),
                rule_arn: None,
                enabled: false,
                status: StreamStatus::Pending,
                error_detail: None,
            },
        );
        EventStream {
            producer_domain: "orders".to_string(),
            consumer_domain: "checkout".to_string(),
            source: "acme-orders".to_string(),
            detail_type: "OrderNotification".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            status: StreamStatus::Pending,
            error_detail: None,
            environments,
            request_id: Some("req-123".to_string()),
            created_at: "2026-07-15T00:00:00Z".parse().unwrap(),
            updated_at: "2026-07-15T00:00:00Z".parse().unwrap(),
            removed_at: None,
            expires_at: None,
        }
    }

    #[test]
    fn stream_round_trips_through_item() {
        let stream = sample_stream();
        let item = stream_to_item(&stream);
        let restored = item_to_stream(&item).unwrap();

        assert_eq!(restored.producer_domain, "orders");
        assert_eq!(restored.consumer_domain, "checkout");
        assert_eq!(restored.schema_name, "acme-orders@OrderNotification");
        assert_eq!(restored.status, StreamStatus::Pending);
        assert_eq!(restored.environments.len(), 2);

        let dev = &restored.environments["dev"];
        assert_eq!(dev.pinned_version, Some(5));
        assert!(dev.enabled);
        assert_eq!(dev.status, StreamStatus::Active);

        let prod = &restored.environments["prod"];
        assert_eq!(prod.pinned_version, Some(3));
        assert!(!prod.enabled);
        assert!(prod.rule_arn.is_none());
    }

    #[test]
    fn item_carries_census_friendly_keys() {
        let item = stream_to_item(&sample_stream());
        assert_eq!(item[PK].as_s().unwrap(), "orders");
        assert_eq!(
            item[SK].as_s().unwrap(),
            "acme-orders@OrderNotification#checkout"
        );
    }

    #[test]
    fn removal_timestamps_add_90_day_ttl() {
        let removed_at: DateTime<Utc> = "2026-07-15T12:00:00Z".parse().unwrap();
        let (removed_str, expires_at) = removal_timestamps(removed_at);
        assert_eq!(removed_str, removed_at.to_rfc3339());
        assert_eq!(expires_at, (removed_at + Duration::days(90)).timestamp());
    }

    #[test]
    fn subscription_id_matches_stream_identity() {
        let stream = sample_stream();
        let id = stream.subscription_id();
        assert_eq!(
            id,
            SubscriptionId {
                consumer_domain: "checkout".to_string(),
                producer_domain: "orders".to_string(),
                detail_type: "OrderNotification".to_string(),
            }
        );
        assert_eq!(id.to_string(), "checkout.orders.OrderNotification");
    }

    #[test]
    fn item_to_stream_returns_none_for_missing_required_field() {
        let mut item = stream_to_item(&sample_stream());
        item.remove("schemaName");
        assert!(item_to_stream(&item).is_none());
    }

    #[tokio::test]
    async fn merge_env_stream_updates_only_the_named_env_and_timestamp() {
        use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
        use aws_smithy_mocks::{mock, mock_client};

        let merge_rule = mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                req.update_expression() == Some("SET environments.#env = :entry, updatedAt = :now")
                    && req
                        .expression_attribute_names()
                        .and_then(|names| names.get("#env"))
                        .map(String::as_str)
                        == Some("staging")
                    && req
                        .expression_attribute_values()
                        .and_then(|v| v.get(":entry"))
                        .and_then(|v| v.as_m().ok())
                        .and_then(|m| m.get("status"))
                        .and_then(|v| v.as_s().ok())
                        .map(String::as_str)
                        == Some("AWAITING_PROMOTION")
            })
            .then_output(|| UpdateItemOutput::builder().build());
        let ddb = mock_client!(aws_sdk_dynamodb, &[&merge_rule]);

        let entry = EnvStream {
            pinned_version: None,
            rule_name: "checkout-OrderNotification".to_string(),
            rule_arn: None,
            enabled: true,
            status: StreamStatus::AwaitingPromotion,
            error_detail: None,
        };
        merge_env_stream(
            &ddb,
            "konductor-event-streams",
            "orders",
            "acme-orders@OrderNotification",
            "checkout",
            "staging",
            &entry,
        )
        .await
        .unwrap();

        assert_eq!(merge_rule.num_calls(), 1);
    }
}
