//! `mode: repin` — promotion-driven re-pinning of an existing subscription
//! rule. The widen phase applies a broadened version list without touching
//! the recorded pin; the narrow phase applies the final list and commits it.
//! Neither phase touches targets, alarms, or entitlements: the rule already
//! materialized and delivery must continue uninterrupted.

use anyhow::Result;
use aws_sdk_eventbridge::types::RuleState;

use konductor_common::streams_db::{set_env_failed, set_env_pinned_version};

use crate::config::Config;
use crate::handler::{sdk_cause, Clients, MaterializeInput};
use crate::outcome::{Outcome, StepError};

pub(crate) async fn repin(
    clients: &Clients,
    config: &Config,
    input: &MaterializeInput,
) -> Result<Outcome> {
    if input.versions.is_empty() {
        anyhow::bail!("mode: repin requires a non-empty versions list");
    }
    let bus = config.producer_bus(input);
    let state = match describe_rule_state(clients, &bus, input).await {
        Ok(state) => state,
        Err(cause) => {
            record_failure(clients, config, input, "DescribeSubscriptionRule", &cause).await;
            return Ok(outcome(
                input,
                "FAILED",
                None,
                Some(("DescribeSubscriptionRule", cause)),
            ));
        }
    };
    if let Err(cause) = put_rule_versions(clients, &bus, input, state).await {
        record_failure(clients, config, input, "RepinSubscriptionRule", &cause).await;
        return Ok(outcome(
            input,
            "FAILED",
            None,
            Some(("RepinSubscriptionRule", cause)),
        ));
    }
    let pinned = pin_for(&input.versions);
    if input.commit_pin {
        set_env_pinned_version(
            &clients.ddb,
            &config.streams_table,
            &input.producer_domain,
            &input.schema_name,
            &input.consumer_domain,
            &input.environment,
            pinned,
        )
        .await?;
    }
    Ok(outcome(input, "REPINNED", Some(pinned), None))
}

/// A failed narrow-phase re-pin marks the subscription entry `FAILED` with
/// the cause; the widen phase retries instead of recording. Best effort: a
/// failed record write must not mask the step's own error outcome.
async fn record_failure(
    clients: &Clients,
    config: &Config,
    input: &MaterializeInput,
    step: &str,
    cause: &str,
) {
    if !input.commit_pin {
        return;
    }
    if let Err(err) = set_env_failed(
        &clients.ddb,
        &config.streams_table,
        &input.producer_domain,
        &input.schema_name,
        &input.consumer_domain,
        &input.environment,
        &format!("{step}: {cause}"),
    )
    .await
    {
        tracing::error!(error = %err, "failed to record re-pin failure on the stream record");
    }
}

/// The pattern always ends with the lookahead slot, so the committed pin is
/// the highest stable version: the second-to-last entry when the list has
/// one, the only entry otherwise.
fn pin_for(versions: &[i64]) -> i64 {
    match versions.len() {
        1 => versions[0],
        n => versions[n - 2],
    }
}

/// A re-pin preserves the rule's current state: a subscription the consumer
/// disabled must stay disabled through a promotion.
async fn describe_rule_state(
    clients: &Clients,
    bus: &str,
    input: &MaterializeInput,
) -> std::result::Result<RuleState, String> {
    clients
        .eventbridge
        .describe_rule()
        .name(&input.rule_name)
        .event_bus_name(bus)
        .send()
        .await
        .map_err(|err| sdk_cause(&err))?
        .state()
        .cloned()
        .ok_or_else(|| "describeRule returned no State".to_string())
}

async fn put_rule_versions(
    clients: &Clients,
    bus: &str,
    input: &MaterializeInput,
    state: RuleState,
) -> std::result::Result<(), String> {
    let pattern = serde_json::json!({
        "source": [input.source],
        "detail-type": [input.detail_type],
        "detail": { "metadata": { "schemaVersion": input.versions } },
    })
    .to_string();
    clients
        .eventbridge
        .put_rule()
        .name(&input.rule_name)
        .event_bus_name(bus)
        .event_pattern(pattern)
        .state(state)
        .send()
        .await
        .map(|_| ())
        .map_err(|err| sdk_cause(&err))
}

