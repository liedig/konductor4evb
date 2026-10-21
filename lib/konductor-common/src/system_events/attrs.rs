//! Attribute (de)serialization between `UpgradeEventSnapshot` and the
//! DynamoDB `upgradeEvents.<env>` map member, used by the event-stream item
//! round-trip and the conditional phase commit.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use chrono::{DateTime, SecondsFormat, Utc};

use crate::dynamodb::s;

use super::identity::UpgradePhase;
use super::models::{
    ContractSelector, ContractSelectors, PartialReason, UpgradeOutcome, UpgradeTrigger,
    VersionRange,
};
use super::snapshot::UpgradeEventSnapshot;

/// The top-level event-stream attribute holding per-environment snapshots.
pub const UPGRADE_EVENTS: &str = "upgradeEvents";

pub(super) fn n(value: i64) -> AttributeValue {
    AttributeValue::N(value.to_string())
}

pub(super) fn nu(value: u64) -> AttributeValue {
    AttributeValue::N(value.to_string())
}

fn get_n(item: &HashMap<String, AttributeValue>, key: &str) -> Option<i64> {
    item.get(key)
        .and_then(|v| v.as_n().ok())
        .and_then(|v| v.parse().ok())
}

fn get_nu(item: &HashMap<String, AttributeValue>, key: &str) -> Option<u64> {
    item.get(key)
        .and_then(|v| v.as_n().ok())
        .and_then(|v| v.parse().ok())
}

fn get_bool(item: &HashMap<String, AttributeValue>, key: &str) -> Option<bool> {
    item.get(key).and_then(|v| v.as_bool().ok()).copied()
}

fn get_s(item: &HashMap<String, AttributeValue>, key: &str) -> Option<String> {
    item.get(key).and_then(|v| v.as_s().ok()).cloned()
}

fn get_dt(item: &HashMap<String, AttributeValue>, key: &str) -> Option<DateTime<Utc>> {
    get_s(item, key).and_then(|v| v.parse().ok())
}

