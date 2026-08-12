//! Hourly reconciliation sweep: scans active subscriptions, re-evaluates
//! each against the consumer domain's current entitlements, and disables any
//! uncovered Konductor-managed rules via the worker's disable path.
//!
//! Unlike the revocation worker — which fails closed per candidate because a
//! revocation already happened — the sweep must fail *safe*: an entitlement
//! store error aborts the run rather than disabling subscriptions en masse
//! on a transient read failure. Drift is bounded by the next hourly run.

use std::collections::HashMap;
use std::env;

use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::Value;

use entitlement_revocation_worker::revoke::{disable_stream_environments, RevocationContext};
use konductor_common::entitlement::Entitlement;
use konductor_common::entitlement_store::EntitlementStore;
use konductor_common::grant_engine::covers;
use konductor_common::streams::{EventStream, StreamStatus};
use konductor_common::streams_db;

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub eb: aws_sdk_eventbridge::Client,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: aws_sdk_dynamodb::Client::new(config),
            eb: aws_sdk_eventbridge::Client::new(config),
        }
    }
}

pub async fn handle(clients: &Clients, _event: LambdaEvent<Value>) -> Result<Value, Error> {
    let streams_table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let entitlements_table =
        env::var("ENTITLEMENTS_TABLE").context("ENTITLEMENTS_TABLE not set")?;
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;

    let ctx = RevocationContext {
        ddb: &clients.ddb,
        eb: &clients.eb,
        streams_table: &streams_table,
        entitlements_table: &entitlements_table,
        organization: &organization,
    };

    let mut scanned = 0usize;
    let mut disabled = 0usize;
    let mut coverage_cache: HashMap<String, Vec<Entitlement>> = HashMap::new();
    let mut cursor = None;

    loop {
        let (page, next) =
            streams_db::scan_streams_page(&clients.ddb, &streams_table, 200, cursor).await?;
        cursor = next;
        for stream in &page {
            scanned += 1;
            if !is_sweep_target(stream) {
                continue;
            }
            let covered = currently_covered(&ctx, &mut coverage_cache, stream).await?;
            if covered {
                continue;
            }
            let count = disable_stream_environments(&ctx, stream).await?;
            disabled += count;
            tracing::warn!(
                consumer = stream.consumer_domain,
                producer = stream.producer_domain,
                schema = stream.schema_name,
                disabled = count,
                "sweep disabled uncovered subscription environments"
            );
        }
        if cursor.is_none() {
            break;
        }
    }

    tracing::info!(scanned, disabled, "coverage sweep complete");
    Ok(Value::Null)
}

fn is_sweep_target(stream: &EventStream) -> bool {
    stream.status == StreamStatus::Active && stream.consumer_domain != stream.producer_domain
}

