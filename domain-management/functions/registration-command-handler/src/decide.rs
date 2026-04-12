use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::env;

use control_plane_common::dynamodb::{get_s, s};
use control_plane_common::error::ControlPlaneError;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    sfn: &aws_sdk_sfn::Client,
    eb: &aws_sdk_eventbridge::Client,
    message: &Value,
) -> Result<()> {
    let table = env::var("REGISTRATION_REQUESTS_TABLE")
        .context("REGISTRATION_REQUESTS_TABLE not set")?;
    let cp_bus = env::var("CONTROL_PLANE_EVENT_BUS").context("CONTROL_PLANE_EVENT_BUS not set")?;

    let request_id = message
        .get("requestId")
        .and_then(|v| v.as_str())
        .context("missing requestId in message")?
        .to_string();

    let empty = Value::Object(Default::default());
    let body = message.get("body").unwrap_or(&empty);

    let decision = match body.get("decision").and_then(|v| v.as_str()) {
        Some("ACCEPT") => "ACCEPT",
        Some("REJECT") => "REJECT",
        other => {
            let reason = format!("invalid decision value: {:?}", other);
            return emit_failed(eb, &cp_bus, &request_id, &reason).await;
        }
    };
    let approved = decision == "ACCEPT";
    let reason = body.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let request_item = ddb
        .get_item()
        .table_name(&table)
        .key("requestId", s(&request_id))
        .send()
        .await
        .map_err(|e| ControlPlaneError::DynamoDb(e.to_string()))?;

    let item = match request_item.item {
        Some(item) => item,
        None => {
            return emit_failed(eb, &cp_bus, &request_id, "registration request not found").await;
        }
    };

    let current_status = get_s(&item, "status").unwrap_or_default();
    if current_status != "PENDING_APPROVAL" {
        let reason = format!("request is in status {current_status}, expected PENDING_APPROVAL");
        return emit_failed(eb, &cp_bus, &request_id, &reason).await;
    }

    let task_token = match get_s(&item, "taskToken") {
        Some(t) => t,
        None => {
            return emit_failed(eb, &cp_bus, &request_id, "no task token found for request").await;
        }
    };

    let now = chrono::Utc::now().to_rfc3339();

    let callback_output = json!({
        "requestId": request_id,
        "domainId": get_s(&item, "domainId"),
        "domainName": get_s(&item, "domainName"),
        "ownerAccountId": get_s(&item, "ownerAccountId"),
        "approved": approved,
        "reason": reason,
    });

    if approved {
        sfn.send_task_success()
            .task_token(&task_token)
            .output(callback_output.to_string())
            .send()
            .await
            .map_err(|e| ControlPlaneError::StepFunctions(e.to_string()))?;
    } else {
        sfn.send_task_failure()
            .task_token(&task_token)
            .error("RegistrationRejected")
            .cause(callback_output.to_string())
            .send()
            .await
            .map_err(|e| ControlPlaneError::StepFunctions(e.to_string()))?;
    }

    let decision_result = if approved { "ACCEPTED" } else { "REJECTED" };

    eb.put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(&cp_bus)
                .source("controlplane.domain-management")
                .detail_type("DecideRegistrationSucceeded")
                .detail(json!({
                    "requestId": request_id,
                    "decision": decision_result,
                    "decidedAt": now,
                }).to_string())
                .build(),
        )
        .send()
        .await
        .map_err(|e| ControlPlaneError::EventBridge(e.to_string()))?;

    Ok(())
}

async fn emit_failed(eb: &aws_sdk_eventbridge::Client, cp_bus: &str, request_id: &str, reason: &str) -> Result<()> {
    eb.put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(cp_bus)
                .source("controlplane.domain-management")
                .detail_type("DecideRegistrationFailed")
                .detail(json!({
                    "requestId": request_id,
                    "reason": reason,
                    "failedAt": chrono::Utc::now().to_rfc3339(),
                }).to_string())
                .build(),
        )
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("EventBridge error: {e}"))?;
    Ok(())
}
