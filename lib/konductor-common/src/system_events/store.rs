//! DynamoDB storage for committed upgrade phase snapshots: the conditional
//! phase commit the re-pin lifecycle uses to bind notification facts to
//! business state. Attribute mapping lives in `attrs`.

use anyhow::{anyhow, Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use chrono::Utc;

use crate::dynamodb::s;
use crate::streams::EventStream;

use super::attrs::{n, nu};
use super::snapshot::UpgradeEventSnapshot;

pub use super::attrs::{attr_to_upgrade_snapshot, upgrade_snapshot_to_attr, UPGRADE_EVENTS};

/// How a conditional phase commit resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseCommitOutcome {
    /// The snapshot was written.
    Committed,
    /// The exact same phase identity was already committed; the original
    /// snapshot stands and this attempt changed nothing.
    AlreadyCommitted,
    /// A newer phase is already recorded; the caller's operation is
    /// superseded and must not regress the record.
    Superseded { existing_revision: u64 },
}

/// The condition fragment, attribute names, and attribute values ensuring
/// an older phase never overwrites a newer committed snapshot.
/// Placeholders are `ue`-prefixed so they cannot collide with the caller's
/// own names and values.
#[derive(Debug)]
pub struct UpgradeGuard {
    pub condition: String,
    pub names: Vec<(String, String)>,
    pub values: Vec<(String, AttributeValue)>,
}

pub fn upgrade_guard(environment: &str, revision: u64) -> UpgradeGuard {
    UpgradeGuard {
        condition: format!(
            "attribute_not_exists({UPGRADE_EVENTS}.#ueEnv.resourceRevision) OR {UPGRADE_EVENTS}.#ueEnv.resourceRevision < :ueRev"
        ),
        names: vec![("#ueEnv".to_string(), environment.to_string())],
        values: vec![(":ueRev".to_string(), nu(revision))],
    }
}

/// The event-stream record and environment entry a phase commit targets.
pub struct UpgradeTarget<'a> {
    pub producer_domain: &'a str,
    pub schema_name: &'a str,
    pub consumer_domain: &'a str,
    pub environment: &'a str,
}

/// Commits a phase snapshot on its own (the start phase). Terminal commits
/// compose the same guard with the pin/failure write in one `UpdateItem`.
/// Beyond the revision guard the condition re-checks eligibility at write
/// time: the environment entry must still exist and be `ACTIVE`, and the
/// stream must not be removed. A conditional-check failure rereads the
/// record: the same phase identity is `AlreadyCommitted`, a newer revision
/// or an ineligible entry is `Superseded`, and anything else is a genuine
/// conflict the workflow must retry or surface.
pub async fn commit_upgrade_phase(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    producer_domain: &str,
    schema_name: &str,
    consumer_domain: &str,
    environment: &str,
    snapshot: &UpgradeEventSnapshot,
) -> Result<PhaseCommitOutcome> {
    let guard = upgrade_guard(environment, snapshot.resource_revision);
    let mut update = ddb
        .update_item()
        .table_name(table)
        .key(crate::streams_db::PK, s(producer_domain))
        .key(
            crate::streams_db::SK,
            s(&EventStream::sort_key(schema_name, consumer_domain)),
        )
        .update_expression(format!(
            "SET {UPGRADE_EVENTS}.#ueEnv = :ueSnap, updatedAt = :now"
        ))
        .condition_expression(format!(
            "attribute_exists(environments.#ueEntry) \
             AND environments.#ueEntry.#ueEntryStatus = :ueActive \
             AND #ueTopStatus <> :ueRemoved \
             AND ({})",
            guard.condition
        ))
        .expression_attribute_names("#ueEntry", environment)
        .expression_attribute_names("#ueEntryStatus", "status")
        .expression_attribute_names("#ueTopStatus", "status")
        .expression_attribute_values(
            ":ueActive",
            s(crate::streams::StreamStatus::Active.as_str()),
        )
        .expression_attribute_values(
            ":ueRemoved",
            s(crate::streams::StreamStatus::Removed.as_str()),
        )
        .expression_attribute_values(":ueSnap", upgrade_snapshot_to_attr(snapshot))
        .expression_attribute_values(":now", s(&Utc::now().to_rfc3339()));
    for (placeholder, name) in guard.names {
        update = update.expression_attribute_names(placeholder, name);
    }
    for (placeholder, value) in guard.values {
        update = update.expression_attribute_values(placeholder, value);
    }
    match update.send().await {
        Ok(_) => Ok(PhaseCommitOutcome::Committed),
        Err(error) => {
            let service_error = error.into_service_error();
            if !matches!(
                service_error,
                aws_sdk_dynamodb::operation::update_item::UpdateItemError::ConditionalCheckFailedException(_)
            ) {
                return Err(anyhow::Error::new(service_error))
                    .context("failed to commit upgrade phase snapshot");
            }
            resolve_conflict(
                ddb,
                table,
                &UpgradeTarget {
                    producer_domain,
                    schema_name,
                    consumer_domain,
                    environment,
                },
                snapshot,
                true,
            )
            .await
        }
    }
}