fn millis(dt: &DateTime<Utc>) -> AttributeValue {
    s(&dt.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn selector_to_attr(selector: &ContractSelector) -> AttributeValue {
    let mut map = HashMap::new();
    map.insert("producerDomain".to_string(), s(&selector.producer_domain));
    map.insert("schemaName".to_string(), s(&selector.schema_name));
    map.insert("environment".to_string(), s(&selector.environment));
    map.insert("schemaVersion".to_string(), n(selector.schema_version));
    map.insert(
        "metadataRevision".to_string(),
        nu(u64::from(selector.metadata_revision)),
    );
    AttributeValue::M(map)
}

fn attr_to_selector(item: &HashMap<String, AttributeValue>) -> Option<ContractSelector> {
    Some(ContractSelector {
        producer_domain: get_s(item, "producerDomain")?,
        schema_name: get_s(item, "schemaName")?,
        environment: get_s(item, "environment")?,
        schema_version: get_n(item, "schemaVersion")?,
        metadata_revision: u32::try_from(get_nu(item, "metadataRevision")?).ok()?,
    })
}

fn contracts_to_attr(contracts: &ContractSelectors) -> AttributeValue {
    let mut map = HashMap::new();
    map.insert("from".to_string(), selector_to_attr(&contracts.from));
    map.insert(
        "requested".to_string(),
        selector_to_attr(&contracts.requested),
    );
    if let Some(resulting) = &contracts.resulting {
        map.insert("resulting".to_string(), selector_to_attr(resulting));
    }
    AttributeValue::M(map)
}

fn attr_to_contracts(item: &HashMap<String, AttributeValue>) -> Option<ContractSelectors> {
    let selector = |key: &str| {
        item.get(key)
            .and_then(|v| v.as_m().ok())
            .and_then(attr_to_selector)
    };
    Some(ContractSelectors {
        from: selector("from")?,
        requested: selector("requested")?,
        resulting: selector("resulting"),
    })
}

/// Serializes a snapshot as the `upgradeEvents.<env>` map member.
pub fn upgrade_snapshot_to_attr(snapshot: &UpgradeEventSnapshot) -> AttributeValue {
    let mut map = HashMap::new();
    map.insert("phase".to_string(), s(snapshot.phase.as_str()));
    map.insert("operationId".to_string(), s(&snapshot.operation_id));
    map.insert("trigger".to_string(), s(snapshot.trigger.as_str()));
    map.insert("generation".to_string(), nu(snapshot.generation));
    map.insert(
        "resourceRevision".to_string(),
        nu(snapshot.resource_revision),
    );
    map.insert("idempotencyKey".to_string(), s(&snapshot.idempotency_key));
    map.insert("publishedAt".to_string(), millis(&snapshot.published_at));
    map.insert("fromVersion".to_string(), n(snapshot.from_version));
    map.insert(
        "requestedVersion".to_string(),
        n(snapshot.requested_version),
    );
    if let Some(version) = snapshot.resulting_version {
        map.insert("resultingVersion".to_string(), n(version));
    }
    if let Some(range) = &snapshot.intermediate_versions {
        let mut range_map = HashMap::new();
        range_map.insert("fromVersion".to_string(), n(range.from_version));
        range_map.insert("toVersion".to_string(), n(range.to_version));
        map.insert(
            "intermediateVersions".to_string(),
            AttributeValue::M(range_map),
        );
    }
    map.insert(
        "enabled".to_string(),
        AttributeValue::Bool(snapshot.enabled),
    );
    map.insert(
        "subscriptionStatus".to_string(),
        s(snapshot.subscription_status.as_str()),
    );
    if let Some(expected) = &snapshot.expected_repin_at {
        map.insert("expectedRepinAt".to_string(), millis(expected));
    }
    map.insert(
        "contracts".to_string(),
        contracts_to_attr(&snapshot.contracts),
    );
    if let Some(outcome) = &snapshot.outcome {
        map.insert("outcome".to_string(), s(outcome.as_str()));
    }
    if let Some(reason) = &snapshot.reason {
        map.insert("reason".to_string(), s(reason.as_str()));
    }
    AttributeValue::M(map)
}

/// Parses an `upgradeEvents.<env>` map member. Unknown or incomplete
/// snapshots return `None` so legacy records remain readable.
pub fn attr_to_upgrade_snapshot(
    item: &HashMap<String, AttributeValue>,
) -> Option<UpgradeEventSnapshot> {
    let phase = match get_s(item, "phase")?.as_str() {
        "STARTED" => UpgradePhase::Started,
        "TERMINAL" => UpgradePhase::Terminal,
        _ => return None,
    };
    let trigger = match get_s(item, "trigger")?.as_str() {
        "PROMOTION" => UpgradeTrigger::Promotion,
        "AUTHORING" => UpgradeTrigger::Authoring,
        _ => return None,
    };
    let intermediate_versions = match item.get("intermediateVersions") {
        Some(value) => {
            let range = value.as_m().ok()?;
            Some(VersionRange {
                from_version: get_n(range, "fromVersion")?,
                to_version: get_n(range, "toVersion")?,
            })
        }
        None => None,
    };
    Some(UpgradeEventSnapshot {
        phase,
        operation_id: get_s(item, "operationId")?,
        trigger,
        generation: get_nu(item, "generation")?,
        resource_revision: get_nu(item, "resourceRevision")?,
        idempotency_key: get_s(item, "idempotencyKey")?,
        published_at: get_dt(item, "publishedAt")?,
        from_version: get_n(item, "fromVersion")?,
        requested_version: get_n(item, "requestedVersion")?,
        resulting_version: get_n(item, "resultingVersion"),
        intermediate_versions,
        enabled: get_bool(item, "enabled")?,
        subscription_status: get_s(item, "subscriptionStatus")?.parse().ok()?,
        expected_repin_at: get_dt(item, "expectedRepinAt"),
        contracts: attr_to_contracts(item.get("contracts")?.as_m().ok()?)?,
        outcome: get_s(item, "outcome").and_then(|o| match o.as_str() {
            "COMPLETED" => Some(UpgradeOutcome::Completed),
            "PARTIAL" => Some(UpgradeOutcome::Partial),
            _ => None,
        }),
        reason: get_s(item, "reason").and_then(|r| match r.as_str() {
            "REPLAY_PARTIAL" => Some(PartialReason::ReplayPartial),
            "REPIN_FAILED" => Some(PartialReason::RepinFailed),
            _ => None,
        }),
    })
}