fn outcome(
    input: &MaterializeInput,
    status: &'static str,
    pinned_version: Option<i64>,
    error: Option<(&'static str, String)>,
) -> Outcome {
    Outcome {
        status,
        environment: input.environment.clone(),
        enabled: input.enabled,
        rule_name: input.rule_name.clone(),
        pinned_version,
        rule_arn: None,
        error: error.map(|(failed_step, cause)| StepError { failed_step, cause }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
    use aws_sdk_eventbridge::operation::describe_rule::DescribeRuleOutput;
    use aws_sdk_eventbridge::operation::put_rule::PutRuleOutput;
    use aws_sdk_eventbridge::types::error::ResourceNotFoundException;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use serde_json::{json, Value};

    use crate::handler::handle;

    fn set_env() {
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var("EVENT_STREAMS_TABLE", "konductor-event-streams");
        std::env::set_var("ENTITLEMENTS_TABLE", "konductor-authorization-entitlements");
        std::env::set_var(
            "SUBSCRIPTION_DELIVERY_ROLE_ARN",
            "arn:aws:iam::999988887777:role/konductor-subscription-delivery",
        );
        std::env::set_var("PLATFORM_ACCOUNT_ID", "999988887777");
        std::env::set_var("AWS_REGION", "ap-southeast-2");
    }

    fn repin_input(versions: Vec<i64>, commit_pin: bool) -> Value {
        json!({
            "producerDomain": "billing",
            "consumerDomain": "shipping",
            "source": "acme-billing",
            "detailType": "OrderNotification",
            "schemaName": "acme-billing@OrderNotification",
            "ruleName": "shipping-OrderNotification",
            "environment": "staging",
            "enabled": true,
            "mode": "repin",
            "versions": versions,
            "commitPin": commit_pin,
        })
    }

    fn describe_state_rule(state: RuleState) -> Rule {
        mock!(aws_sdk_eventbridge::Client::describe_rule)
            .match_requests(move |req| {
                req.name() == Some("shipping-OrderNotification")
                    && req.event_bus_name() == Some("acme-staging-billing-event-bus")
            })
            .then_output(move || DescribeRuleOutput::builder().state(state.clone()).build())
    }

    fn repin_rule(expected_state: RuleState, versions: &[i64]) -> Rule {
        let needle = format!(
            "\"schemaVersion\":[{}]",
            versions
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        mock!(aws_sdk_eventbridge::Client::put_rule)
            .match_requests(move |req| {
                req.name() == Some("shipping-OrderNotification")
                    && req.event_bus_name() == Some("acme-staging-billing-event-bus")
                    && req.state() == Some(&expected_state)
                    && req.event_pattern().is_some_and(|p| p.contains(&needle))
            })
            .then_output(|| PutRuleOutput::builder().build())
    }

    fn pin_commit_rule(expected_pin: i64) -> Rule {
        mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(move |req| {
                req.table_name() == Some("konductor-event-streams")
                    && req.update_expression()
                        == Some("SET environments.#env.pinnedVersion = :pin, updatedAt = :now")
                    && req
                        .expression_attribute_values()
                        .and_then(|v| v.get(":pin"))
                        .and_then(|v| v.as_n().ok())
                        .map(String::as_str)
                        == Some(Box::leak(expected_pin.to_string().into_boxed_str()))
            })
            .then_output(|| UpdateItemOutput::builder().build())
    }

    fn clients(ddb_rules: &[&Rule], eb_rules: &[&Rule]) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
            schemas: mock_client!(aws_sdk_schemas, RuleMode::MatchAny, &[]),
            eventbridge: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, eb_rules),
            cloudwatch: mock_client!(aws_sdk_cloudwatch, RuleMode::MatchAny, &[]),
        }
    }

    #[tokio::test]
    async fn repin_applies_the_version_list_and_preserves_a_disabled_rule() {
        set_env();
        let describe = describe_state_rule(RuleState::Disabled);
        let put = repin_rule(RuleState::Disabled, &[3, 4, 5]);
        let clients = clients(&[], &[&describe, &put]);

        let outcome = handle(&clients, repin_input(vec![3, 4, 5], false))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "REPINNED");
        assert_eq!(outcome["ruleName"], "shipping-OrderNotification");
        assert_eq!(outcome["pinnedVersion"], 4);
        assert!(outcome.get("error").is_none());
        assert_eq!(put.num_calls(), 1);
    }

    #[tokio::test]
    async fn repin_without_commit_pin_leaves_the_stream_record_untouched() {
        set_env();
        let describe = describe_state_rule(RuleState::Enabled);
        let put = repin_rule(RuleState::Enabled, &[5, 6]);
        let pin_commit = pin_commit_rule(5);
        let clients = clients(&[&pin_commit], &[&describe, &put]);

        let outcome = handle(&clients, repin_input(vec![5, 6], false))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "REPINNED");
        assert_eq!(pin_commit.num_calls(), 0);
    }

    #[tokio::test]
    async fn repin_with_commit_pin_advances_the_recorded_pin() {
        set_env();
        let describe = describe_state_rule(RuleState::Enabled);
        let put = repin_rule(RuleState::Enabled, &[5, 6]);
        let pin_commit = pin_commit_rule(5);
        let clients = clients(&[&pin_commit], &[&describe, &put]);

        let outcome = handle(&clients, repin_input(vec![5, 6], true))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "REPINNED");
        assert_eq!(outcome["pinnedVersion"], 5);
        assert_eq!(pin_commit.num_calls(), 1);
    }

    fn failed_entry_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                req.table_name() == Some("konductor-event-streams")
                    && req.update_expression().is_some_and(|e| {
                        e.contains("environments.#env.#status = :failed")
                            && e.contains("environments.#env.errorDetail = :detail")
                    })
                    && req
                        .expression_attribute_values()
                        .and_then(|v| v.get(":failed"))
                        .and_then(|v| v.as_s().ok())
                        .map(String::as_str)
                        == Some("FAILED")
            })
            .then_output(|| UpdateItemOutput::builder().build())
    }

    #[tokio::test]
    async fn repin_put_rule_error_surfaces_a_failed_entry_outcome() {
        set_env();
        let describe = describe_state_rule(RuleState::Enabled);
        let failing_put = mock!(aws_sdk_eventbridge::Client::put_rule).then_error(|| {
            aws_sdk_eventbridge::operation::put_rule::PutRuleError::ResourceNotFoundException(
                ResourceNotFoundException::builder()
                    .message("rule gone")
                    .build(),
            )
        });
        let failed_entry = failed_entry_rule();
        let clients = clients(&[&failed_entry], &[&describe, &failing_put]);

        let outcome = handle(&clients, repin_input(vec![5, 6], true))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "FAILED");
        assert_eq!(outcome["error"]["failedStep"], "RepinSubscriptionRule");
        assert!(outcome["error"]["cause"]
            .as_str()
            .unwrap()
            .contains("rule gone"));
        assert_eq!(
            failed_entry.num_calls(),
            1,
            "the narrow phase records the failing entry as FAILED"
        );
    }

    #[tokio::test]
    async fn widen_phase_failure_does_not_record_a_failed_entry() {
        set_env();
        let describe = describe_state_rule(RuleState::Enabled);
        let failing_put = mock!(aws_sdk_eventbridge::Client::put_rule).then_error(|| {
            aws_sdk_eventbridge::operation::put_rule::PutRuleError::ResourceNotFoundException(
                ResourceNotFoundException::builder()
                    .message("rule gone")
                    .build(),
            )
        });
        let failed_entry = failed_entry_rule();
        let clients = clients(&[&failed_entry], &[&describe, &failing_put]);

        let outcome = handle(&clients, repin_input(vec![5, 6], false))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "FAILED");
        assert_eq!(
            failed_entry.num_calls(),
            0,
            "widen failures retry through the workflow instead of recording"
        );
    }

    #[tokio::test]
    async fn repin_describe_error_surfaces_a_failed_entry_outcome() {
        set_env();
        let failing_describe = mock!(aws_sdk_eventbridge::Client::describe_rule).then_error(|| {
            aws_sdk_eventbridge::operation::describe_rule::DescribeRuleError::ResourceNotFoundException(
                ResourceNotFoundException::builder().message("no such rule").build(),
            )
        });
        let clients = clients(&[], &[&failing_describe]);

        let outcome = handle(&clients, repin_input(vec![5, 6], false))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "FAILED");
        assert_eq!(outcome["error"]["failedStep"], "DescribeSubscriptionRule");
    }

    #[tokio::test]
    async fn repin_without_versions_is_rejected() {
        set_env();
        let describe = describe_state_rule(RuleState::Enabled);
        let put = repin_rule(RuleState::Enabled, &[]);
        let clients = clients(&[], &[&describe, &put]);

        let result = handle(&clients, repin_input(vec![], false)).await;

        assert!(result.is_err());
        assert_eq!(describe.num_calls(), 0);
        assert_eq!(put.num_calls(), 0);
    }

    #[test]
    fn pin_is_the_highest_stable_version() {
        assert_eq!(pin_for(&[5, 6]), 5);
        assert_eq!(pin_for(&[3, 4, 5]), 4);
        assert_eq!(pin_for(&[7]), 7);
    }
}
