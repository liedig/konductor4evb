//! In-region latency harness for the grant engine (change `add-grant-engine`,
//! task 6.1). Reuses the spike's declared measurement procedure: sequential
//! warm samples per probe, SDK retries disabled, no decision cache, and the
//! real `DynamoGrantEngine` decision path against the deployed bindings table.
//!
//! The harness seeds its own fixture bindings (idempotent put) and removes
//! them after the run, so it needs no admin token and leaves no state behind.

use std::time::Instant;

use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde::Serialize;
use serde_json::{json, Value};

use konductor_common::entitlement_store::EntitlementStore;
use konductor_common::grant::{BindingPrincipal, ResourceRole, RoleBinding};
use konductor_common::grant_engine::{actions, DynamoGrantEngine};
use konductor_common::grant_store::{binding_to_item, BindingStore};
use konductor_common::principal::{AuthorizationDecision, ResourceAuthorizer, TrustedPrincipal};

const BENCH_DOMAIN: &str = "benchmark-domain";
const BENCH_ISSUER: &str = "https://benchmark.invalid";
const BENCH_GROUP: &str = "benchmark-publishers";
const BENCH_ACCOUNT: &str = "000000000001";
const UNBOUND_ACCOUNT: &str = "111111111111";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProbeReport {
    probe: &'static str,
    expected_decision: &'static str,
    samples: usize,
    /// Decisions that diverged from the expectation. On allow probes this
    /// also catches fail-closed store errors (they surface as Deny); on deny
    /// probes a store error is indistinguishable from a true denial by design.
    wrong_decisions: usize,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
}

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = std::env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    let samples = event
        .payload
        .get("samples")
        .and_then(Value::as_u64)
        .unwrap_or(200) as usize;
    let warmup = event
        .payload
        .get("warmup")
        .and_then(Value::as_u64)
        .unwrap_or(20) as usize;

    let entitlements_table =
        std::env::var("ENTITLEMENTS_TABLE").context("ENTITLEMENTS_TABLE not set")?;
    let engine = DynamoGrantEngine::new(
        BindingStore::new(ddb.clone(), table.clone()),
        EntitlementStore::new(ddb.clone(), entitlements_table),
    );

    let fixtures = fixture_bindings();
    seed_fixtures(ddb, &table, &fixtures).await?;

    let probes: Vec<(&'static str, TrustedPrincipal, AuthorizationDecision)> = vec![
        (
            "cognito-publish-allow",
            oauth(&[BENCH_GROUP]),
            AuthorizationDecision::Allow,
        ),
        (
            "cognito-publish-deny",
            oauth(&["benchmark-nobody"]),
            AuthorizationDecision::Deny,
        ),
        (
            "aws-publish-allow",
            TrustedPrincipal::iam(BENCH_ACCOUNT, None),
            AuthorizationDecision::Allow,
        ),
        (
            "aws-publish-deny",
            TrustedPrincipal::iam(UNBOUND_ACCOUNT, None),
            AuthorizationDecision::Deny,
        ),
    ];

    let mut reports = Vec::new();
    for (name, principal, expected) in &probes {
        reports.push(run_probe(&engine, name, principal, *expected, warmup, samples).await);
    }

    let cleanup = cleanup_fixtures(ddb, &table, &fixtures).await;

    let total_wrong: usize = reports.iter().map(|r| r.wrong_decisions).sum();
    Ok(json!({
        "table": table,
        "samplesPerProbe": samples,
        "warmupSamples": warmup,
        "sdkRetries": "disabled (max attempts 1)",
        "probes": reports,
        "totalWrongDecisions": total_wrong,
        "fixtureCleanup": match cleanup {
            Ok(()) => "removed",
            Err(ref e) => {
                tracing::warn!(error = %e, "fixture cleanup failed");
                "failed"
            }
        },
    }))
}