/// What the terminal phase commit writes next to the snapshot in the same
/// `UpdateItem`.
#[derive(Debug)]
pub enum TerminalWrite<'a> {
    /// The narrow phase succeeded; land the final recorded pin.
    Pin(i64),
    /// The narrow phase failed; flip the entry to `FAILED` with the cause.
    Failure(&'a str),
}

/// The terminal phase commit: the pin (or failure marker) and the terminal
/// snapshot land atomically, guarded by the snapshot revision so a newer
/// re-pin is never regressed. The pin write additionally requires the entry
/// to still be `ACTIVE`; the failure write accepts any entry state as long
/// as the subscription still exists.
pub async fn commit_upgrade_terminal(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    target: &UpgradeTarget<'_>,
    write: &TerminalWrite<'_>,
    snapshot: &UpgradeEventSnapshot,
) -> Result<PhaseCommitOutcome> {
    let environment = target.environment;
    let guard = upgrade_guard(environment, snapshot.resource_revision);
    let (set_expression, require_active) = match write {
        TerminalWrite::Pin(_) => (
            "SET environments.#ueEntry.pinnedVersion = :uePin, \
             upgradeEvents.#ueEnv = :ueSnap, updatedAt = :now",
            true,
        ),
        TerminalWrite::Failure(_) => (
            "SET environments.#ueEntry.#ueEntryStatus = :ueFailed, \
             environments.#ueEntry.errorDetail = :ueDetail, \
             upgradeEvents.#ueEnv = :ueSnap, updatedAt = :now",
            false,
        ),
    };
    let eligibility = if require_active {
        "attribute_exists(environments.#ueEntry) \
         AND environments.#ueEntry.#ueEntryStatus = :ueActive \
         AND #ueTopStatus <> :ueRemoved"
    } else {
        "attribute_exists(environments.#ueEntry) AND #ueTopStatus <> :ueRemoved"
    };
    let mut update = ddb
        .update_item()
        .table_name(table)
        .key(crate::streams_db::PK, s(target.producer_domain))
        .key(
            crate::streams_db::SK,
            s(&EventStream::sort_key(
                target.schema_name,
                target.consumer_domain,
            )),
        )
        .update_expression(set_expression)
        .condition_expression(format!("{eligibility} AND ({})", guard.condition))
        .expression_attribute_names("#ueEntry", environment)
        .expression_attribute_names("#ueEntryStatus", "status")
        .expression_attribute_names("#ueTopStatus", "status")
        .expression_attribute_values(
            ":ueRemoved",
            s(crate::streams::StreamStatus::Removed.as_str()),
        )
        .expression_attribute_values(":ueSnap", upgrade_snapshot_to_attr(snapshot))
        .expression_attribute_values(":now", s(&Utc::now().to_rfc3339()));
    match write {
        TerminalWrite::Pin(pin) => {
            update = update
                .expression_attribute_values(":uePin", n(*pin))
                .expression_attribute_values(
                    ":ueActive",
                    s(crate::streams::StreamStatus::Active.as_str()),
                );
        }
        TerminalWrite::Failure(detail) => {
            update = update
                .expression_attribute_values(
                    ":ueFailed",
                    s(crate::streams::StreamStatus::Failed.as_str()),
                )
                .expression_attribute_values(":ueDetail", s(detail));
        }
    }
    for (placeholder, name) in guard.names {
        update = update.expression_attribute_names(placeholder, name);
    }
    for (placeholder, value) in guard.values {
        update = update.expression_attribute_values(placeholder, value);
    }
    match update.send().await {
        Ok(_) => Ok(PhaseCommitOutcome::Committed),
        Err(error) => {
            let service_error = error.into_service_error();
            if !matches!(
                service_error,
                aws_sdk_dynamodb::operation::update_item::UpdateItemError::ConditionalCheckFailedException(_)
            ) {
                return Err(anyhow::Error::new(service_error))
                    .context("failed to commit terminal upgrade phase");
            }
            resolve_conflict(ddb, table, target, snapshot, require_active).await
        }
    }
}

