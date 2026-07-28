use std::path::Path;

use auth_engine_spike::adapter::{AdapterError, GrantAdapter};
use auth_engine_spike::contract::{
    Action, AuthContext, AuthMethod, AuthorizationSubject, Decision, Resource,
};
use auth_engine_spike::manifest::{GrantSeed, Manifest, SubscriptionSeed};
use auth_engine_spike::runner::ScenarioRunner;

struct StubAdapter;

impl GrantAdapter for StubAdapter {
    async fn seed(&self, _: &[GrantSeed], _: &[SubscriptionSeed]) -> Result<(), AdapterError> {
        Ok(())
    }

    async fn decide(
        &self,
        _: &AuthorizationSubject,
        _: &Action,
        _: &Resource,
        _: &AuthContext,
    ) -> Result<Decision, AdapterError> {
        Ok(Decision::Allow)
    }

    async fn revoke(&self, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }

    async fn register_schema(&self, _: &str, _: &str) -> Result<(), AdapterError> {
        Ok(())
    }

    async fn subscription_state(&self, _: &str) -> Result<String, AdapterError> {
        Ok("active".to_string())
    }

    async fn discover_list(
        &self,
        _: &AuthorizationSubject,
        _: &str,
        _: &AuthContext,
    ) -> Result<Vec<String>, AdapterError> {
        Ok(Vec::new())
    }
}

fn manifest_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../openspec/changes/evaluate-authorization-engine/evidence/fixture-manifest.json")
}

fn load_manifest() -> Manifest {
    let raw = std::fs::read_to_string(manifest_path()).expect("manifest readable");
    serde_json::from_str(&raw).expect("manifest parses")
}

#[test]
fn manifest_loads_with_expected_inventory() {
    let manifest = load_manifest();
    assert_eq!(manifest.manifest_version, "1.0");
    assert_eq!(manifest.change, "evaluate-authorization-engine");
    assert_eq!(manifest.scenarios.len(), 13);
    assert!(manifest.baseline_grant_seeds.len() >= 9);
    assert_eq!(manifest.baseline_subscription_seeds.len(), 3);
    assert!(manifest.topology.producer_domains.len() >= 2);
}

#[test]
fn manifest_action_set_is_closed() {
    let manifest = load_manifest();
    let allowed = manifest.action_set;
    for scenario in &manifest.scenarios {
        let actions: Vec<&str> = scenario
            .action
            .as_ref()
            .into_iter()
            .filter_map(|a| action_str(a))
            .chain(
                scenario
                    .steps
                    .iter()
                    .filter_map(|s| s.action.as_ref())
                    .filter_map(|a| action_str(a)),
            )
            .chain(
                scenario
                    .steps
                    .iter()
                    .filter_map(|s| s.enqueue.as_ref())
                    .flat_map(|enqueue| action_str(&enqueue.action)),
            )
            .collect();
        for action in actions {
            assert!(
                allowed.iter().any(|a| a == action),
                "scenario {} uses action {} not in action_set",
                scenario.id,
                action
            );
        }
    }
}

fn action_str(action: &Action) -> Option<&'static str> {
    match action {
        Action::PlatformAdmin => Some("platform:admin"),
        Action::PlatformRead => Some("platform:read"),
        Action::DomainRegister => Some("domain:register"),
        Action::SchemaPublish => Some("schema:publish"),
        Action::SchemaSubscribe => Some("schema:subscribe"),
        Action::SchemaDiscover => Some("schema:discover"),
        Action::SubscriptionAdmin => Some("subscription:admin"),
    }
}

#[tokio::test]
async fn runner_executes_every_scenario_against_stub() {
    let manifest = load_manifest();
    let adapter = StubAdapter;
    let runner = ScenarioRunner::new(&manifest, &adapter);
    let results = runner.run_all().await;
    assert_eq!(results.len(), manifest.scenarios.len());
    for result in &results {
        assert!(
            !result.step_results.is_empty(),
            "scenario {} produced no step results",
            result.scenario_id
        );
    }
}

#[test]
fn stub_adapter_compiles_and_responds() {
    let adapter = StubAdapter;
    let subject = AuthorizationSubject::cognito_group("issuer", "group");
    let context = AuthContext {
        auth_method: AuthMethod::Oauth,
        global_role: None,
        acting_as: None,
        command_id: "cmd-test".to_string(),
        queue: None,
        replay_of: None,
        missing_field: None,
        tampered: None,
        trusted_issuer: None,
    };
    let resource = Resource::Domain {
        id: "marketing-prod".to_string(),
    };
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let decision = rt
        .block_on(adapter.decide(&subject, &Action::PlatformAdmin, &resource, &context))
        .expect("stub decide");
    assert_eq!(decision, Decision::Allow);
}
