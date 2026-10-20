//! `replay`: writes one source version to the target registry, idempotently.
//!
//! The guard is read-before-write under the single-writer lease: write only
//! when the target's latest is N−1, accepting the write only when the
//! registry mints N; a no-change conflict is accepted only after verifying
//! the version landed with equal content. A latest of N with equal content
//! means an earlier attempt landed the write. Source content comes from the
//! version record, falling back to the immutable source-registry version for
//! schemas that predate content recording. Every other state fails the step;
//! the workflow's Catch turns it into `REPLAY_FAILED`.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use lambda_runtime::Error;
use serde::Serialize;
use serde_json::Value;

use aws_sdk_schemas::error::ProvideErrorMetadata;
use konductor_common::promotion::{canonical_content_eq, SchemaVersionContent};

use crate::store::{
    commit_progress, describe_latest, full_schema_name, registry_name, resolve_source_content,
    write_record, Clients, StepInput,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayOutput {
    pub version: i64,
    pub wrote: bool,
}

pub async fn run(clients: &Clients, event: &Value) -> Result<Value, Error> {
    let input: StepInput =
        serde_json::from_value(event.clone()).context("replay input is malformed")?;
    let version = event
        .get("version")
        .and_then(Value::as_i64)
        .context("replay input is missing version")?;
    let output = replay(clients, &input, version).await?;
    Ok(serde_json::to_value(output).context("replay output failed to serialize")?)
}

async fn replay(clients: &Clients, input: &StepInput, version: i64) -> Result<ReplayOutput> {
    let Some((source_content, source_hash)) =
        resolve_source_content(clients, input, version).await?
    else {
        return Err(anyhow!(
            "source v{version} exists in neither the content store nor the {} registry ({}/{})",
            input.source_environment,
            input.domain,
            input.schema_name
        ));
    };
    let target = describe_latest(
        clients,
        &input.target_environment,
        &input.domain,
        &input.schema_name,
    )
    .await?;

    let wrote = match target {
        None if version == 1 => {
            match create_first_version(clients, input, &source_content).await? {
                WriteOutcome::Written => true,
                WriteOutcome::RefusedAsDuplicate => {
                    // An ambiguous earlier attempt created the schema without
                    // this step learning about it.
                    verify_landed(clients, input, version, &source_content).await?;
                    false
                }
            }
        }
        None => {
            return Err(anyhow!(
                "target holds no schema; cannot start at v{version}"
            ))
        }
        Some((latest, content)) if latest == version => {
            if canonical_content_eq(&content, &source_content) != Some(true) {
                return Err(anyhow!(
                    "target v{version} content differs from the source record"
                ));
            }
            tracing::info!(
                version,
                "replay write already landed; completing the record"
            );
            false
        }
        Some((latest, _)) if latest == version - 1 => {
            match write_version(clients, input, version, &source_content).await? {
                WriteOutcome::Written => true,
                WriteOutcome::RefusedAsDuplicate => {
                    // Identical content mints nothing (spike 1.1). Accept only
                    // if the version landed anyway (an ambiguous first attempt);
                    // otherwise the registry is refusing the write.
                    verify_landed(clients, input, version, &source_content).await?;
                    false
                }
            }
        }
        Some((latest, _)) => {
            return Err(anyhow!(
                "target latest is v{latest}, expected v{} or v{version}",
                version - 1
            ))
        }
    };

    let record = SchemaVersionContent {
        domain: input.domain.clone(),
        schema_name: input.schema_name.clone(),
        environment: input.target_environment.clone(),
        version,
        content: source_content,
        content_hash: source_hash,
        written_by: input.promotion_id.clone(),
        written_at: Utc::now(),
    };
    write_record(clients, &record).await?;
    commit_progress(clients, input, version).await?;
    Ok(ReplayOutput { version, wrote })
}

enum WriteOutcome {
    Written,
    /// The registry refused a no-op: identical content mints nothing on
    /// update (ConflictException), and create reports the schema as already
    /// present (BadRequest "already exists"). The caller verifies what
    /// actually landed before accepting either.
    RefusedAsDuplicate,
}

/// First arrival in an empty target registry: v1 must be created, since
/// `UpdateSchema` on a missing schema answers NotFound. CreateSchema has no
/// client token in the service model; the duplicate refusal plus
/// `verify_landed` carries the idempotency instead.
async fn create_first_version(
    clients: &Clients,
    input: &StepInput,
    content: &str,
) -> Result<WriteOutcome> {
    use aws_sdk_schemas::operation::create_schema::CreateSchemaError;
    let result = clients
        .schemas
        .create_schema()
        .registry_name(registry_name(&input.target_environment, &input.domain)?)
        .schema_name(full_schema_name(&input.domain, &input.schema_name)?)
        .r#type(aws_sdk_schemas::types::Type::JsonSchemaDraft4)
        .description("Promoted by Konductor")
        .content(content)
        .send()
        .await;
    match result {
        Ok(_) => Ok(WriteOutcome::Written),
        Err(e) => {
            let err = e.into_service_error();
            let duplicate = matches!(&err,
                CreateSchemaError::BadRequestException(bad_request)
                    if bad_request.message().unwrap_or_default().contains("already exists"))
                || err.code() == Some("ConflictException");
            if duplicate {
                Ok(WriteOutcome::RefusedAsDuplicate)
            } else {
                Err(anyhow::Error::new(err).context("replay create failed"))
            }
        }
    }
}

/// The replay write: `UpdateSchema` mints the next version, and the returned
/// version must be the one this step owns.
async fn write_version(
    clients: &Clients,
    input: &StepInput,
    version: i64,
    content: &str,
) -> Result<WriteOutcome> {
    let result = clients
        .schemas
        .update_schema()
        .registry_name(registry_name(&input.target_environment, &input.domain)?)
        .schema_name(full_schema_name(&input.domain, &input.schema_name)?)
        .r#type(aws_sdk_schemas::types::Type::JsonSchemaDraft4)
        .description("Promoted by Konductor")
        .content(content)
        .send()
        .await;
    let output = match result {
        Ok(output) => output,
        Err(e) => {
            let err = e.into_service_error();
            // The service model has no typed ConflictException for
            // UpdateSchema; match the code structurally (same as
            // schema-admin).
            if err.code() == Some("ConflictException") {
                return Ok(WriteOutcome::RefusedAsDuplicate);
            }
            return Err(anyhow::Error::new(err).context("replay write failed"));
        }
    };
    let minted = output
        .schema_version()
        .and_then(|v| v.parse::<i64>().ok())
        .context("registry returned no version for the replay write")?;
    if minted != version {
        return Err(anyhow!("registry minted v{minted}, expected v{version}"));
    }
    Ok(WriteOutcome::Written)
}

/// After a no-change conflict: accept when the target's latest is exactly
/// this version with canonically equal content.
async fn verify_landed(
    clients: &Clients,
    input: &StepInput,
    version: i64,
    content: &str,
) -> Result<()> {
    let current = describe_latest(
        clients,
        &input.target_environment,
        &input.domain,
        &input.schema_name,
    )
    .await?;
    match current {
        Some((latest, actual))
            if latest == version && canonical_content_eq(&actual, content) == Some(true) =>
        {
            Ok(())
        }
        _ => Err(anyhow!(
            "registry reported no change but does not hold v{version} with the recorded content"
        )),
    }
}

#[cfg(test)]
mod tests;
