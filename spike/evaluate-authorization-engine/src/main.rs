use std::path::Path;

use anyhow::Context;

use auth_engine_spike::manifest::Manifest;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../openspec/changes/evaluate-authorization-engine/evidence/fixture-manifest.json",
    );

    let raw = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading fixture manifest at {}", manifest_path.display()))?;
    let manifest: Manifest =
        serde_json::from_str(&raw).with_context(|| "parsing fixture manifest")?;

    println!("Fixture manifest: {}", manifest.change);
    println!("  version:   {}", manifest.manifest_version);
    println!("  frozen at: {}", manifest.frozen_at);
    println!("  scenarios: {}", manifest.scenarios.len());
    println!("  grants:    {}", manifest.baseline_grant_seeds.len());
    println!(
        "  subs:      {}",
        manifest.baseline_subscription_seeds.len()
    );
    println!();
    for scenario in &manifest.scenarios {
        let steps = scenario.steps.len();
        let step_label = if steps > 0 {
            format!(" ({} steps)", steps)
        } else {
            String::new()
        };
        println!("  {} | {}{}", scenario.id, scenario.scenario, step_label);
    }
    println!();
    println!("Adapter wiring is delivered by task 2.2 (DynamoDB grant adapter).");
    Ok(())
}
