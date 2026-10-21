//! `narrow`: commits the final pin after the drain, but only on its own
//! generation — a newer widen supersedes it and takes over. The conditional
//! update also requires the drain deadline to have elapsed.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use konductor_common::dynamodb::{n, s};
use konductor_common::promotion::{promotion_pk, RepinState, REPIN_SK};

use super::{
    active_metadata_revision, emission_enabled, enumerate, is_conditional_failure, pattern,
    read_repin_item, rfc3339, RepinEntry, StepFacts,
};
use crate::store::{promotions_table, Clients};
use konductor_common::system_events::UpgradeTrigger;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NarrowInput {
    pub promotion_id: String,
    pub domain: String,
    pub schema_name: String,
    pub environment: String,
    pub to: i64,
    /// The generation this promotion's widen committed; narrowing any other
    /// generation would clobber a newer re-pin.
    pub generation: i64,
    /// Promotion trigger for system-event snapshots. Absent in legacy
    /// in-flight executions, which emit nothing.
    #[serde(default)]
    pub trigger: Option<UpgradeTrigger>,
    /// The version this operation was asked to land; `to` may be lower when
    /// the replay stopped early, which the terminal snapshot records as a
    /// partial outcome.
    #[serde(default)]
    pub requested_version: Option<i64>,
    /// The metadata revision widen observed; from/requested selectors pin
    /// to it so both phases reference one generation of contract metadata.
    #[serde(default)]
    pub start_metadata_revision: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NarrowOutput {
    pub narrowed: bool,
    pub generation: i64,
    pub pattern: Vec<i64>,
    pub entries: Vec<RepinEntry>,
}

pub async fn narrow(
    clients: &Clients,
    input: &serde_json::Value,
) -> Result<serde_json::Value, lambda_runtime::Error> {
    let input: NarrowInput =
        serde_json::from_value(input.clone()).context("narrow input is malformed")?;
    let output = run_narrow(clients, &input).await?;
    Ok(serde_json::to_value(output)?)
}

async fn run_narrow(clients: &Clients, input: &NarrowInput) -> Result<NarrowOutput> {
    let fallback = vec![input.to, input.to + 1];
    let Some(state) = read_repin_item(
        clients,
        &input.domain,
        &input.schema_name,
        &input.environment,
    )
    .await?
    else {
        return Ok(NarrowOutput {
            narrowed: false,
            generation: 0,
            pattern: fallback,
            entries: vec![],
        });
    };
    if state.generation != input.generation
        || state.widened_by.as_deref() != Some(input.promotion_id.as_str())
    {
        return Ok(superseded(state));
    }
    let expected_repin_at = state.drain_until;
    let now = rfc3339(&Utc::now());
    let result = clients
        .ddb
        .update_item()
        .table_name(promotions_table()?)
        .key(
            "pk",
            s(&promotion_pk(
                &input.domain,
                &input.schema_name,
                &input.environment,
            )),
        )
        .key("sk", s(REPIN_SK))
        .update_expression(
            "SET floorVersion = :to, targetVersion = :to, updatedAt = :now \
             REMOVE drainUntil, pinnedBefore",
        )
        .condition_expression("generation = :gen AND widenedBy = :pid AND drainUntil <= :now")
        .expression_attribute_values(":to", n(input.to))
        .expression_attribute_values(":gen", n(input.generation))
        .expression_attribute_values(":pid", s(&input.promotion_id))
        .expression_attribute_values(":now", s(&now))
        .send()
        .await;
    match result {
        Ok(_) => {}
        Err(e) => {
            let err = anyhow::Error::new(e.into_service_error());
            if is_conditional_failure(&err) {
                let fresh = read_repin_item(
                    clients,
                    &input.domain,
                    &input.schema_name,
                    &input.environment,
                )
                .await?
                .context("REPIN item vanished mid-narrow")?;
                if fresh.generation != input.generation
                    || fresh.widened_by.as_deref() != Some(input.promotion_id.as_str())
                {
                    return Ok(superseded(fresh));
                }
                anyhow::bail!("narrow condition failed on its own generation (drain not elapsed?)");
            }
            return Err(err).context("narrow update failed");
        }
    }
    let facts = narrow_facts(clients, input, expected_repin_at).await?;
    let entries = enumerate(
        clients,
        &input.domain,
        &input.schema_name,
        &input.environment,
        facts.as_ref(),
    )
    .await?;
    Ok(NarrowOutput {
        narrowed: true,
        generation: input.generation,
        pattern: fallback,
        entries,
    })
}

/// Terminal-phase facts: present only when the workflow passed its trigger
/// and the widen-phase metadata revision, and emission is enabled. The
/// terminal revision is read fresh — metadata may have activated mid-drain.
async fn narrow_facts(
    clients: &Clients,
    input: &NarrowInput,
    expected_repin_at: Option<DateTime<Utc>>,
) -> Result<Option<StepFacts>> {
    Ok(
        match (
            input.trigger,
            input.requested_version,
            input.start_metadata_revision,
            emission_enabled(),
        ) {
            (Some(trigger), Some(requested), Some(start_revision), true) => Some(StepFacts {
                operation_id: input.promotion_id.clone(),
                trigger,
                generation: input.generation as u64,
                requested_version: requested,
                start_metadata_revision: start_revision,
                terminal_metadata_revision: Some(
                    active_metadata_revision(clients, &input.environment).await?,
                ),
                expected_repin_at,
            }),
            _ => None,
        },
    )
}

fn superseded(state: RepinState) -> NarrowOutput {
    NarrowOutput {
        narrowed: false,
        generation: state.generation,
        pattern: pattern(state.floor_version, state.target_version),
        entries: vec![],
    }
}
