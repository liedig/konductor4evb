//! `mode: repin` — promotion-driven re-pinning of an existing subscription
//! rule. The widen phase applies a broadened version list without touching
//! the recorded pin; the narrow phase applies the final list and commits it.
//! Neither phase touches targets, alarms, or entitlements: the rule already
//! materialized and delivery must continue uninterrupted.
//!
//! When the workflow passes upgrade facts (system-event emission enabled),
//! the widen phase first commits the `SubscriptionUpgradeStarted` snapshot
//! and the narrow phase lands the pin (or failure marker) and the terminal
//! snapshot in one conditional write, so the stream record always carries
//! notification facts bound to the business state they describe.

use anyhow::Result;
use aws_sdk_eventbridge::types::RuleState;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use konductor_common::streams::StreamStatus;
use konductor_common::streams_db::{set_env_failed, set_env_pinned_version};
use konductor_common::system_events::store::{
    commit_upgrade_phase, commit_upgrade_terminal, PhaseCommitOutcome, TerminalWrite, UpgradeTarget,
};
use konductor_common::system_events::{
    ContractSelector, ContractSelectors, PartialReason, SnapshotScope, UpgradeEventSnapshot,
    UpgradeOutcome, UpgradePhase, UpgradeTrigger, VersionRange,
};

use crate::config::Config;
use crate::handler::{sdk_cause, Clients, MaterializeInput};
use crate::outcome::{Outcome, StepError};

/// The operation facts a re-pin workflow enumerates per subscription entry.
/// Absent when system-event emission is disabled or for legacy in-flight
/// executions; the step then writes no snapshots.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpgradeFactsInput {
    pub operation_id: String,
    pub trigger: UpgradeTrigger,
    pub generation: u64,
    pub requested_version: i64,
    pub original_pin: i64,
    pub start_metadata_revision: u32,
    #[serde(default)]
    pub terminal_metadata_revision: Option<u32>,
    #[serde(default)]
    pub expected_repin_at: Option<DateTime<Utc>>,
}

