//! In-region load and latency harness for platform metadata composition
//! (change `add-platform-metadata-composition`, task 7.4). Deploy, invoke,
//! tear down — the stack is throwaway, exactly like the grant benchmark.
//!
//! Probes mirror the production hot paths:
//! - `pointer-read`: the strongly consistent pointer read admission performs
//!   on every publish, through the exact `MetadataStore` production call.
//! - `pointer-hot-key`: sustained sequential reads of one environment
//!   pointer item at a target rate — the environment hot-key pressure shape.
//! - `registry-describe-burst`: the post-activation registry refresh burst
//!   (one version-less `DescribeSchema` per schema, as the cache refresh
//!   performs) over the schema list supplied in the invoke payload.
//!
//! Seeding: the pointer probes put and later delete their own fixture
//! pointer item (`benchmark-env`); they never touch real environments. The
//! registry burst reads only; it fabricates no registry state — supply the
//! target registry's real schema list from the catalog.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use lambda_runtime::{Error, LambdaEvent};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::model;
use konductor_common::dynamodb::s;
use konductor_common::metadata::revisions::MetadataStore;
use konductor_common::metadata::store::ENVIRONMENT_PARTITION;

const BENCH_ENVIRONMENT: &str = "benchmark-env";
const DEFAULT_POINTER_SAMPLES: usize = 200;
const DEFAULT_HOT_KEY_SECONDS: u64 = 30;
const DEFAULT_HOT_KEY_TARGET_RPS: f64 = 50.0;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct BenchmarkRequest {
    pointer_samples: Option<usize>,
    hot_key_seconds: Option<u64>,
    hot_key_target_rps: Option<f64>,
    registry: Option<String>,
    schema_names: Vec<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProbeReport {
    probe: &'static str,
    samples: usize,
    errors: usize,
    achieved_rps: Option<f64>,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    notes: &'static str,
}

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    schemas: &aws_sdk_schemas::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = std::env::var("METADATA_TABLE").context("METADATA_TABLE not set")?;
    let request: BenchmarkRequest = serde_json::from_value(event.payload.clone())
        .context("benchmark request must be a JSON object with camelCase fields")?;
    let store = MetadataStore::new(ddb.clone(), table.clone());

    seed_pointer(ddb, &table).await?;
    let pointer = probe_pointer_read(
        &store,
        request.pointer_samples.unwrap_or(DEFAULT_POINTER_SAMPLES),
    )
    .await?;
    let hot_key = probe_pointer_hot_key(
        &store,
        request.hot_key_seconds.unwrap_or(DEFAULT_HOT_KEY_SECONDS),
        request
            .hot_key_target_rps
            .unwrap_or(DEFAULT_HOT_KEY_TARGET_RPS),
    )
    .await?;
    cleanup_pointer(ddb, &table).await?;
    let burst = probe_registry_burst(schemas, &request.registry, &request.schema_names).await?;

    Ok(json!({
        "pointerRead": pointer,
        "pointerHotKey": hot_key,
        "registryDescribeBurst": burst,
        "model": model::model_summary(),
    }))
}

async fn seed_pointer(ddb: &aws_sdk_dynamodb::Client, table: &str) -> Result<()> {
    ddb.put_item()
        .table_name(table)
        .item("PK", s(ENVIRONMENT_PARTITION))
        .item("SK", s(BENCH_ENVIRONMENT))
        .item("activeRevision", AttributeValue::N("0".into()))
        .item("generation", AttributeValue::N("1".into()))
        .item("activatedBy", s("metadata-benchmark"))
        .item("activatedAt", s("2026-09-22T00:00:00Z"))
        .send()
        .await
        .context("seed benchmark pointer fixture")?;
    Ok(())
}

async fn cleanup_pointer(ddb: &aws_sdk_dynamodb::Client, table: &str) -> Result<()> {
    ddb.delete_item()
        .table_name(table)
        .key("PK", s(ENVIRONMENT_PARTITION))
        .key("SK", s(BENCH_ENVIRONMENT))
        .send()
        .await
        .context("remove benchmark pointer fixture")?;
    Ok(())
}

async fn probe_pointer_read(store: &MetadataStore, samples: usize) -> Result<ProbeReport> {
    let mut latencies = Vec::with_capacity(samples);
    let mut errors = 0;
    for _ in 0..samples {
        match timed_resolve(store).await {
            Ok(ms) => latencies.push(ms),
            Err(_) => errors += 1,
        }
    }
    Ok(report(
        "pointer-read",
        latencies,
        errors,
        None,
        "strongly consistent pointer read per admission, via MetadataStore::get_environment_pointer",
    ))
}

