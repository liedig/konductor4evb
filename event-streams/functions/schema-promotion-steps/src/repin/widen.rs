//! `widen`: records the broadened pattern (`floor ..= target`, plus the
//! lookahead slot) on the `REPIN` item — one conditional put that takes the
//! minimum floor, the maximum target, bumps the generation, and stamps the
//! drain deadline — and returns the affected subscription entries for the
//! workflow's apply map.

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use konductor_common::dynamodb::{n, s};
use konductor_common::promotion::{promotion_pk, RepinState, REPIN_SK};
use konductor_common::promotion_store::repin_to_item;

use super::{enumerate, is_conditional_failure, pattern, read_repin_item, rfc3339, RepinEntry};
use crate::store::{promotions_table, Clients};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidenInput {
    pub promotion_id: String,
    pub domain: String,
    pub schema_name: String,
    pub environment: String,
    pub through: i64,
    pub drain_seconds: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidenOutput {
    pub generation: i64,
    /// Pin subscriptions held before this widening; the workflow's narrow
    /// step falls back to it when the replay wrote nothing.
    pub old_pin: i64,
    pub pattern: Vec<i64>,
    pub drain_seconds: i64,
    pub drain_until: String,
    pub entries: Vec<RepinEntry>,
}

pub async fn widen(
    clients: &Clients,
    input: &serde_json::Value,
) -> Result<serde_json::Value, lambda_runtime::Error> {
    let input: WidenInput =
        serde_json::from_value(input.clone()).context("widen input is malformed")?;
    let output = run_widen(clients, &input).await?;
    Ok(serde_json::to_value(output)?)
}

async fn run_widen(clients: &Clients, input: &WidenInput) -> Result<WidenOutput> {
    let entries = enumerate(
        clients,
        &input.domain,
        &input.schema_name,
        &input.environment,
    )
    .await?;
    // Bounded re-read loop: a concurrent widen (an authoring-environment
    // re-pin can overlap a promotion) wins the conditional put, and the next
    // iteration folds its floor/target into ours.
    for _ in 0..3 {
        let existing = read_repin_item(
            clients,
            &input.domain,
            &input.schema_name,
            &input.environment,
        )
        .await?;
        if let Some(state) = &existing {
            if state.widened_by.as_deref() == Some(input.promotion_id.as_str()) {
                return Ok(widen_output(input, state, entries));
            }
        }
        let current_pin = entries.iter().filter_map(|e| e.pinned_version).min();
        let floor = match (&existing, current_pin) {
            (Some(state), Some(pin)) => state.floor_version.min(pin),
            (Some(state), None) => state.floor_version,
            (None, Some(pin)) => pin,
            (None, None) => input.through,
        };
        let target = existing
            .as_ref()
            .map_or(input.through, |s| s.target_version)
            .max(input.through);
        let old_generation = existing.as_ref().map_or(0, |s| s.generation);
        let pinned_before = existing
            .as_ref()
            .and_then(|s| s.pinned_before)
            .or(current_pin)
            .unwrap_or(input.through);
        let state = RepinState {
            floor_version: floor,
            target_version: target,
            generation: old_generation + 1,
            widened_by: Some(input.promotion_id.clone()),
            pinned_before: Some(pinned_before),
            drain_until: Some(Utc::now() + Duration::seconds(input.drain_seconds)),
            updated_at: Utc::now(),
        };
        match put_repin(clients, input, &state, old_generation).await {
            Ok(()) => return Ok(widen_output(input, &state, entries)),
            Err(e) if is_conditional_failure(&e) => continue,
            Err(e) => return Err(e),
        }
    }
    anyhow::bail!("widen lost the generation race three times");
}

fn widen_output(input: &WidenInput, state: &RepinState, entries: Vec<RepinEntry>) -> WidenOutput {
    WidenOutput {
        generation: state.generation,
        old_pin: state.pinned_before.unwrap_or(state.floor_version),
        pattern: pattern(state.floor_version, state.target_version),
        drain_seconds: input.drain_seconds,
        drain_until: state
            .drain_until
            .map(|t| rfc3339(&t))
            .unwrap_or_else(|| rfc3339(&Utc::now())),
        entries,
    }
}

async fn put_repin(
    clients: &Clients,
    input: &WidenInput,
    state: &RepinState,
    old_generation: i64,
) -> Result<()> {
    let mut item = repin_to_item(state);
    item.insert(
        "pk".to_string(),
        s(&promotion_pk(
            &input.domain,
            &input.schema_name,
            &input.environment,
        )),
    );
    item.insert("sk".to_string(), s(REPIN_SK));
    clients
        .ddb
        .put_item()
        .table_name(promotions_table()?)
        .set_item(Some(item))
        .condition_expression("attribute_not_exists(pk) OR generation = :gen")
        .expression_attribute_values(":gen", n(old_generation))
        .send()
        .await
        .map_err(|e| anyhow::Error::new(e.into_service_error()))
        .context("write REPIN item failed")?;
    Ok(())
}