pub(crate) async fn repin(
    clients: &Clients,
    config: &Config,
    input: &MaterializeInput,
) -> Result<Outcome> {
    if input.versions.is_empty() {
        anyhow::bail!("mode: repin requires a non-empty versions list");
    }
    // The start snapshot commits before the rule changes: a consumer reading
    // the stream sees the operation announced ahead of any pattern shift.
    if !input.commit_pin {
        if let Some(upgrade) = &input.upgrade {
            let snapshot = start_snapshot(config, input, upgrade)?;
            match commit_upgrade_phase(
                &clients.ddb,
                &config.streams_table,
                &input.producer_domain,
                &input.schema_name,
                &input.consumer_domain,
                &input.environment,
                &snapshot,
            )
            .await?
            {
                PhaseCommitOutcome::Committed | PhaseCommitOutcome::AlreadyCommitted => {}
                PhaseCommitOutcome::Superseded { .. } => {
                    return Ok(outcome(input, "SUPERSEDED", None, None));
                }
            }
        }
    }
    let bus = config.producer_bus(input);
    let state = match describe_rule_state(clients, &bus, input).await {
        Ok(state) => state,
        Err(cause) => {
            record_failure(clients, config, input, "DescribeSubscriptionRule", &cause).await?;
            return Ok(outcome(
                input,
                "FAILED",
                None,
                Some(("DescribeSubscriptionRule", cause)),
            ));
        }
    };
    if let Err(cause) = put_rule_versions(clients, &bus, input, state).await {
        record_failure(clients, config, input, "RepinSubscriptionRule", &cause).await?;
        return Ok(outcome(
            input,
            "FAILED",
            None,
            Some(("RepinSubscriptionRule", cause)),
        ));
    }
    let pinned = pin_for(&input.versions);
    if input.commit_pin {
        match &input.upgrade {
            Some(upgrade) => {
                let snapshot = completed_terminal_snapshot(config, input, upgrade, pinned)?;
                match commit_upgrade_terminal(
                    &clients.ddb,
                    &config.streams_table,
                    &upgrade_target(input),
                    &TerminalWrite::Pin(pinned),
                    &snapshot,
                )
                .await?
                {
                    PhaseCommitOutcome::Committed | PhaseCommitOutcome::AlreadyCommitted => {}
                    PhaseCommitOutcome::Superseded { .. } => {
                        return Ok(outcome(input, "SUPERSEDED", Some(pinned), None));
                    }
                }
            }
            None => {
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
        }
    }
    Ok(outcome(input, "REPINNED", Some(pinned), None))
}

/// A failed narrow-phase re-pin marks the subscription entry `FAILED` with
/// the cause. With upgrade facts the failure marker and the terminal
/// snapshot land in the same write; either way the commit is required and
/// retriable — a failed write surfaces as a step error so the workflow
/// retries instead of losing the failure record. The widen phase retries
/// through the workflow and records nothing.
async fn record_failure(
    clients: &Clients,
    config: &Config,
    input: &MaterializeInput,
    step: &str,
    cause: &str,
) -> Result<()> {
    if !input.commit_pin {
        return Ok(());
    }
    let detail = format!("{step}: {cause}");
    match &input.upgrade {
        Some(upgrade) => {
            let snapshot = failed_terminal_snapshot(config, input, upgrade)?;
            match commit_upgrade_terminal(
                &clients.ddb,
                &config.streams_table,
                &upgrade_target(input),
                &TerminalWrite::Failure(&detail),
                &snapshot,
            )
            .await?
            {
                PhaseCommitOutcome::Superseded { .. } => {
                    tracing::info!("a newer re-pin owns the record; skipping the failure marker");
                }
                PhaseCommitOutcome::Committed | PhaseCommitOutcome::AlreadyCommitted => {}
            }
        }
        None => {
            set_env_failed(
                &clients.ddb,
                &config.streams_table,
                &input.producer_domain,
                &input.schema_name,
                &input.consumer_domain,
                &input.environment,
                &detail,
            )
            .await?;
        }
    }
    Ok(())
}

fn upgrade_target(input: &MaterializeInput) -> UpgradeTarget<'_> {
    UpgradeTarget {
        producer_domain: &input.producer_domain,
        schema_name: &input.schema_name,
        consumer_domain: &input.consumer_domain,
        environment: &input.environment,
    }
}

fn scope<'a>(config: &'a Config, input: &'a MaterializeInput) -> SnapshotScope<'a> {
    SnapshotScope {
        organization: &config.organization,
        consumer_domain: &input.consumer_domain,
        producer_domain: &input.producer_domain,
        schema_name: &input.schema_name,
        detail_type: &input.detail_type,
        environment: &input.environment,
    }
}

fn selector(input: &MaterializeInput, version: i64, metadata_revision: u32) -> ContractSelector {
    ContractSelector {
        producer_domain: input.producer_domain.clone(),
        schema_name: input.schema_name.clone(),
        environment: input.environment.clone(),
        schema_version: version,
        metadata_revision,
    }
}

/// Versions strictly between the original pin and the requested target;
/// never a lookahead range.
fn intermediate_versions(from: i64, requested: i64) -> Option<VersionRange> {
    (requested - from > 1).then(|| VersionRange {
        from_version: from + 1,
        to_version: requested - 1,
    })
}

fn start_snapshot(
    config: &Config,
    input: &MaterializeInput,
    upgrade: &UpgradeFactsInput,
) -> Result<UpgradeEventSnapshot> {
    UpgradeEventSnapshot::new(
        &scope(config, input),
        UpgradePhase::Started,
        &upgrade.operation_id,
        upgrade.trigger,
        upgrade.generation,
        Utc::now(),
        upgrade.original_pin,
        upgrade.requested_version,
        None,
        intermediate_versions(upgrade.original_pin, upgrade.requested_version),
        input.enabled,
        StreamStatus::Active,
        upgrade.expected_repin_at,
        ContractSelectors {
            from: selector(input, upgrade.original_pin, upgrade.start_metadata_revision),
            requested: selector(
                input,
                upgrade.requested_version,
                upgrade.start_metadata_revision,
            ),
            resulting: None,
        },
        None,
        None,
    )
    .map_err(|e| anyhow::anyhow!("invalid upgrade start snapshot: {e}"))
}

