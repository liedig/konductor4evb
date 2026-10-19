//! The version-content store writes: every accepted registry write lands in
//! `konductor-schema-versions`, because the registry normalizes whitespace
//! and serves only the latest version — this record is the only replayable
//! history a promotion can read.

use chrono::Utc;

use konductor_common::dynamodb::{n, s};
use konductor_common::error::ControlPlaneError;
use konductor_common::promotion::{
    canonical_content_eq, canonical_content_hash, promotion_pk, version_sk, WRITTEN_BY_SCHEMA_ADMIN,
};

use crate::handler::{describe_latest, Clients, SchemaTarget};

/// Every accepted registry write lands in the version-content store: the
/// registry normalizes whitespace and serves only the latest version, so
/// this record is the only replayable history. A write failure is surfaced —
/// the retry is safe because identical content hits the registry's no-change
/// conflict, whose handler completes the record instead of minting a version.
pub(crate) async fn record_version_content(
    clients: &Clients,
    target: &SchemaTarget,
    version: i64,
    content: &str,
) -> Result<(), ControlPlaneError> {
    let table = std::env::var("SCHEMA_VERSIONS_TABLE")
        .map_err(|_| ControlPlaneError::Schemas("SCHEMA_VERSIONS_TABLE not set".to_string()))?;
    let content_hash = canonical_content_hash(content).ok_or_else(|| {
        ControlPlaneError::Schemas("accepted content failed canonical hashing".to_string())
    })?;
    clients
        .ddb
        .put_item()
        .table_name(&table)
        .item(
            "pk",
            s(&promotion_pk(
                &target.producer_domain,
                &target.event_name,
                &target.environment,
            )),
        )
        .item("sk", s(&version_sk(version)))
        .item("domain", s(&target.producer_domain))
        .item("schemaName", s(&target.event_name))
        .item("environment", s(&target.environment))
        .item("version", n(version))
        .item("content", s(content))
        .item("contentHash", s(&content_hash))
        .item("writtenBy", s(WRITTEN_BY_SCHEMA_ADMIN))
        .item("writtenAt", s(&Utc::now().to_rfc3339()))
        .send()
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                schema = %target.schema_name,
                version,
                "version content record write failed"
            );
            ControlPlaneError::Schemas(format!("version content record write failed: {e}"))
        })?;
    Ok(())
}

/// Shared ambiguous-retry repair. When the registry reports that the
/// requested content already exists (duplicate create, no-change append),
/// the latest version is described and canonically compared with the
/// request: a match completes the missing content record for that version
/// and returns it; a mismatch returns `None` and changes nothing.
pub(crate) async fn complete_record_if_canonical_equal(
    clients: &Clients,
    target: &SchemaTarget,
    content: &str,
) -> Result<Option<i64>, ControlPlaneError> {
    let (version, latest_content) = describe_latest(clients, target).await?;
    if canonical_content_eq(content, &latest_content) != Some(true) {
        return Ok(None);
    }
    let Some(version) = version else {
        return Err(ControlPlaneError::Schemas(
            "describe schema carried no usable schemaVersion".to_string(),
        ));
    };
    record_version_content(clients, target, version, content).await?;
    Ok(Some(version))
}