async fn resolve_conflict(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    target: &UpgradeTarget<'_>,
    snapshot: &UpgradeEventSnapshot,
    require_active_entry: bool,
) -> Result<PhaseCommitOutcome> {
    let environment = target.environment;
    let stream = crate::streams_db::get_stream_consistent(
        ddb,
        table,
        target.producer_domain,
        target.schema_name,
        target.consumer_domain,
    )
    .await?;
    let existing = stream
        .as_ref()
        .and_then(|st| st.upgrade_events.as_ref())
        .and_then(|events| events.get(environment));
    match existing {
        Some(committed) if committed.idempotency_key == snapshot.idempotency_key => {
            Ok(PhaseCommitOutcome::AlreadyCommitted)
        }
        Some(committed) if committed.resource_revision > snapshot.resource_revision => {
            Ok(PhaseCommitOutcome::Superseded {
                existing_revision: committed.resource_revision,
            })
        }
        Some(committed) if committed.resource_revision == snapshot.resource_revision => {
            // Same revision with a different phase identity is a corruption
            // signal, never an eligibility outcome.
            Err(anyhow!(
                "conflicting upgrade phase at revision {} for {environment}; committed key {} != {}",
                committed.resource_revision,
                committed.idempotency_key,
                snapshot.idempotency_key
            ))
        }
        _ => {
            let eligible = stream.as_ref().is_some_and(|st| {
                st.status != crate::streams::StreamStatus::Removed
                    && st.environments.get(environment).is_some_and(|entry| {
                        !require_active_entry
                            || entry.status == crate::streams::StreamStatus::Active
                    })
            });
            if !eligible {
                // The subscription vanished, was removed, or failed between
                // enumeration and commit; the operation no longer applies,
                // whatever snapshot it left behind.
                return Ok(PhaseCommitOutcome::Superseded {
                    existing_revision: existing.map_or(0, |c| c.resource_revision),
                });
            }
            match existing {
                Some(_) => unreachable!("older committed revisions cannot reject a newer write"),
                None => Err(anyhow!(
                    "phase commit for {environment} was rejected without an existing snapshot; retry"
                )),
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use aws_smithy_mocks::{mock, mock_client, Rule};
    use chrono::TimeZone;

    use crate::streams::StreamStatus;
    use crate::streams_db::{item_to_stream, stream_to_item};
    use crate::system_events::{
        ContractSelector, ContractSelectors, SnapshotScope, UpgradeOutcome, UpgradePhase,
        UpgradeTrigger,
    };

    const TABLE: &str = "event-streams";

    fn scope() -> SnapshotScope<'static> {
        SnapshotScope {
            organization: "acme",
            consumer_domain: "checkout",
            producer_domain: "orders",
            schema_name: "acme-orders@OrderNotification",
            detail_type: "OrderNotification",
            environment: "prod",
        }
    }

    fn selectors(with_resulting: bool) -> ContractSelectors {
        let selector = |version| ContractSelector {
            producer_domain: "orders".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            environment: "prod".to_string(),
            schema_version: version,
            metadata_revision: 0,
        };
        ContractSelectors {
            from: selector(3),
            requested: selector(4),
            resulting: with_resulting.then(|| selector(4)),
        }
    }

    fn snapshot(phase: UpgradePhase, generation: u64) -> UpgradeEventSnapshot {
        let mut snapshot = UpgradeEventSnapshot::new(
            &scope(),
            phase,
            "promotion-42",
            UpgradeTrigger::Promotion,
            generation,
            Utc.timestamp_millis_opt(1785407400123).unwrap(),
            3,
            4,
            None,
            None,
            true,
            StreamStatus::Active,
            None,
            selectors(matches!(phase, UpgradePhase::Terminal)),
            None,
            None,
        )
        .unwrap();
        if matches!(phase, UpgradePhase::Terminal) {
            snapshot.resulting_version = Some(4);
            snapshot.outcome = Some(UpgradeOutcome::Completed);
        }
        snapshot
    }

    fn stream_with(environment: &str, entry: Option<&UpgradeEventSnapshot>) -> EventStream {
        let mut stream = EventStream {
            producer_domain: "orders".to_string(),
            consumer_domain: "checkout".to_string(),
            source: "acme-orders".to_string(),
            detail_type: "OrderNotification".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            status: StreamStatus::Active,
            error_detail: None,
            environments: HashMap::new(),
            upgrade_events: None,
            request_id: None,
            created_at: Utc.timestamp_millis_opt(1785400000000).unwrap(),
            updated_at: Utc.timestamp_millis_opt(1785400000000).unwrap(),
            removed_at: None,
            expires_at: None,
        };
        if let Some(entry) = entry {
            stream.upgrade_events = Some(HashMap::from([(environment.to_string(), entry.clone())]));
        }
        stream
    }

    fn active_env(stream: &mut EventStream, environment: &str) {
        stream.environments.insert(
            environment.to_string(),
            crate::streams::EnvStream {
                pinned_version: Some(3),
                rule_name: "checkout-OrderNotification".to_string(),
                rule_arn: None,
                enabled: true,
                status: StreamStatus::Active,
                error_detail: None,
            },
        );
    }

    fn target() -> UpgradeTarget<'static> {
        UpgradeTarget {
            producer_domain: "orders",
            schema_name: "acme-orders@OrderNotification",
            consumer_domain: "checkout",
            environment: "prod",
        }
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

    fn reads(stream: &EventStream) -> Rule {
        let item = stream_to_item(stream);
        mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        })
    }

    fn clients(rules: &[&Rule]) -> aws_sdk_dynamodb::Client {
        mock_client!(aws_sdk_dynamodb, rules)
    }

    #[test]
    fn snapshot_attr_round_trips_for_both_phases() {
        for phase in [UpgradePhase::Started, UpgradePhase::Terminal] {
            let original = snapshot(phase, 3);
            let attr = upgrade_snapshot_to_attr(&original);
            let map = attr.as_m().unwrap();
            let back = attr_to_upgrade_snapshot(map).unwrap();
            assert_eq!(back, original);
        }
    }

    #[test]
    fn snapshot_attr_requires_core_fields() {
        let attr = upgrade_snapshot_to_attr(&snapshot(UpgradePhase::Started, 3));
        let mut map = attr.as_m().unwrap().clone();
        map.remove("resourceRevision");
        assert!(attr_to_upgrade_snapshot(&map).is_none());
    }

    #[test]
    fn item_round_trip_preserves_snapshots_and_unrelated_environments() {
        let mut stream = stream_with("prod", Some(&snapshot(UpgradePhase::Started, 3)));
        stream
            .upgrade_events
            .as_mut()
            .unwrap()
            .insert("dev".to_string(), snapshot(UpgradePhase::Terminal, 2));
        let item = stream_to_item(&stream);
        assert!(item.contains_key(UPGRADE_EVENTS));
        let back = item_to_stream(&item).unwrap();
        assert_eq!(back.upgrade_events, stream.upgrade_events);
    }

    #[test]
    fn legacy_item_without_snapshots_reads_as_none() {
        let stream = stream_with("prod", None);
        let item = stream_to_item(&stream);
        assert!(!item.contains_key(UPGRADE_EVENTS));
        let back = item_to_stream(&item).unwrap();
        assert_eq!(back.upgrade_events, None);
    }

    #[tokio::test]
    async fn commit_writes_snapshot_with_revision_guard() {
        let entry = snapshot(UpgradePhase::Started, 3);
        let write = mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                req.condition_expression()
                    .is_some_and(|c| c.contains("upgradeEvents.#ueEnv.resourceRevision < :ueRev"))
                    && req
                        .update_expression()
                        .is_some_and(|u| u.contains("SET upgradeEvents.#ueEnv = :ueSnap"))
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::update_item::UpdateItemOutput::builder().build()
            });
        let ddb = clients(&[&write]);

        let outcome = commit_upgrade_phase(
            &ddb,
            TABLE,
            "orders",
            "acme-orders@OrderNotification",
            "checkout",
            "prod",
            &entry,
        )
        .await
        .unwrap();
        assert_eq!(outcome, PhaseCommitOutcome::Committed);
    }

    #[tokio::test]
    async fn duplicate_commit_of_same_phase_is_already_committed() {
        let entry = snapshot(UpgradePhase::Started, 3);
        let conflict = conditional_failure();
        let read = reads(&stream_with("prod", Some(&entry)));
        let ddb = clients(&[&conflict, &read]);

        let outcome = commit_upgrade_phase(
            &ddb,
            TABLE,
            "orders",
            "acme-orders@OrderNotification",
            "checkout",
            "prod",
            &entry,
        )
        .await
        .unwrap();
        assert_eq!(outcome, PhaseCommitOutcome::AlreadyCommitted);
    }

    #[tokio::test]
    async fn newer_committed_revision_supersedes_the_attempt() {
        let older = snapshot(UpgradePhase::Started, 2);
        let newer = snapshot(UpgradePhase::Terminal, 2);
        let conflict = conditional_failure();
        let read = reads(&stream_with("prod", Some(&newer)));
        let ddb = clients(&[&conflict, &read]);

        let outcome = commit_upgrade_phase(
            &ddb,
            TABLE,
            "orders",
            "acme-orders@OrderNotification",
            "checkout",
            "prod",
            &older,
        )
        .await
        .unwrap();
        assert_eq!(
            outcome,
            PhaseCommitOutcome::Superseded {
                existing_revision: newer.resource_revision
            }
        );
    }

    #[tokio::test]
    async fn conflicting_identity_at_same_revision_is_an_error() {
        let entry = snapshot(UpgradePhase::Terminal, 2);
        let mut competing = snapshot(UpgradePhase::Terminal, 2);
        competing.idempotency_key = "system-event/v1/other".to_string();
        let conflict = conditional_failure();
        let read = reads(&stream_with("prod", Some(&competing)));
        let ddb = clients(&[&conflict, &read]);

        let result = commit_upgrade_phase(
            &ddb,
            TABLE,
            "orders",
            "acme-orders@OrderNotification",
            "checkout",
            "prod",
            &entry,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejection_without_an_existing_snapshot_is_retryable_error() {
        let entry = snapshot(UpgradePhase::Started, 3);
        let conflict = conditional_failure();
        let mut stream = stream_with("prod", None);
        active_env(&mut stream, "prod");
        let read = reads(&stream);
        let ddb = clients(&[&conflict, &read]);

        let result = commit_upgrade_phase(
            &ddb,
            TABLE,
            "orders",
            "acme-orders@OrderNotification",
            "checkout",
            "prod",
            &entry,
        )
        .await;
        let error = result.unwrap_err().to_string();
        assert!(error.contains("retry"), "unexpected error: {error}");
    }

    #[tokio::test]
    async fn non_conditional_write_errors_propagate() {
        let entry = snapshot(UpgradePhase::Started, 3);
        let throttled = mock!(aws_sdk_dynamodb::Client::update_item).then_error(|| {
            aws_sdk_dynamodb::operation::update_item::UpdateItemError::generic(
                aws_sdk_dynamodb::error::ErrorMetadata::builder()
                    .message("throttled")
                    .build(),
            )
        });
        let ddb = clients(&[&throttled]);

        let result = commit_upgrade_phase(
            &ddb,
            TABLE,
            "orders",
            "acme-orders@OrderNotification",
            "checkout",
            "prod",
            &entry,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn removed_stream_rejection_is_superseded_not_retried() {
        let entry = snapshot(UpgradePhase::Started, 3);
        let conflict = conditional_failure();
        let mut stream = stream_with("prod", None);
        stream.status = StreamStatus::Removed;
        let read = reads(&stream);
        let ddb = clients(&[&conflict, &read]);

        let outcome = commit_upgrade_phase(
            &ddb,
            TABLE,
            "orders",
            "acme-orders@OrderNotification",
            "checkout",
            "prod",
            &entry,
        )
        .await
        .unwrap();
        assert!(matches!(outcome, PhaseCommitOutcome::Superseded { .. }));
    }

    #[tokio::test]
    async fn ineligible_entry_with_an_older_snapshot_is_superseded_not_an_error() {
        let older = snapshot(UpgradePhase::Started, 1);
        let entry = snapshot(UpgradePhase::Terminal, 1);
        let conflict = conditional_failure();
        let mut stream = stream_with("prod", Some(&older));
        stream.status = StreamStatus::Removed;
        let read = reads(&stream);
        let ddb = clients(&[&conflict, &read]);

        let outcome = commit_upgrade_phase(
            &ddb,
            TABLE,
            "orders",
            "acme-orders@OrderNotification",
            "checkout",
            "prod",
            &entry,
        )
        .await
        .unwrap();
        assert_eq!(
            outcome,
            PhaseCommitOutcome::Superseded {
                existing_revision: older.resource_revision
            }
        );
    }
    #[tokio::test]
    async fn missing_entry_rejection_is_superseded_not_retried() {
        let entry = snapshot(UpgradePhase::Started, 3);
        let conflict = conditional_failure();
        let read = reads(&stream_with("prod", None));
        let ddb = clients(&[&conflict, &read]);

        let outcome = commit_upgrade_phase(
            &ddb,
            TABLE,
            "orders",
            "acme-orders@OrderNotification",
            "checkout",
            "prod",
            &entry,
        )
        .await
        .unwrap();
        assert!(matches!(outcome, PhaseCommitOutcome::Superseded { .. }));
    }

    #[tokio::test]
    async fn terminal_pin_commit_lands_pin_and_snapshot_atomically() {
        let entry = snapshot(UpgradePhase::Terminal, 3);
        let write = mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                req.update_expression().is_some_and(|u| {
                    u.contains("environments.#ueEntry.pinnedVersion = :uePin")
                        && u.contains("upgradeEvents.#ueEnv = :ueSnap")
                }) && req.condition_expression().is_some_and(|c| {
                    c.contains("environments.#ueEntry.#ueEntryStatus = :ueActive")
                        && c.contains("#ueTopStatus <> :ueRemoved")
                        && c.contains("resourceRevision < :ueRev")
                }) && req
                    .expression_attribute_values()
                    .and_then(|v| v.get(":uePin"))
                    .and_then(|v| v.as_n().ok())
                    .map(String::as_str)
                    == Some("4")
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::update_item::UpdateItemOutput::builder().build()
            });
        let ddb = clients(&[&write]);

        let outcome =
            commit_upgrade_terminal(&ddb, TABLE, &target(), &TerminalWrite::Pin(4), &entry)
                .await
                .unwrap();
        assert_eq!(outcome, PhaseCommitOutcome::Committed);
        assert_eq!(write.num_calls(), 1);
    }

    #[tokio::test]
    async fn terminal_failure_commit_marks_the_entry_failed_with_the_snapshot() {
        let entry = snapshot(UpgradePhase::Terminal, 3);
        let write = mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                req.update_expression().is_some_and(|u| {
                    u.contains("environments.#ueEntry.#ueEntryStatus = :ueFailed")
                        && u.contains("environments.#ueEntry.errorDetail = :ueDetail")
                        && u.contains("upgradeEvents.#ueEnv = :ueSnap")
                }) && req
                    .expression_attribute_values()
                    .and_then(|v| v.get(":ueDetail"))
                    .and_then(|v| v.as_s().ok())
                    .map(String::as_str)
                    == Some("RepinSubscriptionRule: boom")
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::update_item::UpdateItemOutput::builder().build()
            });
        let ddb = clients(&[&write]);

        let outcome = commit_upgrade_terminal(
            &ddb,
            TABLE,
            &target(),
            &TerminalWrite::Failure("RepinSubscriptionRule: boom"),
            &entry,
        )
        .await
        .unwrap();
        assert_eq!(outcome, PhaseCommitOutcome::Committed);
    }

    #[tokio::test]
    async fn duplicate_terminal_commit_is_already_committed() {
        let entry = snapshot(UpgradePhase::Terminal, 3);
        let conflict = conditional_failure();
        let read = reads(&stream_with("prod", Some(&entry)));
        let ddb = clients(&[&conflict, &read]);

        let outcome =
            commit_upgrade_terminal(&ddb, TABLE, &target(), &TerminalWrite::Pin(4), &entry)
                .await
                .unwrap();
        assert_eq!(outcome, PhaseCommitOutcome::AlreadyCommitted);
    }
}
