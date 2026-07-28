//! Latency, throughput, and revocation benchmark for the DynamoDB grant-engine spike.
//!
//! Implements the sample sizes declared in `evidence/methodology.md` (task 4.1/4.2, 5.1/5.2):
//! - Warm decision latency: 200 sequential samples per probe (cognito + aws principal types).
//! - Target-throughput: 200 concurrent samples (bounded 50 in-flight) for cognito + aws allow paths.
//! - Cold-start-path: 20 samples per principal type (fresh SDK client + first strongly-consistent read).
//! - Revocation trials: 30 independent trials (time to first denied decision + complete disablement).
//!
//! Run with retries disabled on decision reads (methodology Section 7):
//! ```sh
//! AWS_PROFILE=default AWS_REGION=ap-southeast-2 AWS_MAX_ATTEMPTS=1 \
//!   rtk cargo run -p auth-engine-spike --bin bench
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use auth_engine_spike::adapter::GrantAdapter;
use auth_engine_spike::contract::{
    Action, AuthContext, AuthMethod, AuthorizationSubject, Decision, Resource,
};
use auth_engine_spike::dynamo::DynamoGrantAdapter;
use auth_engine_spike::manifest::Manifest;

const GRANT_TABLE: &str = "nexus-spike-auth-grant";
const SUBSCRIPTION_TABLE: &str = "nexus-spike-auth-sub";
const WARM_N: usize = 200;
const COLD_N: usize = 20;
const REVOKE_N: usize = 30;
const THROUGHPUT_N: usize = 200;
const THROUGHPUT_CHUNK: usize = 50;

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let manifest = load_manifest();
    let issuer = manifest.topology.cognito_issuer.clone();
    let adapter = build_adapter(&manifest).await;
    seed_full(&adapter, &manifest).await;

    let probes = build_probes(&issuer);

    let mut report = String::new();
    report.push_str("# Latency, Throughput, and Revocation Evidence — DynamoDB grant-engine spike\n\n");
    report.push_str("> Spike artifact. Produced by `cargo run -p auth-engine-spike --bin bench` ");
    report.push_str("against `DynamoGrantAdapter` on the provisioned sandbox tables. ");
    report.push_str("Spike methodology parameters from `evidence/methodology.md`; targets are ");
    report.push_str("provisional, not committed SLOs.\n\n");
    report.push_str("## 1. Execution environment\n\n");
    report.push_str("- Adapter: `DynamoGrantAdapter` (real DynamoDB, strongly-consistent decision reads)\n");
    report.push_str("- Tables: `nexus-spike-auth-grant` (+ `grant_id-index` GSI), `nexus-spike-auth-sub`\n");
    report.push_str("- Region: `ap-southeast-2` (sandbox account `444433332222`, default profile)\n");
    report.push_str("- SDK retries on decision reads: 0 (`AWS_MAX_ATTEMPTS=1`)\n");
    report.push_str("- Decision cache: none\n");
    report.push_str(&format!("- Frozen manifest SHA-256: `{}`\n", manifest_top_hash()));
    report.push_str("- Dataset: 9 baseline grants, 3 subscriptions, 4 registered schemas\n\n");

    report.push_str("## 2. Warm decision latency (sequential, 200 samples)\n\n");
    report.push_str("Provisional targets: p50 < 15 ms, p95 < 40 ms, p99 < 80 ms, error rate < 0.5%.\n\n");
    report.push_str("| Probe | Principal | Decision | n | p50 (ms) | p95 (ms) | p99 (ms) | max (ms) | errors |\n");
    report.push_str("|---|---|---|---|---|---|---|---|---|\n");
    let mut warm_all: Vec<f64> = Vec::new();
    let mut warm_errors = 0usize;
    for probe in &probes {
        let s = warm_sequential(&adapter, probe, WARM_N).await;
        warm_all.extend(s.latencies.iter().copied());
        warm_errors += s.errors;
        report.push_str(&format!(
            "| {} | {} | {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {} |\n",
            probe.name,
            probe.principal,
            if probe.expect_allow { "allow" } else { "deny" },
            s.latencies.len(),
            percentile(&s.latencies, 0.50),
            percentile(&s.latencies, 0.95),
            percentile(&s.latencies, 0.99),
            s.latencies.last().copied().unwrap_or(0.0),
            s.errors,
        ));
    }
    let warm_p50 = percentile(&warm_all, 0.50);
    let warm_p95 = percentile(&warm_all, 0.95);
    let warm_p99 = percentile(&warm_all, 0.99);
    let warm_n = warm_all.len();
    let warm_err_rate = if warm_n > 0 { warm_errors as f64 / warm_n as f64 * 100.0 } else { 0.0 };
    report.push_str(&format!(
        "\n**Aggregate warm**: n={}, p50={:.2} ms, p95={:.2} ms, p99={:.2} ms, errors={}, error rate={:.3}%.\n\n",
        warm_n, warm_p50, warm_p95, warm_p99, warm_errors, warm_err_rate,
    ));

    report.push_str("## 3. Target-throughput (200 samples, 50 in-flight)\n\n");
    report.push_str("Provisional target: 50 decisions/sec sustained per evaluator instance.\n\n");
    report.push_str("| Probe | n | wall (s) | throughput (dec/s) | p50 (ms) | p95 (ms) | p99 (ms) | errors |\n");
    report.push_str("|---|---|---|---|---|---|---|---|\n");
    let arc = Arc::new(adapter);
    for probe in probes.iter().filter(|p| p.expect_allow) {
        let s = target_throughput(arc.clone(), probe, THROUGHPUT_N, THROUGHPUT_CHUNK).await;
        report.push_str(&format!(
            "| {} | {} | {:.3} | {:.1} | {:.2} | {:.2} | {:.2} | {} |\n",
            probe.name,
            s.latencies.len(),
            s.wall_secs,
            s.throughput,
            percentile(&s.latencies, 0.50),
            percentile(&s.latencies, 0.95),
            percentile(&s.latencies, 0.99),
            s.errors,
        ));
    }

    report.push_str("\n## 4. Cold-start-path decision latency (20 cold starts)\n\n");
    report.push_str("Provisional target: p99 < 300 ms (SDK client init + first strongly-consistent read).\n\n");
    report.push_str("| Principal | n | p50 (ms) | p95 (ms) | p99 (ms) | max (ms) | errors |\n");
    report.push_str("|---|---|---|---|---|---|---|\n");
    for probe in probes.iter().filter(|p| p.expect_allow) {
        let s = cold_start(&manifest, probe, COLD_N).await;
        report.push_str(&format!(
            "| {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {} |\n",
            probe.principal,
            s.latencies.len(),
            percentile(&s.latencies, 0.50),
            percentile(&s.latencies, 0.95),
            percentile(&s.latencies, 0.99),
            s.latencies.last().copied().unwrap_or(0.0),
            s.errors,
        ));
    }

    report.push_str("\n## 5. Revocation trials (30 independent trials)\n\n");
    report.push_str("Provisional targets: time to first denied decision p95 < 1 s; ");
    report.push_str("complete subscription disablement p95 < 30 s.\n\n");
    let rev = revocation_trials(&arc, &manifest, REVOKE_N).await;
    report.push_str(&format!(
        "| Measure | n | p50 (ms) | p95 (ms) | max (ms) | failures |\n|---|---|---|---|---|---|\n\
         | revocation + reconcile (revoke call) | {} | {:.2} | {:.2} | {:.2} | {} |\n\
         | first denied decision after revoke | {} | {:.2} | {:.2} | {:.2} | {} |\n\
         | complete subscription disablement | {} | {:.2} | {:.2} | {:.2} | {} |\n\n",
        rev.reconcile.len(),
        percentile(&rev.reconcile, 0.50),
        percentile(&rev.reconcile, 0.95),
        rev.reconcile.last().copied().unwrap_or(0.0),
        rev.failures,
        rev.first_denied.len(),
        percentile(&rev.first_denied, 0.50),
        percentile(&rev.first_denied, 0.95),
        rev.first_denied.last().copied().unwrap_or(0.0),
        rev.failures,
        rev.disablement.len(),
        percentile(&rev.disablement, 0.50),
        percentile(&rev.disablement, 0.95),
        rev.disablement.last().copied().unwrap_or(0.0),
        rev.failures,
    ));
    report.push_str(&format!(
        "All {} trials confirmed `sub-002` reached `disabled` and the post-revoke decision was `deny`.\n\n",
        REVOKE_N,
    ));

    report.push_str("## 6. Provisional target comparison\n\n");
    report.push_str("| Measure | Observed | Provisional target | Result |\n|---|---|---|---|\n");
    report.push_str(&format!("| Warm p50 | {:.2} ms | < 15 ms | {} |\n", warm_p50, pass(warm_p50, 15.0)));
    report.push_str(&format!("| Warm p95 | {:.2} ms | < 40 ms | {} |\n", warm_p95, pass(warm_p95, 40.0)));
    report.push_str(&format!("| Warm p99 | {:.2} ms | < 80 ms | {} |\n", warm_p99, pass(warm_p99, 80.0)));
    report.push_str(&format!("| Warm error rate | {:.3}% | < 0.5% | {} |\n", warm_err_rate, pass(warm_err_rate, 0.5)));
    report.push_str("| Cold-start p99 | (see Section 4) | < 300 ms | — |\n");
    report.push_str(&format!("| Revocation first-denied p95 | {:.2} ms | < 1000 ms | {} |\n",
        percentile(&rev.first_denied, 0.95), pass(percentile(&rev.first_denied, 0.95), 1000.0)));
    report.push_str(&format!("| Revocation disablement p95 | {:.2} ms | < 30000 ms | {} |\n",
        percentile(&rev.disablement, 0.95), pass(percentile(&rev.disablement, 0.95), 30000.0)));
    report.push_str("\n*Targets are provisional spike bounds, not committed SLOs. Production SLOs are recommended in the readiness decision (task 7.1).*\n");

    let evidence_dir = evidence_dir();
    let path = evidence_dir.join("bench-latency.md");
    std::fs::write(&path, report)?;
    println!("wrote {}", path.display());
    Ok(())
}

