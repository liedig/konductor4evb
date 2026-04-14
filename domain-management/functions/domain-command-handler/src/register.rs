use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::env;

use control_plane_common::dynamodb::s;
use control_plane_common::error::ControlPlaneError;
use control_plane_common::models::{DomainStatus, RegisterDomainRequest};

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    eb: &aws_sdk_eventbridge::Client,
    sfn: &aws_sdk_sfn::Client,
    body: &Value,
) -> Result<()> {
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let state_machine_arn =
        env::var("APPROVAL_STATE_MACHINE_ARN").context("APPROVAL_STATE_MACHINE_ARN not set")?;
    let cp_bus = env::var("CONTROL_PLANE_EVENT_BUS").context("CONTROL_PLANE_EVENT_BUS not set")?;

    let request: RegisterDomainRequest = serde_json::from_value(body.clone())
        .context("failed to deserialise RegisterDomainRequest")?;

    if let Err(e) = validate_domain_name(&request.domain_name) {
        emit_failed(
            eb,
            &cp_bus,
            &request.domain_name,
            &request.owner_account_id,
            &request.owner_email,
            &e.to_string(),
        )
        .await?;
        return Ok(());
    }

    let existing = ddb
        .query()
        .table_name(&domains_table)
        .index_name("by-name")
        .key_condition_expression("domainName = :name")
        .expression_attribute_values(":name", s(&request.domain_name))
        .send()
        .await
        .map_err(|e| ControlPlaneError::DynamoDb(e.to_string()))?;

    if existing.count() > 0 {
        let reason = format!("domain '{}' already exists", request.domain_name);
        emit_failed(
            eb,
            &cp_bus,
            &request.domain_name,
            &request.owner_account_id,
            &request.owner_email,
            &reason,
        )
        .await?;
        return Ok(());
    }

    let domain_id = uuid::Uuid::new_v4().to_string();
    let request_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let status = DomainStatus::PendingApproval.to_string();

    let mut put_domain = ddb
        .put_item()
        .table_name(&domains_table)
        .item("domainId", s(&domain_id))
        .item("domainName", s(&request.domain_name))
        .item("ownerAccountId", s(&request.owner_account_id))
        .item("status", s(&status))
        .item("ownerEmail", s(&request.owner_email))
        .item("createdAt", s(&now))
        .item("updatedAt", s(&now))
        .item("createdBy", s(&request.owner_account_id));

    if let Some(desc) = &request.description {
        put_domain = put_domain.item("description", s(desc));
    }

    put_domain
        .send()
        .await
        .map_err(|e| ControlPlaneError::DynamoDb(e.to_string()))?;

    let sfn_input = json!({
        "requestId": request_id,
        "domainId": domain_id,
        "domainName": request.domain_name,
        "ownerAccountId": request.owner_account_id,
        "ownerEmail": request.owner_email,
        "description": request.description,
    });

    sfn.start_execution()
        .state_machine_arn(&state_machine_arn)
        .name(format!("approval-{request_id}"))
        .input(sfn_input.to_string())
        .send()
        .await
        .map_err(|e| ControlPlaneError::StepFunctions(e.to_string()))?;

    eb.put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(&cp_bus)
                .source("controlplane.domain-management")
                .detail_type("RegisterDomainSucceeded")
                .detail(
                    json!({
                        "requestId": request_id,
                        "domainId": domain_id,
                        "domainName": request.domain_name,
                        "ownerAccountId": request.owner_account_id,
                        "ownerEmail": request.owner_email,
                        "status": status,
                        "createdAt": now,
                    })
                    .to_string(),
                )
                .build(),
        )
        .send()
        .await
        .map_err(|e| ControlPlaneError::EventBridge(e.to_string()))?;

    Ok(())
}

async fn emit_failed(
    eb: &aws_sdk_eventbridge::Client,
    cp_bus: &str,
    domain_name: &str,
    owner_account_id: &str,
    owner_email: &str,
    reason: &str,
) -> Result<()> {
    eb.put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(cp_bus)
                .source("controlplane.domain-management")
                .detail_type("RegisterDomainFailed")
                .detail(
                    json!({
                        "domainName": domain_name,
                        "ownerAccountId": owner_account_id,
                        "ownerEmail": owner_email,
                        "reason": reason,
                        "failedAt": chrono::Utc::now().to_rfc3339(),
                    })
                    .to_string(),
                )
                .build(),
        )
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("EventBridge error: {e}"))?;
    Ok(())
}

pub(crate) fn validate_domain_name(name: &str) -> Result<(), ControlPlaneError> {
    if name.len() < 2 || name.len() > 64 {
        return Err(control_plane_common::api::validation_error(
            "Domain name must be between 2 and 64 characters",
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(control_plane_common::api::validation_error(
            "Domain name must contain only lowercase letters, digits, and hyphens",
        ));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(control_plane_common::api::validation_error(
            "Domain name must not start or end with a hyphen",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_domain_names_pass() {
        assert!(validate_domain_name("billing").is_ok());
        assert!(validate_domain_name("my-domain").is_ok());
        assert!(validate_domain_name("a1").is_ok());
        assert!(validate_domain_name("domain-123-test").is_ok());
    }

    #[test]
    fn rejects_name_too_short() {
        assert!(validate_domain_name("a").is_err());
    }

    #[test]
    fn rejects_name_too_long() {
        let long_name = "a".repeat(65);
        assert!(validate_domain_name(&long_name).is_err());
    }

    #[test]
    fn accepts_name_at_boundary_lengths() {
        assert!(validate_domain_name("ab").is_ok());
        assert!(validate_domain_name(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn rejects_uppercase_characters() {
        assert!(validate_domain_name("Billing").is_err());
        assert!(validate_domain_name("MY-DOMAIN").is_err());
    }

    #[test]
    fn rejects_special_characters() {
        assert!(validate_domain_name("billing@v2").is_err());
        assert!(validate_domain_name("my_domain").is_err());
        assert!(validate_domain_name("domain.name").is_err());
    }

    #[test]
    fn rejects_leading_hyphen() {
        assert!(validate_domain_name("-billing").is_err());
    }

    #[test]
    fn rejects_trailing_hyphen() {
        assert!(validate_domain_name("billing-").is_err());
    }
}
