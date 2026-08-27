use std::env;

use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use crate::revoke::{process_revocation, RevocationContext, RevocationMessage};

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

pub async fn handle(clients: &Clients, event: LambdaEvent<Value>) -> Result<Value, Error> {
    let records = event
        .payload
        .get("Records")
        .and_then(|r| r.as_array())
        .context("missing Records array")?;

    let mut batch_item_failures = Vec::new();
    for record in records {
        let Some(message_id) = record.get("messageId").and_then(|v| v.as_str()) else {
            tracing::error!("skipping SQS record without messageId");
            continue;
        };
        if let Err(e) = process_record(clients, record).await {
            tracing::error!(error = %e, message_id, "revocation processing failed; reporting for retry");
            batch_item_failures.push(json!({ "itemIdentifier": message_id }));
        }
    }

    Ok(json!({ "batchItemFailures": batch_item_failures }))
}

async fn process_record(clients: &Clients, record: &Value) -> Result<()> {
    let body = record
        .get("body")
        .and_then(|b| b.as_str())
        .context("missing SQS record body")?;
    let message: RevocationMessage =
        serde_json::from_str(body).context("invalid revocation message")?;

    let ctx = RevocationContext {
        ddb: &clients.ddb,
        eb: &clients.eb,
        streams_table: &env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?,
        entitlements_table: &env::var("ENTITLEMENTS_TABLE")
            .context("ENTITLEMENTS_TABLE not set")?,
        organization: &env::var("ORGANIZATION").context("ORGANIZATION not set")?,
    };
    process_revocation(&ctx, &message).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use aws_sdk_dynamodb::operation::query::QueryOutput;
    use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_sdk_eventbridge::operation::disable_rule::DisableRuleOutput;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use lambda_runtime::Context as LambdaContext;

    use crate::revoke::is_revocation_target;
    use konductor_common::entitlement::{Entitlement, ScopePattern};
    use konductor_common::entitlement_store::entitlement_to_item;
    use konductor_common::streams::{EnvStream, EventStream, StreamStatus};
    use konductor_common::streams_db::stream_to_item;

    fn set_env() {
        std::env::set_var("EVENT_STREAMS_TABLE", "streams");
        std::env::set_var("ENTITLEMENTS_TABLE", "entitlements");
        std::env::set_var("ORGANIZATION", "acme");
    }

    fn message(scope_pattern: &str) -> Value {
        json!({
            "revocationId": "evt-1",
            "entitlementId": "ent-1",
            "consumerDomain": "checkout",
            "producerDomain": "orders",
            "scopePattern": scope_pattern,
            "revokedAt": "2026-01-01T00:00:00Z"
        })
    }

    fn sqs_record(id: &str, body: Value) -> Value {
        json!({ "messageId": id, "body": body.to_string() })
    }

    fn stream(
        consumer: &str,
        detail_type: &str,
        status: StreamStatus,
        envs: &[(&str, bool)],
    ) -> EventStream {
        let environments = envs
            .iter()
            .map(|(name, enabled)| {
                (
                    name.to_string(),
                    EnvStream {
                        pinned_version: Some(3),
                        rule_name: format!("{consumer}-{detail_type}"),
                        rule_arn: Some(format!(
                            "arn:aws:events:us-east-1:123:rule/acme-{name}-orders-event-bus/{consumer}-{detail_type}"
                        )),
                        enabled: *enabled,
                        status: StreamStatus::Active,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        EventStream {
            producer_domain: "orders".to_string(),
            consumer_domain: consumer.to_string(),
            source: "acme-orders".to_string(),
            detail_type: detail_type.to_string(),
            schema_name: format!("acme-orders@{detail_type}"),
            status,
            environments,
            request_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
            expires_at: None,
        }
    }

    /// Streams query whose items are the given streams; narrows by whatever
    /// key condition the worker chooses (schema, prefix, or partition scan).
    fn streams_query_rule(streams: Vec<EventStream>) -> Rule {
        let items: Vec<HashMap<String, AttributeValue>> =
            streams.iter().map(stream_to_item).collect();
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req| {
                req.expression_attribute_values()
                    .and_then(|v| v.get(":producer"))
                    .and_then(|v| v.as_s().ok())
                    .map(|v| v == "orders")
                    .unwrap_or(false)
            })
            .then_output(move || {
                QueryOutput::builder()
                    .set_items(Some(items.clone()))
                    .build()
            })
    }

    fn entitlements_rule(patterns: Vec<&str>) -> Rule {
        let items: Vec<HashMap<String, AttributeValue>> = patterns
            .into_iter()
            .map(|p| {
                let e = Entitlement::new(
                    "checkout",
                    "orders",
                    ScopePattern::new(p).unwrap(),
                    Utc::now(),
                    "test",
                )
                .unwrap();
                entitlement_to_item(&e)
            })
            .collect();
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req| {
                req.expression_attribute_values()
                    .and_then(|v| v.get(":pk"))
                    .and_then(|v| v.as_s().ok())
                    .map(|v| v.starts_with("CONSUMER#"))
                    .unwrap_or(false)
            })
            .then_output(move || {
                QueryOutput::builder()
                    .set_items(Some(items.clone()))
                    .build()
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

    async fn run(c: &Clients, scope_pattern: &str) -> Value {
        handle(
            c,
            LambdaEvent::new(
                json!({ "Records": [sqs_record("m-1", message(scope_pattern))] }),
                LambdaContext::default(),
            ),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn last_grant_revocation_disables_every_enabled_environment() {
        set_env();
        let target = stream(
            "checkout",
            "OrderNotification",
            StreamStatus::Active,
            &[("dev", true), ("prod", true)],
        );
        let streams = streams_query_rule(vec![target]);
        let no_grants = entitlements_rule(vec![]);
        let disable = disable_rule();
        let update = update_rule();
        let c = clients(&[&streams, &no_grants, &update], &[&disable]);

        let result = run(&c, "OrderNotification").await;

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(disable.num_calls(), 2, "one disable per enabled env");
        assert_eq!(update.num_calls(), 2, "desired state persisted per env");
    }

    #[tokio::test]
    async fn overlapping_grant_preserves_the_subscription() {
        set_env();
        let target = stream(
            "checkout",
            "OrderNotification",
            StreamStatus::Active,
            &[("prod", true)],
        );
        let streams = streams_query_rule(vec![target]);
        let overlapping = entitlements_rule(vec!["Order*"]);
        let disable = disable_rule();
        let update = update_rule();
        let c = clients(&[&streams, &overlapping, &update], &[&disable]);

        let result = run(&c, "OrderNotification").await;

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(disable.num_calls(), 0, "overlapping grant keeps coverage");
        assert_eq!(update.num_calls(), 0);
    }

    #[tokio::test]
    async fn redrive_after_a_new_grant_cancels_the_disable() {
        set_env();
        let target = stream(
            "checkout",
            "OrderNotification",
            StreamStatus::Active,
            &[("prod", true)],
        );
        let streams = streams_query_rule(vec![target]);
        // The grant was revoked, the message redelivered, and a fresh grant
        // created before re-processing: current coverage wins.
        let new_grant = entitlements_rule(vec!["OrderNotification"]);
        let disable = disable_rule();
        let update = update_rule();
        let c = clients(&[&streams, &new_grant, &update], &[&disable]);

        let result = run(&c, "OrderNotification").await;

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(disable.num_calls(), 0);
    }

    #[tokio::test]
    async fn already_disabled_environments_are_skipped() {
        set_env();
        let target = stream(
            "checkout",
            "OrderNotification",
            StreamStatus::Active,
            &[("dev", false), ("prod", true)],
        );
        let streams = streams_query_rule(vec![target]);
        let no_grants = entitlements_rule(vec![]);
        let disable = disable_rule();
        let update = update_rule();
        let c = clients(&[&streams, &no_grants, &update], &[&disable]);

        let result = run(&c, "OrderNotification").await;

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(disable.num_calls(), 1, "only the enabled env is disabled");
        assert_eq!(update.num_calls(), 1);
    }

    #[tokio::test]
    async fn other_consumers_pending_streams_and_self_subscriptions_are_untouched() {
        set_env();
        let other_consumer = stream(
            "billing",
            "OrderNotification",
            StreamStatus::Active,
            &[("prod", true)],
        );
        let pending = stream(
            "checkout",
            "OrderNotification",
            StreamStatus::Pending,
            &[("prod", true)],
        );
        let self_sub = EventStream {
            producer_domain: "orders".to_string(),
            consumer_domain: "orders".to_string(),
            ..stream(
                "checkout",
                "OrderNotification",
                StreamStatus::Active,
                &[("prod", true)],
            )
        };
        let streams = streams_query_rule(vec![other_consumer, pending, self_sub]);
        let disable = disable_rule();
        let update = update_rule();
        // No entitlement rule: no eligible candidate may reach coverage.
        let c = clients(&[&streams, &update], &[&disable]);

        let result = run(&c, "OrderNotification").await;

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(disable.num_calls(), 0);
        assert_eq!(update.num_calls(), 0);
    }

    #[tokio::test]
    async fn domain_wide_revocation_queries_the_whole_producer_partition() {
        set_env();
        let whole_partition = mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req| {
                req.key_condition_expression()
                    .map(|e| e == "#pk = :producer")
                    .unwrap_or(false)
            })
            .then_output(|| QueryOutput::builder().build());
        let c = clients(&[&whole_partition], &[]);

        let result = run(&c, "*").await;

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(whole_partition.num_calls(), 1);
    }

    #[tokio::test]
    async fn prefix_revocation_narrows_candidates_by_schema_prefix() {
        set_env();
        let prefix_query = mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req| {
                req.expression_attribute_values()
                    .and_then(|v| v.get(":prefix"))
                    .and_then(|v| v.as_s().ok())
                    .map(|v| v == "acme-orders@Order")
                    .unwrap_or(false)
            })
            .then_output(|| QueryOutput::builder().build());
        let c = clients(&[&prefix_query], &[]);

        let result = run(&c, "Order*").await;

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(prefix_query.num_calls(), 1);
    }

    #[tokio::test]
    async fn store_failure_reports_the_message_for_redrive() {
        set_env();
        let failing_query = mock!(aws_sdk_dynamodb::Client::query).then_error(|| {
            aws_sdk_dynamodb::operation::query::QueryError::generic(
                aws_sdk_dynamodb::error::ErrorMetadata::builder()
                    .message("simulated outage")
                    .build(),
            )
        });
        let c = clients(&[&failing_query], &[]);

        let result = run(&c, "*").await;

        assert_eq!(
            result["batchItemFailures"],
            json!([{ "itemIdentifier": "m-1" }])
        );
    }

    #[test]
    fn revocation_message_deserializes() {
        let parsed: RevocationMessage =
            serde_json::from_value(message("Order*")).expect("message parses");
        assert_eq!(parsed.consumer_domain, "checkout");
        assert_eq!(parsed.scope_pattern, "Order*");
    }

    #[test]
    fn target_filter_accepts_only_active_cross_domain_consumer_streams() {
        let msg: RevocationMessage = serde_json::from_value(message("*")).expect("message parses");
        let active = stream("checkout", "OrderNotification", StreamStatus::Active, &[]);
        let removed = stream("checkout", "OrderNotification", StreamStatus::Removed, &[]);
        assert!(is_revocation_target(&active, &msg));
        assert!(!is_revocation_target(&removed, &msg));
    }
}