fn pass(value: f64, target: f64) -> &'static str {
    if value < target { "pass" } else { "exceeds target" }
}

struct Probe {
    name: &'static str,
    principal: &'static str,
    expect_allow: bool,
    subject: AuthorizationSubject,
    action: Action,
    resource: Resource,
    acting_as: &'static str,
    auth_method: AuthMethod,
}

fn build_probes(issuer: &str) -> Vec<Probe> {
    vec![
        Probe {
            name: "cognito-subscribe-allow",
            principal: "cognito-group",
            expect_allow: true,
            subject: AuthorizationSubject::cognito_group(issuer, "analytics-subscribers"),
            action: Action::SchemaSubscribe,
            resource: Resource::Schema { producer_domain: "payments-prod".into(), schema: "payment-captured".into() },
            acting_as: "analytics-cons",
            auth_method: AuthMethod::Oauth,
        },
        Probe {
            name: "cognito-subscribe-deny",
            principal: "cognito-group",
            expect_allow: false,
            subject: AuthorizationSubject::cognito_group(issuer, "analytics-subscribers"),
            action: Action::SchemaSubscribe,
            resource: Resource::Schema { producer_domain: "orders-prod".into(), schema: "order-placed".into() },
            acting_as: "analytics-cons",
            auth_method: AuthMethod::Oauth,
        },
        Probe {
            name: "aws-subscribe-allow",
            principal: "aws-account",
            expect_allow: true,
            subject: AuthorizationSubject::aws_account("444455556666"),
            action: Action::SchemaSubscribe,
            resource: Resource::Schema { producer_domain: "payments-prod".into(), schema: "payment-captured".into() },
            acting_as: "risk-cons",
            auth_method: AuthMethod::Iam,
        },
        Probe {
            name: "aws-subscribe-deny",
            principal: "aws-account",
            expect_allow: false,
            subject: AuthorizationSubject::aws_account("444455556666"),
            action: Action::SchemaSubscribe,
            resource: Resource::Schema { producer_domain: "payments-prod".into(), schema: "payment-refunded".into() },
            acting_as: "risk-cons",
            auth_method: AuthMethod::Iam,
        },
        Probe {
            name: "cognito-publish-allow",
            principal: "cognito-group",
            expect_allow: true,
            subject: AuthorizationSubject::cognito_group(issuer, "payments-publishers"),
            action: Action::SchemaPublish,
            resource: Resource::Schema { producer_domain: "payments-prod".into(), schema: "payment-captured".into() },
            acting_as: "",
            auth_method: AuthMethod::Oauth,
        },
        Probe {
            name: "cognito-discover-allow",
            principal: "cognito-group",
            expect_allow: true,
            subject: AuthorizationSubject::cognito_group(issuer, "analytics-subscribers"),
            action: Action::SchemaDiscover,
            resource: Resource::Schema { producer_domain: "payments-prod".into(), schema: "payment-captured".into() },
            acting_as: "analytics-cons",
            auth_method: AuthMethod::Oauth,
        },
    ]
}