async fn run_probe(
    engine: &DynamoGrantEngine,
    name: &'static str,
    principal: &TrustedPrincipal,
    expected: AuthorizationDecision,
    warmup: usize,
    samples: usize,
) -> ProbeReport {
    for _ in 0..warmup {
        let _ = engine
            .authorize(principal, actions::SCHEMA_PUBLISH, BENCH_DOMAIN)
            .await;
    }

    let mut latencies = Vec::with_capacity(samples);
    let mut wrong_decisions = 0;
    for _ in 0..samples {
        let start = Instant::now();
        let decision = engine
            .authorize(principal, actions::SCHEMA_PUBLISH, BENCH_DOMAIN)
            .await;
        latencies.push(start.elapsed().as_secs_f64() * 1000.0);
        if decision != expected {
            wrong_decisions += 1;
        }
    }
    latencies.sort_by(f64::total_cmp);

    ProbeReport {
        probe: name,
        expected_decision: match expected {
            AuthorizationDecision::Allow => "allow",
            AuthorizationDecision::Deny => "deny",
        },
        samples,
        wrong_decisions,
        p50_ms: percentile(&latencies, 0.50),
        p95_ms: percentile(&latencies, 0.95),
        p99_ms: percentile(&latencies, 0.99),
        max_ms: latencies.last().copied().unwrap_or(0.0),
    }
}

/// Nearest-rank percentile over sorted samples.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.max(1).min(sorted.len()) - 1]
}

fn oauth(groups: &[&str]) -> TrustedPrincipal {
    TrustedPrincipal::oauth(
        "benchmark-subject",
        BENCH_ISSUER,
        "benchmark-client",
        vec![],
        groups.iter().map(|g| g.to_string()).collect(),
    )
}

fn fixture_bindings() -> Vec<RoleBinding> {
    let now = chrono::Utc::now();
    [
        (
            "bench-fixture-group",
            BindingPrincipal::cognito_group(BENCH_ISSUER, BENCH_GROUP),
        ),
        (
            "bench-fixture-account",
            BindingPrincipal::aws_account(BENCH_ACCOUNT),
        ),
    ]
    .into_iter()
    .map(|(id, principal)| {
        RoleBinding::new(
            id,
            principal.expect("fixture principal is valid"),
            ResourceRole::Publisher,
            BENCH_DOMAIN,
            now,
            "grant-benchmark",
        )
        .expect("fixture binding is valid")
    })
    .collect()
}

async fn seed_fixtures(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    fixtures: &[RoleBinding],
) -> Result<()> {
    for binding in fixtures {
        ddb.put_item()
            .table_name(table)
            .set_item(Some(binding_to_item(binding)))
            .send()
            .await
            .context("seed fixture binding")?;
    }
    Ok(())
}

async fn cleanup_fixtures(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    fixtures: &[RoleBinding],
) -> Result<()> {
    for binding in fixtures {
        ddb.delete_item()
            .table_name(table)
            .key("PK", konductor_common::dynamodb::s(&binding.pk()))
            .key("SK", konductor_common::dynamodb::s(&binding.sk()))
            .send()
            .await
            .context("remove fixture binding")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank() {
        let sorted: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        assert_eq!(percentile(&sorted, 0.50), 50.0);
        assert_eq!(percentile(&sorted, 0.95), 95.0);
        assert_eq!(percentile(&sorted, 0.99), 99.0);
        assert_eq!(percentile(&sorted, 0.999), 100.0);
    }

    #[test]
    fn percentile_handles_small_samples() {
        assert_eq!(percentile(&[3.0], 0.99), 3.0);
        assert_eq!(percentile(&[], 0.50), 0.0);
        assert_eq!(percentile(&[2.0, 8.0], 0.50), 2.0);
    }

    #[test]
    fn fixture_bindings_are_valid_and_scoped() {
        let fixtures = fixture_bindings();
        assert_eq!(fixtures.len(), 2);
        for f in &fixtures {
            assert_eq!(f.domain, BENCH_DOMAIN);
            assert_eq!(f.role, ResourceRole::Publisher);
            assert!(f.binding_id.starts_with("bench-fixture-"));
        }
    }
}