/// The terminal snapshot for a landed pin. A pin below the requested
/// version means the replay stopped early: a truthful partial outcome.
fn completed_terminal_snapshot(
    config: &Config,
    input: &MaterializeInput,
    upgrade: &UpgradeFactsInput,
    pinned: i64,
) -> Result<UpgradeEventSnapshot> {
    let terminal_revision = upgrade
        .terminal_metadata_revision
        .unwrap_or(upgrade.start_metadata_revision);
    let outcome = if pinned >= upgrade.requested_version {
        UpgradeOutcome::Completed
    } else {
        UpgradeOutcome::Partial
    };
    let reason = (outcome == UpgradeOutcome::Partial).then_some(PartialReason::ReplayPartial);
    UpgradeEventSnapshot::new(
        &scope(config, input),
        UpgradePhase::Terminal,
        &upgrade.operation_id,
        upgrade.trigger,
        upgrade.generation,
        Utc::now(),
        upgrade.original_pin,
        upgrade.requested_version,
        Some(pinned),
        intermediate_versions(upgrade.original_pin, upgrade.requested_version),
        input.enabled,
        StreamStatus::Active,
        upgrade.expected_repin_at,
        ContractSelectors {
            from: selector(input, upgrade.original_pin, upgrade.start_metadata_revision),
            requested: selector(
                input,
                upgrade.requested_version,
                upgrade.start_metadata_revision,
            ),
            resulting: Some(selector(input, pinned, terminal_revision)),
        },
        Some(outcome),
        reason,
    )
    .map_err(|e| anyhow::anyhow!("invalid upgrade terminal snapshot: {e}"))
}

