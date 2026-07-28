//! Cloud correctness suite — exercises `DynamoGrantAdapter` against real DynamoDB.
//!
//! Gated with `#[ignore]` so the default `cargo test` invocation (no AWS credentials)
//! is unaffected. Run explicitly:
//!
//! ```sh
//! AWS_PROFILE=default AWS_REGION=ap-southeast-2 \
//!   rtk cargo test -p auth-engine-spike --test dynamo_correctness -- --ignored --nocapture
//! ```
//!
//! Requires the sandbox tables `nexus-spike-auth-grant` and `nexus-spike-auth-sub`
//! (provisioned by the spike migration step) to exist in the target account/Region.

use std::path::PathBuf;

use auth_engine_spike::adapter::GrantAdapter;
use auth_engine_spike::dynamo::DynamoGrantAdapter;
use auth_engine_spike::manifest::Manifest;
use auth_engine_spike::runner::ScenarioRunner;

const GRANT_TABLE: &str = "nexus-spike-auth-grant";
const SUBSCRIPTION_TABLE: &str = "nexus-spike-auth-sub";

fn load_manifest() -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("openspec")
        .join("changes")
        .join("evaluate-authorization-engine")
        .join("evidence")
        .join("fixture-manifest.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("manifest not found at {}", path.display()));
    serde_json::from_str(&raw).expect("manifest deserializes")
}

async fn build_adapter(manifest: &Manifest) -> DynamoGrantAdapter {
    let cfg = aws_config::load_from_env().await;
    let client = aws_sdk_dynamodb::Client::new(&cfg);
    DynamoGrantAdapter::new(
        client,
        GRANT_TABLE,
        SUBSCRIPTION_TABLE,
        &manifest.topology.cognito_issuer,
    )
}

fn failing_steps(results: &[auth_engine_spike::evidence::ScenarioResult]) -> Vec<String> {
    results
        .iter()
        .filter(|r| !r.passed)
        .flat_map(|r| {
            r.step_results.iter().filter(|s| !s.pass).map(|s| {
                format!(
                    "{} / {:?}: expected {:?}, observed {:?}",
                    r.scenario_id, s.label, s.expected, s.observed
                )
            })
        })
        .collect()
}

#[tokio::test]
#[ignore = "requires AWS credentials and provisioned sandbox tables"]
async fn all_scenarios_pass_against_dynamo_adapter() {
    let manifest = load_manifest();
    let adapter = build_adapter(&manifest).await;
    let runner = ScenarioRunner::new(&manifest, &adapter);
    let results = runner.run_all().await;

    assert_eq!(
        results.len(),
        manifest.scenarios.len(),
        "runner returned one result per scenario"
    );

    let failures = failing_steps(&results);
    assert!(
        failures.is_empty(),
        "cloud scenario failures:\n{}",
        failures.join("\n")
    );

    let passed = results.iter().filter(|r| r.passed).count();
    println!(
        "DynamoDB correctness: {}/{} scenarios passed",
        passed,
        results.len()
    );
}

#[tokio::test]
#[ignore = "requires AWS credentials and provisioned sandbox tables"]
async fn s10_revocation_reconciles_subscriptions_in_dynamo() {
    let manifest = load_manifest();
    let adapter = build_adapter(&manifest).await;
    let runner = ScenarioRunner::new(&manifest, &adapter);
    let results = runner.run_all().await;

    let s10 = results
        .iter()
        .find(|r| r.scenario_id.contains("S10"))
        .expect("S10 scenario exists");
    assert!(
        s10.passed,
        "S10 must pass against DynamoDB (revocation disables uncovered subscription, preserves \
         overlapping): failing steps: {:?}",
        s10.step_results
            .iter()
            .filter(|s| !s.pass)
            .map(|s| format!("{:?} expected={:?} observed={:?}", s.label, s.expected, s.observed))
            .collect::<Vec<_>>()
    );
}