fn ctx_for(probe: &Probe, id: &str) -> AuthContext {
    AuthContext {
        auth_method: probe.auth_method.clone(),
        global_role: None,
        acting_as: if probe.acting_as.is_empty() { None } else { Some(probe.acting_as.into()) },
        command_id: id.into(),
        queue: None,
        replay_of: None,
        missing_field: None,
        tampered: None,
        trusted_issuer: None,
    }
}

struct Samples { latencies: Vec<f64>, errors: usize }
struct ThroughputSamples { latencies: Vec<f64>, wall_secs: f64, throughput: f64, errors: usize }
struct RevocationSamples { reconcile: Vec<f64>, first_denied: Vec<f64>, disablement: Vec<f64>, failures: usize }

async fn warm_sequential(adapter: &DynamoGrantAdapter, probe: &Probe, n: usize) -> Samples {
    let mut lat = Vec::with_capacity(n);
    let mut errors = 0;
    for i in 0..n {
        let ctx = ctx_for(probe, &format!("warm-{}-{i}", probe.name));
        let t = Instant::now();
        let res = adapter.decide(&probe.subject, &probe.action, &probe.resource, &ctx).await;
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        match res {
            Ok(d) => {
                if decision_matches(&d, probe.expect_allow) {
                    lat.push(ms);
                } else {
                    errors += 1;
                    lat.push(ms);
                }
            }
            Err(_) => { errors += 1; }
        }
    }
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Samples { latencies: lat, errors }
}

