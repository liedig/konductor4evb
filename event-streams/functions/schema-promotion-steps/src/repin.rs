//! The re-pin coordination steps of a promotion: `widen` records the
//! broadened pattern on the `REPIN` item and returns the affected
//! subscription entries; `narrow` commits the final pin after the drain on
//! its own generation only. Shared plumbing lives here; the steps are
//! `widen.rs` and `narrow.rs`.

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use konductor_common::dynamodb::s;
use konductor_common::promotion::{promotion_pk, RepinState, REPIN_SK};
use konductor_common::promotion_store::repin_from_item;
use konductor_common::streams::StreamStatus;
use konductor_common::streams_db::query_streams_by_schema;
use konductor_common::system_events::UpgradeTrigger;

use crate::store::{full_schema_name, promotions_table, Clients};

mod narrow;
mod widen;

pub(crate) use narrow::narrow;
pub(crate) use widen::widen;

/// One affected subscription's env entry, in the shape the materialize
/// function's `mode: repin` consumes.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepinEntry {
    pub producer_domain: String,
    pub consumer_domain: String,
    pub source: String,
    pub detail_type: String,
    pub schema_name: String,
    pub rule_name: String,
    pub environment: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_version: Option<i64>,
    /// Facts for the upgrade phase snapshot the materialize step commits.
    /// Always serialized (`null` when emission is disabled) so JSONata
    /// passthrough in the state machines stays well-formed.
    pub upgrade: Option<EntryUpgrade>,
}

/// Per-entry facts for one phase commit. Everything except `original_pin`
/// is identical across a step call's entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntryUpgrade {
    pub operation_id: String,
    pub trigger: UpgradeTrigger,
    pub generation: u64,
    pub requested_version: i64,
    pub original_pin: i64,
    pub start_metadata_revision: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_metadata_revision: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_repin_at: Option<String>,
}

/// The step-level inputs every entry's upgrade facts share.
#[derive(Debug, Clone)]
pub(crate) struct StepFacts {
    pub operation_id: String,
    pub trigger: UpgradeTrigger,
    pub generation: u64,
    pub requested_version: i64,
    pub start_metadata_revision: u32,
    pub terminal_metadata_revision: Option<u32>,
    pub expected_repin_at: Option<DateTime<Utc>>,
}

/// System-event emission is a deployment gate: disabled means repin entries
/// carry no upgrade facts and the materialize step behaves exactly as
/// before. Steps of legacy in-flight executions (no `trigger` field) skip
/// emission the same way.
pub(crate) fn emission_enabled() -> bool {
    std::env::var("SYSTEM_EVENT_EMISSION_ENABLED").as_deref() == Ok("true")
}

/// The environment's authoritative active metadata revision, including the
/// valid legacy revision 0. Read only when emission is enabled.
pub(crate) async fn active_metadata_revision(clients: &Clients, environment: &str) -> Result<u32> {
    let table = std::env::var("METADATA_TABLE").context("METADATA_TABLE not set")?;
    let store =
        konductor_common::metadata::revisions::MetadataStore::new(clients.ddb.clone(), table);
    let pointer = store
        .get_environment_pointer(environment)
        .await
        .map_err(|e| anyhow::anyhow!("metadata pointer read failed: {e}"))?;
    Ok(pointer
        .ok_or_else(|| anyhow::anyhow!("no metadata pointer for {environment}"))?
        .active_revision)
}

/// Attaches upgrade facts to an enumerated entry. Entries without a
/// recorded pin are awaiting promotion and never carry facts.
pub(crate) fn with_facts(entry: &mut RepinEntry, facts: Option<&StepFacts>) {
    entry.upgrade = match (facts, entry.pinned_version) {
        (Some(facts), Some(original_pin)) => Some(EntryUpgrade {
            operation_id: facts.operation_id.clone(),
            trigger: facts.trigger,
            generation: facts.generation,
            requested_version: facts.requested_version,
            original_pin,
            start_metadata_revision: facts.start_metadata_revision,
            terminal_metadata_revision: facts.terminal_metadata_revision,
            expected_repin_at: facts.expected_repin_at.map(|t| rfc3339(&t)),
        }),
        _ => None,
    };
}

/// `[floor ..= target] ∪ {target + 1}` — the contiguous widened range plus
/// the lookahead slot, so producers publishing the next version mid-promotion
/// keep matching.
fn pattern(floor: i64, target: i64) -> Vec<i64> {
    (floor..=target + 1).collect()
}

fn rfc3339(t: &chrono::DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn is_conditional_failure(e: &anyhow::Error) -> bool {
    e.downcast_ref::<aws_sdk_dynamodb::operation::put_item::PutItemError>()
        .is_some_and(|e| e.is_conditional_check_failed_exception())
        || e.downcast_ref::<aws_sdk_dynamodb::operation::update_item::UpdateItemError>()
            .is_some_and(|e| e.is_conditional_check_failed_exception())
}

async fn read_repin_item(
    clients: &Clients,
    domain: &str,
    schema_name: &str,
    environment: &str,
) -> Result<Option<RepinState>> {
    let output = clients
        .ddb
        .get_item()
        .table_name(promotions_table()?)
        .key("pk", s(&promotion_pk(domain, schema_name, environment)))
        .key("sk", s(REPIN_SK))
        .consistent_read(true)
        .send()
        .await
        .map_err(|e| anyhow::Error::new(e.into_service_error()))
        .context("read REPIN item failed")?;
    output
        .item
        .map(|item| repin_from_item(&item).ok_or_else(|| anyhow::anyhow!("malformed REPIN item")))
        .transpose()
}

/// Every ACTIVE env entry for this schema in this environment — enabled or
/// disabled, since a disabled rule still carries the pin it will resume with.
async fn enumerate(
    clients: &Clients,
    domain: &str,
    event_name: &str,
    environment: &str,
    facts: Option<&StepFacts>,
) -> Result<Vec<RepinEntry>> {
    let table = std::env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let full = full_schema_name(domain, event_name)?;
    let streams = query_streams_by_schema(&clients.ddb, &table, domain, &full).await?;
    let mut entries = Vec::new();
    for stream in streams {
        let Some(env) = stream.environments.get(environment) else {
            continue;
        };
        if env.status != StreamStatus::Active {
            continue;
        }
        let mut entry = RepinEntry {
            producer_domain: stream.producer_domain.clone(),
            consumer_domain: stream.consumer_domain.clone(),
            source: stream.source.clone(),
            detail_type: stream.detail_type.clone(),
            schema_name: stream.schema_name.clone(),
            rule_name: env.rule_name.clone(),
            environment: environment.to_string(),
            enabled: env.enabled,
            pinned_version: env.pinned_version,
            upgrade: None,
        };
        with_facts(&mut entry, facts);
        entries.push(entry);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests;