async fn probe_pointer_hot_key(
    store: &MetadataStore,
    seconds: u64,
    target_rps: f64,
) -> Result<ProbeReport> {
    let mut latencies = Vec::new();
    let mut errors = 0;
    let interval = Duration::from_secs_f64(1.0 / target_rps);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let started = Instant::now();
    while Instant::now() < deadline {
        let next = Instant::now() + interval;
        match timed_resolve(store).await {
            Ok(ms) => latencies.push(ms),
            Err(_) => errors += 1,
        }
        let now = Instant::now();
        if now < next {
            tokio::time::sleep(next - now).await;
        }
    }
    let achieved = latencies.len() as f64 / started.elapsed().as_secs_f64();
    Ok(report(
        "pointer-hot-key",
        latencies,
        errors,
        Some(achieved),
        "sustained sequential reads of one pointer item at the target rate; errors surface throttling with retries disabled",
    ))
}

async fn timed_resolve(store: &MetadataStore) -> Result<f64> {
    let started = Instant::now();
    let pointer = store
        .get_environment_pointer(BENCH_ENVIRONMENT)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .context("benchmark pointer fixture missing")?;
    let _ = pointer;
    Ok(started.elapsed().as_secs_f64() * 1000.0)
}

async fn probe_registry_burst(
    schemas: &aws_sdk_schemas::Client,
    registry: &Option<String>,
    schema_names: &[String],
) -> Result<ProbeReport> {
    let Some(registry) = registry else {
        return Ok(report(
            "registry-describe-burst",
            Vec::new(),
            0,
            None,
            "skipped: supply registry and the environment's real schema list from the catalog",
        ));
    };
    let mut latencies = Vec::with_capacity(schema_names.len());
    let mut errors = 0;
    let started = Instant::now();
    for name in schema_names {
        let call = Instant::now();
        match schemas
            .describe_schema()
            .registry_name(registry)
            .schema_name(name)
            .send()
            .await
        {
            Ok(_) => latencies.push(call.elapsed().as_secs_f64() * 1000.0),
            Err(_) => errors += 1,
        }
    }
    let total = started.elapsed();
    let achieved = if latencies.is_empty() {
        None
    } else {
        Some(schema_names.len() as f64 / total.as_secs_f64())
    };
    Ok(report(
        "registry-describe-burst",
        latencies,
        errors,
        achieved,
        "version-less DescribeSchema per schema — the post-activation cold-start refresh shape",
    ))
}

fn report(
    probe: &'static str,
    latencies: Vec<f64>,
    errors: usize,
    achieved_rps: Option<f64>,
    notes: &'static str,
) -> ProbeReport {
    let mut sorted = latencies;
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ProbeReport {
        probe,
        samples: sorted.len(),
        errors,
        achieved_rps,
        p50_ms: percentile(&sorted, 0.50),
        p95_ms: percentile(&sorted, 0.95),
        p99_ms: percentile(&sorted, 0.99),
        max_ms: sorted.last().copied().unwrap_or(0.0),
        notes,
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
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
    }

    #[test]
    fn percentile_handles_empty_and_small() {
        assert_eq!(percentile(&[], 0.99), 0.0);
        assert_eq!(percentile(&[3.0], 0.50), 3.0);
        assert_eq!(percentile(&[2.0, 8.0], 0.50), 2.0);
    }

    #[test]
    fn report_counts_errors_separately_from_samples() {
        let report = report("test", vec![10.0, 20.0], 3, Some(4.0), "note");
        assert_eq!(report.samples, 2);
        assert_eq!(report.errors, 3);
        assert_eq!(report.p50_ms, 10.0);
        assert_eq!(report.max_ms, 20.0);
        assert_eq!(report.achieved_rps, Some(4.0));
    }

    #[tokio::test]
    async fn burst_probe_skips_without_a_registry() {
        let config = aws_sdk_schemas::Config::builder()
            .behavior_version(aws_sdk_schemas::config::BehaviorVersion::latest())
            .build();
        let client = aws_sdk_schemas::Client::from_conf(config);
        let report = probe_registry_burst(&client, &None, &["orders".to_string()])
            .await
            .expect("skip path");
        assert_eq!(report.samples, 0);
        assert_eq!(report.errors, 0);
    }
}