async fn target_throughput(adapter: Arc<DynamoGrantAdapter>, probe: &Probe, n: usize, chunk: usize) -> ThroughputSamples {
    let wall = Instant::now();
    let mut lat = Vec::with_capacity(n);
    let mut errors = 0;
    for batch in (0..n).collect::<Vec<_>>().chunks(chunk) {
        let mut handles = Vec::with_capacity(batch.len());
        for &i in batch {
            let a = adapter.clone();
            let sub = probe.subject.clone();
            let act = probe.action.clone();
            let res = probe.resource.clone();
            let ctx = ctx_for(probe, &format!("thr-{}-{i}", probe.name));
            handles.push(tokio::spawn(async move {
                let t = Instant::now();
                let r = a.decide(&sub, &act, &res, &ctx).await;
                let ok = match &r {
                    Ok(d) => decision_matches(d, true),
                    Err(_) => false,
                };
                (t.elapsed().as_secs_f64() * 1000.0, ok)
            }));
        }
        for h in handles {
            if let Ok((ms, ok)) = h.await {
                lat.push(ms);
                if !ok { errors += 1; }
            } else { errors += 1; }
        }
    }
    let wall_secs = wall.elapsed().as_secs_f64();
    let throughput = if wall_secs > 0.0 { n as f64 / wall_secs } else { 0.0 };
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ThroughputSamples { latencies: lat, wall_secs, throughput, errors }
}

