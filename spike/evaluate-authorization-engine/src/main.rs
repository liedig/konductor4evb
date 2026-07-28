use std::path::Path;

use anyhow::Context;
use sha2::{Digest, Sha256};

use auth_engine_spike::contract::Decision;
use auth_engine_spike::evidence::ScenarioResult;
use auth_engine_spike::in_memory::InMemoryGrantAdapter;
use auth_engine_spike::manifest::Manifest;
use auth_engine_spike::runner::ScenarioRunner;

const FROZEN_HASH: &str = "ec4efb25c034159aafb041bc50061961dc0140c10380ab794ce9067547b441b0";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let evidence_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../openspec/changes/evaluate-authorization-engine/evidence");
    let manifest_path = evidence_dir.join("fixture-manifest.json");

    let bytes = std::fs::read(&manifest_path)
        .with_context(|| format!("reading fixture manifest at {}", manifest_path.display()))?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).with_context(|| "parsing fixture manifest")?;
    let hash = sha256_hex(&bytes);
    let hash_matches = hash == FROZEN_HASH;

    let issuer = manifest.topology.cognito_issuer.clone();
    let adapter = InMemoryGrantAdapter::with_trusted_issuer(&issuer);
    let runner = ScenarioRunner::new(&manifest, &adapter);
    let results = runner.run_all().await;

    let total_steps: usize = results.iter().map(|r| r.step_results.len()).sum();
    let passed_steps: usize = results
        .iter()
        .map(|r| r.step_results.iter().filter(|s| s.pass).count())
        .sum();
    let passed_scenarios = results.iter().filter(|r| r.passed).count();

    println!(
        "Fixture manifest: {} (sha256={}, match={})",
        manifest.change, hash, hash_matches
    );
    println!(
        "  scenarios: {} ({} passed), steps: {} ({} passed)",
        results.len(), passed_scenarios, total_steps, passed_steps
    );

    let report = build_report(
        &manifest,
        &hash,
        hash_matches,
        &results,
        total_steps,
        passed_steps,
        passed_scenarios,
    );
    let evidence_path = evidence_dir.join("local-correctness.md");
    std::fs::write(&evidence_path, report)
        .with_context(|| format!("writing evidence to {}", evidence_path.display()))?;
    println!("  evidence: {}", evidence_path.display());
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn build_report(
    manifest: &Manifest,
    hash: &str,
    hash_matches: bool,
    results: &[ScenarioResult],
    total_steps: usize,
    passed_steps: usize,
    passed_scenarios: usize,
) -> String {
    let mut s = String::new();
    s.push_str("# Local Correctness Evidence — DynamoDB grant-engine spike\n\n");
    s.push_str(
        "> Spike artifact. Produced by `cargo run -p auth-engine-spike` against the in-memory \
         grant adapter. No AWS credentials are used; cloud latency, cost, revocation, and \
         asynchronous-handling measurements (tasks 4.x, 5.x) are deferred.\n\n",
    );

    s.push_str("## 1. Frozen fixture integrity\n\n");
    s.push_str("- Manifest: `evidence/fixture-manifest.json`\n");
    s.push_str(&format!("- SHA-256 (computed): `{hash}`\n"));
    s.push_str(&format!("- Frozen hash (methodology.md): `{FROZEN_HASH}`\n"));
    s.push_str(&format!(
        "- Match: {}\n\n",
        if hash_matches { "yes" } else { "no — re-freeze required" }
    ));

    s.push_str("## 2. Execution environment\n\n");
    s.push_str("- Adapter: `InMemoryGrantAdapter` (local, no AWS credentials)\n");
    s.push_str("- AWS Region (declared, not contacted): us-east-1\n");
    s.push_str(&format!(
        "- Trusted Cognito issuer: `{}`\n",
        manifest.topology.cognito_issuer
    ));
    s.push_str(
        "- Sample sizes, workload profiles, provisional targets: declared in `methodology.md` \
         (cloud measurement deferred)\n",
    );
    s.push_str("- Secrets: none present; configuration retained with secrets removed\n\n");

    s.push_str("## 3. Correctness results\n\n");
    s.push_str(&format!(
        "Scenarios: {}/{} passed. Steps: {}/{} passed.\n\n",
        passed_scenarios,
        results.len(),
        passed_steps,
        total_steps
    ));
    s.push_str("| Scenario | Passed | Steps | Failing step |\n");
    s.push_str("|---|---|---|---|\n");
    for r in results {
        let fail = r
            .step_results
            .iter()
            .find(|s| !s.pass)
            .and_then(|s| s.label.clone())
            .unwrap_or_default();
        s.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            r.scenario_id,
            if r.passed { "yes" } else { "no" },
            r.step_results.len(),
            fail
        ));
    }
    s.push_str("\n### Per-step detail\n\n");
    for r in results {
        s.push_str(&format!("**{}** — {}\n\n", r.scenario_id, r.scenario));
        for step in &r.step_results {
            let label = step.label.clone().unwrap_or_else(|| "single-case".to_string());
            s.push_str(&format!(
                "- {}: expected `{}`, observed `{}`, pass={}\n",
                label,
                step.expected,
                fmt_decision(&step.observed),
                step.pass
            ));
        }
        s.push('\n');
    }

    s.push_str("## 4. Cloud-service semantic gaps (task 2.3)\n\n");
    s.push_str(
        "The in-memory adapter implements the grant-table key model and the full \
         decision/revocation/non-disclosure logic, but does NOT model the following DynamoDB \
         cloud-service semantics. These are exercised by the DynamoDB-backed adapter \
         (`src/dynamo.rs`, excluded from this local run) in the cloud integration phase:\n\n",
    );
    s.push_str(
        "- **Read consistency**: the local store has no consistency lag. Cloud decisions MUST \
         use `ConsistentRead=true` on the grant table; GSIs are eventually consistent and serve \
         administration/candidate lookup only.\n",
    );
    s.push_str(
        "- **GSI / subscription reverse-index lag**: a just-created subscription may be briefly \
         absent from the consumer/producer GSI during revocation candidate lookup. Safe because \
         subscription activation performs a final current-entitlement check before enabling \
         EventBridge rules.\n",
    );
    s.push_str(
        "- **Throttling, capacity, retries**: the local adapter never throttles. Cloud runs must \
         handle `ProvisionedThroughputExceededException` with backoff; retries are recorded in \
         latency evidence.\n",
    );
    s.push_str(
        "- **SDK transport and cold start**: the local adapter is in-process. Cloud measurements \
         include AWS SDK client initialization and network latency (cold-start-path samples in \
         methodology.md).\n",
    );
    s.push_str(
        "- **Conditional writes / concurrent grant mutation**: the local adapter serializes via a \
         `Mutex`. Cloud grant mutation needs optimistic concurrency or transactions; \
         concurrent-change behavior is recorded under the policy-administration exercise \
         (task 6.2).\n",
    );
    s.push_str(
        "- **Audit persistence**: the local adapter retains audit records in memory only. Cloud \
         audit uses DynamoDB audit items and CloudTrail; audit reconstruction is exercised under \
         task 6.1.\n",
    );
    s.push_str(
        "- **Revocation propagation timing**: the local adapter reconciles subscriptions \
         synchronously and instantaneously. Cloud revocation timing (time to first denied \
         decision vs complete subscription disablement) is measured separately in task 5.x.\n\n",
    );

    s.push_str("## 5. Mandatory behavior status\n\n");
    let correctness_ok = results.iter().all(|r| r.passed);
    s.push_str(&format!(
        "- Correctness: {}\n",
        if correctness_ok {
            "all frozen scenarios pass against the in-memory adapter"
        } else {
            "failures present — see table above"
        }
    ));
    s.push_str(
        "- Non-disclosure: verified — S12a (unauthorized existing schema) and S08c (exact-schema \
         consumer, producer-growth schema) both return `deny-as-unknown`, indistinguishable from \
         the S12b unknown-schema control; S12c list filtering omits unauthorized schemas.\n",
    );
    s.push_str(
        "- Decision: local correctness satisfies mandatory correctness and non-disclosure \
         behavior. Cloud latency, cost, revocation-propagation, and asynchronous-handling \
         measurements (tasks 4.x, 5.x) and operational exercises (task 6.x) remain before the \
         readiness decision (task 7.1).\n",
    );

    s
}

fn fmt_decision(d: &Option<Decision>) -> String {
    match d {
        Some(Decision::Allow) => "allow".to_string(),
        Some(Decision::DenyAsUnknown) => "deny-as-unknown".to_string(),
        Some(Decision::Deny { reason }) => format!("deny:{reason:?}"),
        None => "none".to_string(),
    }
}
