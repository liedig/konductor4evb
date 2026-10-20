//! `plan`: verifies the target's prefix against the version-content store
//! and returns the versions to replay. Divergence fails closed: any hash
//! mismatch, any target version without a promotion write record (the latest
//! excepted — it is repairable through a canonical registry comparison), and
//! a target ahead of its source all yield `DIVERGED` with the first
//! differing version.

use anyhow::{Context, Result};
use chrono::Utc;
use lambda_runtime::Error;
use serde::Serialize;
use serde_json::Value;

use konductor_common::environments;
use konductor_common::promotion::{
    canonical_content_eq, SchemaVersionContent, WRITTEN_BY_SCHEMA_ADMIN,
};

use crate::store::{
    self, describe_latest, read_record, registry_latest, write_record, Clients, StepInput,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanOutput {
    pub ok: bool,
    /// True when the input's source is not the target's immediate
    /// predecessor in the configured pipeline: the promotion is refused
    /// before any registry read, regardless of target state.
    pub skipped: bool,
    pub from_version: i64,
    pub versions: Vec<i64>,
    /// The registry's canonical schema name, for the publish signal's detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_schema_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diverged_at: Option<i64>,
}

impl PlanOutput {
    fn ok(from_version: i64, requested: i64, full_schema_name: String) -> Self {
        Self {
            ok: true,
            skipped: false,
            from_version,
            versions: ((from_version + 1)..=requested).collect(),
            full_schema_name: Some(full_schema_name),
            diverged_at: None,
        }
    }

    fn diverged(at: i64) -> Self {
        Self {
            ok: false,
            skipped: false,
            from_version: 0,
            versions: vec![],
            full_schema_name: None,
            diverged_at: Some(at),
        }
    }

    fn skipped() -> Self {
        Self {
            ok: false,
            skipped: true,
            from_version: 0,
            versions: vec![],
            full_schema_name: None,
            diverged_at: None,
        }
    }
}

pub async fn run(clients: &Clients, event: &Value) -> Result<Value, Error> {
    let input: StepInput =
        serde_json::from_value(event.clone()).context("plan input is malformed")?;
    let requested = event
        .get("requestedVersion")
        .and_then(Value::as_i64)
        .context("plan input is missing requestedVersion")?;
    let output = plan(clients, &input, requested).await?;
    Ok(serde_json::to_value(output).context("plan output failed to serialize")?)
}

async fn plan(clients: &Clients, input: &StepInput, requested: i64) -> Result<PlanOutput> {
    if !is_adjacent(input) {
        return Ok(PlanOutput::skipped());
    }
    let latest = registry_latest(
        clients,
        &input.target_environment,
        &input.domain,
        &input.schema_name,
    )
    .await?;
    let k = latest.unwrap_or(0);
    if k > requested {
        return Ok(PlanOutput::diverged(k));
    }
    for version in 1..=k {
        if !verify_version(clients, input, version, version == k).await? {
            return Ok(PlanOutput::diverged(version));
        }
    }
    let full_schema_name = store::full_schema_name(&input.domain, &input.schema_name)?;
    Ok(PlanOutput::ok(k, requested, full_schema_name))
}

/// The workflow-level adjacency gate: the API already derives the source as
/// the stage's immediate predecessor, so a mismatch here means the execution
/// was started out-of-band with a planted lease. Fail closed.
fn is_adjacent(input: &StepInput) -> bool {
    environments::promotion_source(&input.target_environment)
        .map(|source| source.as_deref() == Some(input.source_environment.as_str()))
        .unwrap_or(false)
}

/// One prefix position: the target's record must be a promotion write whose
/// hash matches the source's. A missing record for the latest version is
/// repairable because only the latest is registry-readable.
async fn verify_version(
    clients: &Clients,
    input: &StepInput,
    version: i64,
    is_latest: bool,
) -> Result<bool> {
    let Some(source) = read_record(
        clients,
        &input.domain,
        &input.schema_name,
        &input.source_environment,
        version,
    )
    .await?
    else {
        return Ok(false);
    };
    let target = read_record(
        clients,
        &input.domain,
        &input.schema_name,
        &input.target_environment,
        version,
    )
    .await?;
    match target {
        Some(record) => Ok(record.written_by != WRITTEN_BY_SCHEMA_ADMIN
            && record.content_hash == source.content_hash),
        None if is_latest => repair_latest(clients, input, &source, version).await,
        None => Ok(false),
    }
}

/// Completes the missing latest-version record when the registry content is
/// canonically equal to the source record's content.
async fn repair_latest(
    clients: &Clients,
    input: &StepInput,
    source: &SchemaVersionContent,
    version: i64,
) -> Result<bool> {
    let current = describe_latest(
        clients,
        &input.target_environment,
        &input.domain,
        &input.schema_name,
    )
    .await?;
    let Some((latest, content)) = current else {
        return Ok(false);
    };
    if latest != version || canonical_content_eq(&content, &source.content) != Some(true) {
        return Ok(false);
    }
    let repair = SchemaVersionContent {
        domain: input.domain.clone(),
        schema_name: input.schema_name.clone(),
        environment: input.target_environment.clone(),
        version,
        content: source.content.clone(),
        content_hash: source.content_hash.clone(),
        written_by: input.promotion_id.clone(),
        written_at: Utc::now(),
    };
    write_record(clients, &repair).await?;
    tracing::info!(
        domain = %input.domain,
        schema = %input.schema_name,
        version,
        "completed missing version record from registry content"
    );
    Ok(true)
}

#[cfg(test)]
mod tests;