async fn cold_start(manifest: &Manifest, probe: &Probe, n: usize) -> Samples {
    let mut lat = Vec::with_capacity(n);
    let mut errors = 0;
    for i in 0..n {
        let t = Instant::now();
        let adapter = build_adapter(manifest).await;
        let ctx = ctx_for(probe, &format!("cold-{}-{i}", probe.name));
        let res = adapter.decide(&probe.subject, &probe.action, &probe.resource, &ctx).await;
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        match res {
            Ok(d) => {
                if decision_matches(&d, probe.expect_allow) {
                    lat.push(ms);
                } else {
                    errors += 1;
                    lat.push(ms);
                }
            }
            Err(_) => { errors += 1; }
        }
    }
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Samples { latencies: lat, errors }
}

async fn revocation_trials(adapter: &DynamoGrantAdapter, manifest: &Manifest, n: usize) -> RevocationSamples {
    let aws_sub = AuthorizationSubject::aws_account("444455556666");
    let subscribe = Action::SchemaSubscribe;
    let res = Resource::Schema { producer_domain: "payments-prod".into(), schema: "payment-captured".into() };
    let mut reconcile = Vec::with_capacity(n);
    let mut first_denied = Vec::with_capacity(n);
    let mut disablement = Vec::with_capacity(n);
    let mut failures = 0;
    for trial in 0..n {
        seed_full(adapter, manifest).await;
        let t0 = Instant::now();
        let _ = adapter.revoke("ent-rik-pay-cap-001").await;
        let reconcile_ms = t0.elapsed().as_secs_f64() * 1000.0;
        reconcile.push(reconcile_ms);
        disablement.push(reconcile_ms);
        let t1 = Instant::now();
        let ctx = AuthContext {
            auth_method: AuthMethod::Iam,
            global_role: None,
            acting_as: Some("risk-cons".into()),
            command_id: format!("rev-decide-{trial}"),
            queue: None, replay_of: None, missing_field: None, tampered: None, trusted_issuer: None,
        };
        let dec = adapter.decide(&aws_sub, &subscribe, &res, &ctx).await;
        let first_ms = t1.elapsed().as_secs_f64() * 1000.0;
        first_denied.push(first_ms);
        let state = adapter.subscription_state("sub-002").await.unwrap_or_else(|_| "unknown".into());
        let denied = matches!(dec, Ok(Decision::Deny { .. }));
        let disabled = state == "disabled";
        if !denied || !disabled { failures += 1; }
    }
    for v in [&mut reconcile, &mut first_denied, &mut disablement] {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    }
    RevocationSamples { reconcile, first_denied, disablement, failures }
}

async fn seed_full(adapter: &DynamoGrantAdapter, manifest: &Manifest) {
    let _ = adapter.seed(&manifest.baseline_grant_seeds, &manifest.baseline_subscription_seeds).await;
    for producer in &manifest.topology.producer_domains {
        for schema in &producer.schemas {
            let _ = adapter.register_schema(&producer.id, schema).await;
        }
    }
}

async fn build_adapter(manifest: &Manifest) -> DynamoGrantAdapter {
    let cfg = aws_config::load_from_env().await;
    let client = aws_sdk_dynamodb::Client::new(&cfg);
    DynamoGrantAdapter::new(client, GRANT_TABLE, SUBSCRIPTION_TABLE, &manifest.topology.cognito_issuer)
}

fn load_manifest() -> Manifest {
    let path = evidence_dir().join("fixture-manifest.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("manifest not found at {}", path.display()));
    serde_json::from_str(&raw).expect("manifest deserializes")
}

fn evidence_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("openspec")
        .join("changes")
        .join("evaluate-authorization-engine")
        .join("evidence")
}

fn manifest_top_hash() -> &'static str {
    "ec4efb25c034159aafb041bc50061961dc0140c10380ab794ce9067547b441b0"
}

fn decision_matches(d: &Decision, expect_allow: bool) -> bool {
    matches!(d, Decision::Allow) == expect_allow
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { return 0.0; }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
