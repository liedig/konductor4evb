//! Candidate discovery and rule disablement for one revoked entitlement.
//!
//! The revoked scope pattern narrows the candidate query on the
//! event-streams table's producer partition: an exact name is one schema
//! prefix, a trailing-`*` pattern is a schema-name prefix, and `*` alone is
//! the whole partition. Every candidate is then re-evaluated against the
//! consumer domain's *current* entitlements, so an alternative grant added
//! after the revocation legitimately cancels the disable on redrive.

use anyhow::{Context, Result};
use serde::Deserialize;

use nexus_common::entitlement_store::EntitlementStore;
use nexus_common::error::ControlPlaneError;
use nexus_common::grant_engine::covers;
use nexus_common::streams::{EventStream, StreamStatus};
use nexus_common::streams_db;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevocationMessage {
    pub revocation_id: String,
    // Carried for audit correlation in logs, not read by the worker logic.
    #[allow(dead_code)]
    pub entitlement_id: String,
    pub consumer_domain: String,
    pub producer_domain: String,
    pub scope_pattern: String,
    #[allow(dead_code)]
    pub revoked_at: String,
}

/// The shared context every candidate query and disable runs against.
pub struct RevocationContext<'a> {
    pub ddb: &'a aws_sdk_dynamodb::Client,
    pub eb: &'a aws_sdk_eventbridge::Client,
    pub streams_table: &'a str,
    pub entitlements_table: &'a str,
    pub organization: &'a str,
}

/// Streams on the revoked producer whose schema falls under the revoked
/// pattern. Narrowed by key where the pattern allows; coverage is
/// re-evaluated per candidate regardless, so over-matching is harmless.
pub async fn candidates(
    ctx: &RevocationContext<'_>,
    message: &RevocationMessage,
) -> Result<Vec<EventStream>> {
    let schema_prefix = format!("{}-{}@", ctx.organization, message.producer_domain);
    match message.scope_pattern.strip_suffix('*') {
        // Exact name: one schema's subscriptions.
        None => {
            let schema_name = format!("{schema_prefix}{}", message.scope_pattern);
            streams_db::query_streams_by_schema(
                ctx.ddb,
                ctx.streams_table,
                &message.producer_domain,
                &schema_name,
            )
            .await
        }
        // Domain-wide: the whole producer partition.
        Some("") => {
            streams_db::query_streams_by_producer(
                ctx.ddb,
                ctx.streams_table,
                &message.producer_domain,
            )
            .await
        }
        // Prefix family: one schema-name prefix.
        Some(prefix) => {
            streams_db::query_streams_by_schema_prefix(
                ctx.ddb,
                ctx.streams_table,
                &message.producer_domain,
                &format!("{schema_prefix}{prefix}"),
            )
            .await
        }
    }
}

/// Whether `stream` is a revocation target for `message`: owned by the
/// revoked consumer, an active cross-domain subscription. Self-subscriptions
/// are implicitly covered and pending streams are guarded by the activation
/// path's final check, so both are skipped.
pub fn is_revocation_target(stream: &EventStream, message: &RevocationMessage) -> bool {
    stream.consumer_domain == message.consumer_domain
        && stream.consumer_domain != stream.producer_domain
        && stream.status == StreamStatus::Active
}

/// Current coverage for the revoked consumer over the stream's detail type,
/// re-read on every invocation so redrives see grants added since. Fails
/// closed: an unreadable store reports uncovered.
pub async fn is_currently_covered(ctx: &RevocationContext<'_>, stream: &EventStream) -> bool {
    let store = EntitlementStore::new(ctx.ddb.clone(), ctx.entitlements_table);
    match store
        .entitlements_for_consumer(&stream.consumer_domain)
        .await
    {
        Ok(current) => covers(&current, &stream.producer_domain, &stream.detail_type),
        Err(e) => {
            tracing::warn!(
                error = %e,
                consumer = stream.consumer_domain,
                "coverage re-evaluation failed closed"
            );
            false
        }
    }
}

/// Disables every enabled environment's Nexus-managed rule and flips its
/// persisted desired state. Idempotent: already-disabled environments are
/// skipped, and rule identities, pinned versions, and the subscription
/// record are retained. Returns the number of environments disabled.
pub async fn disable_stream_environments(
    ctx: &RevocationContext<'_>,
    stream: &EventStream,
) -> Result<usize> {
    let mut disabled = 0;
    for (env_name, env_stream) in &stream.environments {
        if !env_stream.enabled {
            continue;
        }
        let bus_name = format!(
            "{}-{env_name}-{}-event-bus",
            ctx.organization, stream.producer_domain
        );
        ctx.eb
            .disable_rule()
            .name(&env_stream.rule_name)
            .event_bus_name(&bus_name)
            .send()
            .await
            .map_err(|e| ControlPlaneError::EventBridge(e.into_service_error().to_string()))?;
        streams_db::set_stream_env_enabled(ctx.ddb, ctx.streams_table, stream, env_name, false)
            .await?;
        disabled += 1;
    }
    Ok(disabled)
}

/// Full per-message flow shared by the worker handler tests.
pub async fn process_revocation(
    ctx: &RevocationContext<'_>,
    message: &RevocationMessage,
) -> Result<()> {
    let candidates = candidates(ctx, message).await.context("candidate query")?;
    for stream in &candidates {
        if !is_revocation_target(stream, message) {
            continue;
        }
        if is_currently_covered(ctx, stream).await {
            tracing::info!(
                consumer = stream.consumer_domain,
                schema = stream.schema_name,
                "candidate still covered by an overlapping grant; left enabled"
            );
            continue;
        }
        let disabled = disable_stream_environments(ctx, stream).await?;
        tracing::info!(
            consumer = stream.consumer_domain,
            schema = stream.schema_name,
            disabled,
            revocation_id = message.revocation_id,
            "disabled uncovered subscription environments"
        );
    }
    Ok(())
}
