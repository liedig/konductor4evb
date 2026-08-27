use anyhow::Result;
use serde_json::Value;

use konductor_common::error::ControlPlaneError;

pub const EVENT_SOURCE: &str = "konductor.event-streams";

/// Emits a `{Operation}Succeeded|Failed` outcome event on the control-plane
/// bus. Deterministic validation failures are reported this way and the SQS
/// message is acknowledged — retrying cannot change the outcome.
pub async fn emit(
    eb: &aws_sdk_eventbridge::Client,
    cp_bus: &str,
    detail_type: &str,
    detail: Value,
) -> Result<()> {
    let result = eb
        .put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(cp_bus)
                .source(EVENT_SOURCE)
                .detail_type(detail_type)
                .detail(detail.to_string())
                .build(),
        )
        .send()
        .await
        .map_err(|e| ControlPlaneError::EventBridge(e.to_string()))?;

    if result.failed_entry_count > 0 {
        return Err(ControlPlaneError::EventBridge(format!(
            "{} {detail_type} event entries failed to publish",
            result.failed_entry_count
        ))
        .into());
    }

    Ok(())
}