/// Coverage for one stream, caching each consumer domain's entitlements for
/// the run. Store errors abort the sweep (fail-safe), unlike the worker's
/// per-candidate fail-closed posture.
async fn currently_covered(
    ctx: &RevocationContext<'_>,
    cache: &mut HashMap<String, Vec<Entitlement>>,
    stream: &EventStream,
) -> Result<bool> {
    if !cache.contains_key(&stream.consumer_domain) {
        let store = EntitlementStore::new(ctx.ddb.clone(), ctx.entitlements_table);
        let current = store
            .entitlements_for_consumer(&stream.consumer_domain)
            .await
            .map_err(|e| anyhow::anyhow!("entitlement read failed; aborting sweep: {e}"))?;
        cache.insert(stream.consumer_domain.clone(), current);
    }
    let current = &cache[&stream.consumer_domain];
    Ok(covers(
        current,
        &stream.producer_domain,
        &stream.detail_type,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::query::QueryOutput;
    use aws_sdk_dynamodb::operation::scan::ScanOutput;
    use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_sdk_eventbridge::operation::disable_rule::DisableRuleOutput;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use lambda_runtime::Context as LambdaContext;
    use serde_json::json;

    use konductor_common::entitlement::ScopePattern;
    use konductor_common::entitlement_store::entitlement_to_item;
    use konductor_common::streams::EnvStream;
    use konductor_common::streams_db::stream_to_item;

    fn set_env() {
        std::env::set_var("EVENT_STREAMS_TABLE", "streams");
        std::env::set_var("ENTITLEMENTS_TABLE", "entitlements");
        std::env::set_var("ORGANIZATION", "acme");
    }

    fn stream(
        consumer: &str,
        producer: &str,
        detail_type: &str,
        status: StreamStatus,
        enabled: bool,
    ) -> EventStream {
        let mut environments = HashMap::new();
        environments.insert(
            "prod".to_string(),
            EnvStream {
                pinned_version: Some(1),
                rule_name: format!("{consumer}-{detail_type}"),
                rule_arn: None,
                enabled,
                status: StreamStatus::Active,
            },
        );
        EventStream {
            producer_domain: producer.to_string(),
            consumer_domain: consumer.to_string(),
            source: format!("acme-{producer}"),
            detail_type: detail_type.to_string(),
            schema_name: format!("acme-{producer}@{detail_type}"),
            status,
            environments,
            request_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
            expires_at: None,
        }
    }

    fn scan_rule(streams: Vec<EventStream>) -> Rule {
        let items: Vec<HashMap<String, AttributeValue>> =
            streams.iter().map(stream_to_item).collect();
        mock!(aws_sdk_dynamodb::Client::scan)
            .then_output(move || ScanOutput::builder().set_items(Some(items.clone())).build())
    }

    fn entitlements_rule(consumer: &'static str, patterns: Vec<&'static str>) -> Rule {
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(move |req| {
                req.expression_attribute_values()
                    .and_then(|v| v.get(":pk"))
                    .and_then(|v| v.as_s().ok())
                    .map(|v| *v == format!("CONSUMER#{consumer}"))
                    .unwrap_or(false)
            })
            .then_output(move || {
                let items: Vec<HashMap<String, AttributeValue>> = patterns
                    .iter()
                    .map(|p| {
                        let e = Entitlement::new(
                            consumer,
                            "orders",
                            ScopePattern::new(*p).unwrap(),
                            Utc::now(),
                            "test",
                        )
                        .unwrap();
                        entitlement_to_item(&e)
                    })
                    .collect();
                QueryOutput::builder().set_items(Some(items)).build()
            })
    }

    fn disable_rule() -> Rule {
        mock!(aws_sdk_eventbridge::Client::disable_rule)
            .then_output(|| DisableRuleOutput::builder().build())
    }

    fn update_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::update_item)
            .then_output(|| UpdateItemOutput::builder().build())
    }

    fn clients(ddb_rules: &[&Rule], eb_rules: &[&Rule]) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, eb_rules),
        }
    }

    async fn run(c: &Clients) -> Result<Value, Error> {
        handle(c, LambdaEvent::new(json!({}), LambdaContext::default())).await
    }

    #[tokio::test]
    async fn uncovered_active_stream_is_disabled() {
        set_env();
        let drifted = stream(
            "checkout",
            "orders",
            "OrderNotification",
            StreamStatus::Active,
            true,
        );
        let scan = scan_rule(vec![drifted]);
        let no_grants = entitlements_rule("checkout", vec![]);
        let disable = disable_rule();
        let update = update_rule();
        let c = clients(&[&scan, &no_grants, &update], &[&disable]);

        run(&c).await.unwrap();

        assert_eq!(disable.num_calls(), 1);
        assert_eq!(update.num_calls(), 1);
    }

    #[tokio::test]
    async fn covered_streams_are_a_no_op() {
        set_env();
        let covered = stream(
            "checkout",
            "orders",
            "OrderNotification",
            StreamStatus::Active,
            true,
        );
        let scan = scan_rule(vec![covered]);
        let grants = entitlements_rule("checkout", vec!["Order*"]);
        let disable = disable_rule();
        let update = update_rule();
        let c = clients(&[&scan, &grants, &update], &[&disable]);

        run(&c).await.unwrap();

        assert_eq!(disable.num_calls(), 0);
        assert_eq!(update.num_calls(), 0);
    }

    #[tokio::test]
    async fn pending_removed_and_self_subscription_streams_are_skipped() {
        set_env();
        let pending = stream(
            "checkout",
            "orders",
            "OrderNotification",
            StreamStatus::Pending,
            true,
        );
        let removed = stream(
            "checkout",
            "orders",
            "OrderRefunded",
            StreamStatus::Removed,
            true,
        );
        let self_sub = stream(
            "orders",
            "orders",
            "OrderDisputed",
            StreamStatus::Active,
            true,
        );
        let scan = scan_rule(vec![pending, removed, self_sub]);
        let disable = disable_rule();
        let update = update_rule();
        // No entitlement rule: skipped streams must never reach coverage.
        let c = clients(&[&scan, &update], &[&disable]);

        run(&c).await.unwrap();

        assert_eq!(disable.num_calls(), 0);
        assert_eq!(update.num_calls(), 0);
    }

    #[tokio::test]
    async fn entitlement_store_error_aborts_without_disabling() {
        set_env();
        let drifted = stream(
            "checkout",
            "orders",
            "OrderNotification",
            StreamStatus::Active,
            true,
        );
        let scan = scan_rule(vec![drifted]);
        let failing_query = mock!(aws_sdk_dynamodb::Client::query).then_error(|| {
            aws_sdk_dynamodb::operation::query::QueryError::generic(
                aws_sdk_dynamodb::error::ErrorMetadata::builder()
                    .message("simulated outage")
                    .build(),
            )
        });
        let disable = disable_rule();
        let update = update_rule();
        let c = clients(&[&scan, &failing_query, &update], &[&disable]);

        let result = run(&c).await;

        assert!(result.is_err(), "sweep aborts on store failure");
        assert_eq!(
            disable.num_calls(),
            0,
            "fail-safe: a read outage never disables subscriptions"
        );
    }
}
