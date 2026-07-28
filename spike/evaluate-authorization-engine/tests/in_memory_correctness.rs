use std::path::PathBuf;

use auth_engine_spike::adapter::GrantAdapter;
use auth_engine_spike::in_memory::InMemoryGrantAdapter;
use auth_engine_spike::manifest::Manifest;
use auth_engine_spike::runner::ScenarioRunner;

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

#[tokio::test]
async fn all_scenarios_pass_against_in_memory_adapter() {
    let manifest = load_manifest();
    let issuer = manifest.topology.cognito_issuer.clone();
    let adapter = InMemoryGrantAdapter::with_trusted_issuer(&issuer);
    let runner = ScenarioRunner::new(&manifest, &adapter);
    let results = runner.run_all().await;

    assert_eq!(
        results.len(),
        manifest.scenarios.len(),
        "runner returned one result per scenario"
    );

    let failures: Vec<_> = results
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
        .collect();

    assert!(
        failures.is_empty(),
        "scenario failures:\n{}",
        failures.join("\n")
    );
}

#[tokio::test]
async fn s08_producer_growth_distinguishes_registered_from_unknown() {
    let manifest = load_manifest();
    let issuer = manifest.topology.cognito_issuer.clone();
    let adapter = InMemoryGrantAdapter::with_trusted_issuer(&issuer);
    let runner = ScenarioRunner::new(&manifest, &adapter);
    let results = runner.run_all().await;

    let s08 = results
        .iter()
        .find(|r| r.scenario_id.contains("S08"))
        .expect("S08 scenario exists");
    assert!(s08.passed, "S08 producer-growth scenario must pass");

    let s12 = results
        .iter()
        .find(|r| r.scenario_id.contains("S12"))
        .expect("S12 scenario exists");
    assert!(s12.passed, "S12 non-disclosure scenario must pass");
}

#[tokio::test]
async fn s09_additive_grants_union_and_removal() {
    let manifest = load_manifest();
    let issuer = manifest.topology.cognito_issuer.clone();
    let adapter = InMemoryGrantAdapter::with_trusted_issuer(&issuer);
    let runner = ScenarioRunner::new(&manifest, &adapter);
    let results = runner.run_all().await;

    let s09 = results
        .iter()
        .find(|r| r.scenario_id.contains("S09"))
        .expect("S09 scenario exists");
    assert!(s09.passed, "S09 additive-grants scenario must pass");
}

#[tokio::test]
async fn s10_revocation_disables_uncovered_subscription() {
    let manifest = load_manifest();
    let issuer = manifest.topology.cognito_issuer.clone();
    let adapter = InMemoryGrantAdapter::with_trusted_issuer(&issuer);
    let runner = ScenarioRunner::new(&manifest, &adapter);
    let results = runner.run_all().await;

    let s10 = results
        .iter()
        .find(|r| r.scenario_id.contains("S10"))
        .expect("S10 scenario exists");
    assert!(s10.passed, "S10 revocation scenario must pass");
}