/// The terminal snapshot for a failed narrow phase: the subscription stays
/// on its original pin, so the resulting selector describes that pin.
fn failed_terminal_snapshot(
    config: &Config,
    input: &MaterializeInput,
    upgrade: &UpgradeFactsInput,
) -> Result<UpgradeEventSnapshot> {
    let terminal_revision = upgrade
        .terminal_metadata_revision
        .unwrap_or(upgrade.start_metadata_revision);
    UpgradeEventSnapshot::new(
        &scope(config, input),
        UpgradePhase::Terminal,
        &upgrade.operation_id,
        upgrade.trigger,
        upgrade.generation,
        Utc::now(),
        upgrade.original_pin,
        upgrade.requested_version,
        Some(upgrade.original_pin),
        intermediate_versions(upgrade.original_pin, upgrade.requested_version),
        input.enabled,
        StreamStatus::Failed,
        upgrade.expected_repin_at,
        ContractSelectors {
            from: selector(input, upgrade.original_pin, upgrade.start_metadata_revision),
            requested: selector(
                input,
                upgrade.requested_version,
                upgrade.start_metadata_revision,
            ),
            resulting: Some(selector(input, upgrade.original_pin, terminal_revision)),
        },
        Some(UpgradeOutcome::Partial),
        Some(PartialReason::RepinFailed),
    )
    .map_err(|e| anyhow::anyhow!("invalid upgrade failure snapshot: {e}"))
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

    fn upgrade_facts() -> Value {
        json!({
            "operationId": "promo-7",
            "trigger": "PROMOTION",
            "generation": 3,
            "requestedVersion": 5,
            "originalPin": 3,
            "startMetadataRevision": 2,
            "terminalMetadataRevision": 4,
            "expectedRepinAt": "2026-02-01T00:05:00Z",
        })
    }

    fn repin_input_with_upgrade(versions: Vec<i64>, commit_pin: bool) -> Value {
        let mut input = repin_input(versions, commit_pin);
        input["upgrade"] = upgrade_facts();
        input
    }

    fn snapshot_attr(
        req: &aws_sdk_dynamodb::operation::update_item::UpdateItemInput,
    ) -> Option<std::collections::HashMap<String, aws_sdk_dynamodb::types::AttributeValue>> {
        req.expression_attribute_values()
            .and_then(|v| v.get(":ueSnap"))
            .and_then(|v| v.as_m().ok())
            .cloned()
    }

    fn snap_str<'a>(
        map: &'a std::collections::HashMap<String, aws_sdk_dynamodb::types::AttributeValue>,
        key: &str,
    ) -> Option<&'a str> {
        map.get(key).and_then(|v| v.as_s().ok()).map(String::as_str)
    }

    fn start_commit_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                req.table_name() == Some("konductor-event-streams")
                    && req.update_expression().is_some_and(|u| {
                        u == "SET upgradeEvents.#ueEnv = :ueSnap, updatedAt = :now"
                    })
                    && snapshot_attr(req).is_some_and(|m| {
                        snap_str(&m, "phase") == Some("STARTED")
                            && snap_str(&m, "operationId") == Some("promo-7")
                            && snap_str(&m, "trigger") == Some("PROMOTION")
                            && snap_str(&m, "subscriptionStatus") == Some("ACTIVE")
                            && !m.contains_key("outcome")
                    })
            })
            .then_output(|| UpdateItemOutput::builder().build())
    }

    fn terminal_pin_commit_rule(expected_outcome: &'static str) -> Rule {
        mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(move |req| {
                req.table_name() == Some("konductor-event-streams")
                    && req.update_expression().is_some_and(|u| {
                        u.contains("environments.#ueEntry.pinnedVersion = :uePin")
                            && u.contains("upgradeEvents.#ueEnv = :ueSnap")
                    })
                    && snapshot_attr(req).is_some_and(|m| {
                        snap_str(&m, "phase") == Some("TERMINAL")
                            && snap_str(&m, "outcome") == Some(expected_outcome)
                    })
            })
            .then_output(|| UpdateItemOutput::builder().build())
    }

    #[tokio::test]
    async fn widen_with_upgrade_facts_commits_the_start_snapshot_before_the_rule() {
        set_env();
        let snapshot_commit = start_commit_rule();
        let describe = describe_state_rule(RuleState::Enabled);
        let put = repin_rule(RuleState::Enabled, &[3, 4, 5, 6]);
        let clients = clients(&[&snapshot_commit], &[&describe, &put]);

        let outcome = handle(&clients, repin_input_with_upgrade(vec![3, 4, 5, 6], false))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "REPINNED");
        assert_eq!(snapshot_commit.num_calls(), 1);
        assert_eq!(put.num_calls(), 1);
    }

    fn conditional_failure() -> Rule {
        mock!(aws_sdk_dynamodb::Client::update_item).then_error(|| {
            aws_sdk_dynamodb::operation::update_item::UpdateItemError::ConditionalCheckFailedException(
                aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                    .message("condition failed")
                    .build(),
            )
        })
    }

    fn stream_item_with_snapshot(
        operation_id: &str,
        generation: u64,
    ) -> std::collections::HashMap<String, aws_sdk_dynamodb::types::AttributeValue> {
        use konductor_common::streams::{EventStream, StreamStatus};
        use konductor_common::streams_db::stream_to_item;
        use konductor_common::system_events::{
            ContractSelector, ContractSelectors, SnapshotScope, UpgradeEventSnapshot, UpgradePhase,
            UpgradeTrigger,
        };

        let scope = SnapshotScope {
            organization: "acme",
            consumer_domain: "shipping",
            producer_domain: "billing",
            schema_name: "acme-billing@OrderNotification",
            detail_type: "OrderNotification",
            environment: "staging",
        };
        let selector = |version| ContractSelector {
            producer_domain: "billing".to_string(),
            schema_name: "acme-billing@OrderNotification".to_string(),
            environment: "staging".to_string(),
            schema_version: version,
            metadata_revision: 2,
        };
        let snapshot = UpgradeEventSnapshot::new(
            &scope,
            UpgradePhase::Started,
            operation_id,
            UpgradeTrigger::Promotion,
            generation,
            chrono::Utc::now(),
            3,
            6,
            None,
            None,
            true,
            StreamStatus::Active,
            None,
            ContractSelectors {
                from: selector(3),
                requested: selector(6),
                resulting: None,
            },
            None,
            None,
        )
        .unwrap();
        let stream = EventStream {
            producer_domain: "billing".to_string(),
            consumer_domain: "shipping".to_string(),
            source: "acme-billing".to_string(),
            detail_type: "OrderNotification".to_string(),
            schema_name: "acme-billing@OrderNotification".to_string(),
            status: StreamStatus::Active,
            error_detail: None,
            environments: std::collections::HashMap::new(),
            upgrade_events: Some(std::collections::HashMap::from([(
                "staging".to_string(),
                snapshot,
            )])),
            request_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            removed_at: None,
            expires_at: None,
        };
        stream_to_item(&stream)
    }

    fn read_rule(
        item: std::collections::HashMap<String, aws_sdk_dynamodb::types::AttributeValue>,
    ) -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        })
    }

    #[tokio::test]
    async fn widen_snapshot_superseded_by_a_newer_repin_skips_the_rule_update() {
        set_env();
        let conflict = conditional_failure();
        let read = read_rule(stream_item_with_snapshot("promo-8", 4));
        let describe = describe_state_rule(RuleState::Enabled);
        let put = repin_rule(RuleState::Enabled, &[3, 4, 5, 6]);
        let clients = clients(&[&conflict, &read], &[&describe, &put]);

        let outcome = handle(&clients, repin_input_with_upgrade(vec![3, 4, 5, 6], false))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "SUPERSEDED");
        assert_eq!(
            put.num_calls(),
            0,
            "a superseded widen must not touch the rule"
        );
    }

    #[tokio::test]
    async fn widen_snapshot_retry_after_an_ambiguous_failure_is_idempotent() {
        set_env();
        let conflict = conditional_failure();
        // The snapshot actually landed: the reread finds this operation's own
        // generation-3 identity, so the widen proceeds instead of looping.
        let read = read_rule(stream_item_with_snapshot("promo-7", 3));
        let describe = describe_state_rule(RuleState::Enabled);
        let put = repin_rule(RuleState::Enabled, &[3, 4, 5, 6]);
        let clients = clients(&[&conflict, &read], &[&describe, &put]);

        let outcome = handle(&clients, repin_input_with_upgrade(vec![3, 4, 5, 6], false))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "REPINNED");
        assert_eq!(put.num_calls(), 1);
    }

    #[tokio::test]
    async fn narrow_with_upgrade_facts_commits_pin_and_terminal_snapshot_atomically() {
        set_env();
        let describe = describe_state_rule(RuleState::Enabled);
        let put = repin_rule(RuleState::Enabled, &[5, 6]);
        let terminal = terminal_pin_commit_rule("COMPLETED");
        let clients = clients(&[&terminal], &[&describe, &put]);

        let outcome = handle(&clients, repin_input_with_upgrade(vec![5, 6], true))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "REPINNED");
        assert_eq!(outcome["pinnedVersion"], 5);
        assert_eq!(terminal.num_calls(), 1, "one write lands pin and snapshot");
    }

    #[tokio::test]
    async fn narrow_replay_partial_records_a_truthful_partial_terminal() {
        set_env();
        let describe = describe_state_rule(RuleState::Enabled);
        let put = repin_rule(RuleState::Enabled, &[4, 5]);
        let terminal = mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                snapshot_attr(req).is_some_and(|m| {
                    snap_str(&m, "phase") == Some("TERMINAL")
                        && snap_str(&m, "outcome") == Some("PARTIAL")
                        && snap_str(&m, "reason") == Some("REPLAY_PARTIAL")
                        && m.get("resultingVersion")
                            .and_then(|v| v.as_n().ok())
                            .map(String::as_str)
                            == Some("4")
                })
            })
            .then_output(|| UpdateItemOutput::builder().build());
        let clients = clients(&[&terminal], &[&describe, &put]);

        let outcome = handle(&clients, repin_input_with_upgrade(vec![4, 5], true))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "REPINNED");
        assert_eq!(terminal.num_calls(), 1);
    }

    #[tokio::test]
    async fn narrow_rule_failure_commits_failure_marker_with_terminal_snapshot() {
        set_env();
        let describe = describe_state_rule(RuleState::Enabled);
        let failing_put = mock!(aws_sdk_eventbridge::Client::put_rule).then_error(|| {
            aws_sdk_eventbridge::operation::put_rule::PutRuleError::ResourceNotFoundException(
                ResourceNotFoundException::builder()
                    .message("rule gone")
                    .build(),
            )
        });
        let failure_commit = mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                req.update_expression().is_some_and(|u| {
                    u.contains("environments.#ueEntry.#ueEntryStatus = :ueFailed")
                        && u.contains("upgradeEvents.#ueEnv = :ueSnap")
                }) && snapshot_attr(req).is_some_and(|m| {
                    snap_str(&m, "phase") == Some("TERMINAL")
                        && snap_str(&m, "outcome") == Some("PARTIAL")
                        && snap_str(&m, "reason") == Some("REPIN_FAILED")
                        && snap_str(&m, "subscriptionStatus") == Some("FAILED")
                        && m.get("resultingVersion")
                            .and_then(|v| v.as_n().ok())
                            .map(String::as_str)
                            == Some("3")
                })
            })
            .then_output(|| UpdateItemOutput::builder().build());
        let clients = clients(&[&failure_commit], &[&describe, &failing_put]);

        let outcome = handle(&clients, repin_input_with_upgrade(vec![5, 6], true))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "FAILED");
        assert_eq!(failure_commit.num_calls(), 1);
    }

    #[tokio::test]
    async fn narrow_failure_record_write_error_fails_the_step_for_retry() {
        set_env();
        let describe = describe_state_rule(RuleState::Enabled);
        let failing_put = mock!(aws_sdk_eventbridge::Client::put_rule).then_error(|| {
            aws_sdk_eventbridge::operation::put_rule::PutRuleError::ResourceNotFoundException(
                ResourceNotFoundException::builder()
                    .message("rule gone")
                    .build(),
            )
        });
        let failing_commit = mock!(aws_sdk_dynamodb::Client::update_item).then_error(|| {
            aws_sdk_dynamodb::operation::update_item::UpdateItemError::generic(
                aws_sdk_dynamodb::error::ErrorMetadata::builder()
                    .message("throttled")
                    .build(),
            )
        });
        let clients = clients(&[&failing_commit], &[&describe, &failing_put]);

        let result = handle(&clients, repin_input_with_upgrade(vec![5, 6], true)).await;

        assert!(
            result.is_err(),
            "the failure record is required: a failed write must retry the step"
        );
    }

    #[tokio::test]
    async fn legacy_narrow_failure_record_write_error_fails_the_step_for_retry() {
        set_env();
        let describe = describe_state_rule(RuleState::Enabled);
        let failing_put = mock!(aws_sdk_eventbridge::Client::put_rule).then_error(|| {
            aws_sdk_eventbridge::operation::put_rule::PutRuleError::ResourceNotFoundException(
                ResourceNotFoundException::builder()
                    .message("rule gone")
                    .build(),
            )
        });
        let failing_commit = mock!(aws_sdk_dynamodb::Client::update_item).then_error(|| {
            aws_sdk_dynamodb::operation::update_item::UpdateItemError::generic(
                aws_sdk_dynamodb::error::ErrorMetadata::builder()
                    .message("throttled")
                    .build(),
            )
        });
        let clients = clients(&[&failing_commit], &[&describe, &failing_put]);

        let result = handle(&clients, repin_input(vec![5, 6], true)).await;

        assert!(
            result.is_err(),
            "the narrow failure record is required even without upgrade facts"
        );
    }
}
