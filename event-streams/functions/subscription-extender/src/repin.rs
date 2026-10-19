//! The authoring-environment re-pin trigger: an append in the authoring
//! environment advances that environment's subscription pins, so the
//! extender starts the re-pin workflow when a publish event lands there.

use anyhow::{Context, Result};
use serde_json::json;

use crate::handler::{Clients, PromotionDetail, Settings};

/// The lookahead pattern already admits the new version; the re-pin
/// workflow widens, drains, and narrows the rules so the old version stops
/// matching. The execution name is deterministic per version, so a replayed
/// event (or the same version published twice) is a no-op.
pub(crate) async fn maybe_start_repin(
    clients: &Clients,
    settings: &Settings,
    detail: &PromotionDetail,
) -> Result<()> {
    if !detail.is_authoring(settings) {
        return Ok(());
    }
    let Some(version) = detail.version else {
        tracing::warn!(schema = %detail.schema_name, "publish event without a version — no re-pin");
        return Ok(());
    };
    let event_name = detail.event_name()?;
    let name = format!(
        "repin-{}-{}-{}-v{version}",
        detail.producer_domain, event_name, detail.environment
    );
    let input = json!({
        "domain": detail.producer_domain,
        "schemaName": event_name,
        "environment": detail.environment,
        "version": version,
    });
    let result = clients
        .sfn
        .start_execution()
        .state_machine_arn(&settings.repin_state_machine)
        .name(&name)
        .input(input.to_string())
        .send()
        .await;
    match result {
        Ok(_) => {
            tracing::info!(execution = %name, "authoring-environment re-pin started");
            Ok(())
        }
        Err(error) => {
            let service = error.into_service_error();
            if service.is_execution_already_exists() {
                tracing::info!(execution = %name, "re-pin already running for this version");
                return Ok(());
            }
            Err(anyhow::Error::new(service)).context("failed to start re-pin workflow")
        }
    }
}
