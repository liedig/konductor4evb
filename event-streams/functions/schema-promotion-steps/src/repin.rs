//! The re-pin coordination steps of a promotion: `widen` records the
//! broadened pattern on the `REPIN` item and returns the affected
//! subscription entries; `narrow` commits the final pin after the drain on
//! its own generation only. Shared plumbing lives here; the steps are
//! `widen.rs` and `narrow.rs`.

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;

use konductor_common::dynamodb::s;
use konductor_common::promotion::{promotion_pk, RepinState, REPIN_SK};
use konductor_common::promotion_store::repin_from_item;
use konductor_common::streams::StreamStatus;
use konductor_common::streams_db::query_streams_by_schema;

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
        entries.push(RepinEntry {
            producer_domain: stream.producer_domain.clone(),
            consumer_domain: stream.consumer_domain.clone(),
            source: stream.source.clone(),
            detail_type: stream.detail_type.clone(),
            schema_name: stream.schema_name.clone(),
            rule_name: env.rule_name.clone(),
            environment: environment.to_string(),
            enabled: env.enabled,
            pinned_version: env.pinned_version,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests;
