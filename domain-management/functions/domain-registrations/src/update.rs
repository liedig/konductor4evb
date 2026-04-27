use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::env;

use nexus_common::dynamodb::{get_s, s, string_map_to_attr};
use nexus_common::error::ControlPlaneError;
use nexus_common::models::DomainStatus;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateDomainMessage {
    pub domain_id: String,
    #[serde(default)]
    pub body: UpdateDomainBody,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateDomainBody {
    pub description: Option<String>,
    pub tags: Option<HashMap<String, String>>,
}

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    eb: &aws_sdk_eventbridge::Client,
    message: &UpdateDomainMessage,
) -> Result<()> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let cp_bus = env::var("NEXUS_EVENT_BUS").context("NEXUS_EVENT_BUS not set")?;
    let domain_id = &message.domain_id;

    let existing = ddb
        .get_item()
        .table_name(&table)
        .key("domainId", s(domain_id))
        .send()
        .await
        .map_err(|e| ControlPlaneError::DynamoDb(e.to_string()))?;

    let item = match existing.item {
        Some(item) => item,
        None => {
            return emit_failed(eb, &cp_bus, domain_id, "domain not found").await;
        }
    };

    let current_status = get_s(&item, "status")
        .as_deref()
        .and_then(|status| status.parse::<DomainStatus>().ok());

    if matches!(current_status, Some(DomainStatus::Inactive)) {
        return emit_failed(
            eb,
            &cp_bus,
            domain_id,
            "domain is INACTIVE and cannot be modified",
        )
        .await;
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut update_expr_parts = vec!["updatedAt = :now".to_string()];
    let mut expr_values = vec![(":now".to_string(), s(&now))];

    if let Some(desc) = message.body.description.as_deref() {
        update_expr_parts.push("description = :desc".to_string());
        expr_values.push((":desc".to_string(), s(desc)));
    }

    if let Some(tags) = &message.body.tags {
        update_expr_parts.push("tags = :tags".to_string());
        expr_values.push((":tags".to_string(), string_map_to_attr(tags)));
    }

    let update_expression = format!("SET {}", update_expr_parts.join(", "));

    let mut update = ddb
        .update_item()
        .table_name(&table)
        .key("domainId", s(domain_id))
        .update_expression(&update_expression)
        .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew);

    for (k, v) in expr_values {
        update = update.expression_attribute_values(k, v);
    }

    let attrs = match update
        .send()
        .await
        .map_err(|e| ControlPlaneError::DynamoDb(e.to_string()))?
        .attributes
    {
        Some(a) => a,
        None => return emit_failed(eb, &cp_bus, domain_id, "domain not found after update").await,
    };

    eb.put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(&cp_bus)
                .source("nexus.domain-management")
                .detail_type("UpdateDomainSucceeded")
                .detail(
                    json!({
                        "domainId": domain_id,
                        "updatedAt": now,
                        "status": get_s(&attrs, "status"),
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
    domain_id: &str,
    reason: &str,
) -> Result<()> {
    eb.put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(cp_bus)
                .source("nexus.domain-management")
                .detail_type("UpdateDomainFailed")
                .detail(
                    json!({
                        "domainId": domain_id,
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

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
    use aws_sdk_eventbridge::operation::put_events::PutEventsOutput;
    use aws_smithy_mocks::{mock, mock_client, RuleMode};
    use std::collections::HashMap;

    fn update_message(domain_id: &str, description: Option<&str>) -> UpdateDomainMessage {
        UpdateDomainMessage {
            domain_id: domain_id.to_string(),
            body: UpdateDomainBody {
                description: description.map(std::string::ToString::to_string),
                tags: None,
            },
        }
    }

    fn active_domain_item() -> HashMap<String, aws_sdk_dynamodb::types::AttributeValue> {
        HashMap::from([
            ("domainId".into(), s("d-123")),
            ("domainName".into(), s("billing")),
            ("ownerAccountId".into(), s("111122223333")),
            ("status".into(), s("ACTIVE")),
        ])
    }

    fn set_env() {
        env::set_var("DOMAINS_TABLE", "domains");
        env::set_var("NEXUS_EVENT_BUS", "nexus-bus");
    }

    #[tokio::test]
    async fn updates_domain_and_emits_success_event() {
        set_env();

        let item = active_domain_item();
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        });

        let updated_attrs = HashMap::from([
            ("domainId".into(), s("d-123")),
            ("status".into(), s("ACTIVE")),
            ("description".into(), s("Updated billing domain")),
        ]);
        let update_rule = mock!(aws_sdk_dynamodb::Client::update_item).then_output(move || {
            UpdateItemOutput::builder()
                .set_attributes(Some(updated_attrs.clone()))
                .build()
        });

        let ddb = mock_client!(
            aws_sdk_dynamodb,
            RuleMode::Sequential,
            [&get_rule, &update_rule]
        );

        let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| PutEventsOutput::builder().build());
        let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

        let message = update_message("d-123", Some("Updated billing domain"));
        let result = handle(&ddb, &eb, &message).await;

        assert!(result.is_ok());
        assert_eq!(get_rule.num_calls(), 1);
        assert_eq!(update_rule.num_calls(), 1);
        assert_eq!(eb_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn emits_failed_event_when_domain_not_found() {
        set_env();

        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| GetItemOutput::builder().build());
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| PutEventsOutput::builder().build());
        let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

        let message = update_message("d-nonexistent", None);
        let result = handle(&ddb, &eb, &message).await;

        assert!(result.is_ok());
        assert_eq!(eb_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn rejects_update_on_inactive_domain() {
        set_env();

        let mut item = active_domain_item();
        item.insert("status".into(), s("INACTIVE"));

        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| PutEventsOutput::builder().build());
        let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

        let message = update_message("d-123", Some("Should not work"));
        let result = handle(&ddb, &eb, &message).await;

        assert!(result.is_ok());
        assert_eq!(eb_rule.num_calls(), 1);
    }

    #[test]
    fn update_message_deserialization_rejects_unknown_fields() {
        let message = serde_json::json!({
            "domainId": "d-123",
            "body": {
                "description": "test",
                "unknownField": "x"
            }
        });

        let parsed = serde_json::from_value::<UpdateDomainMessage>(message);
        assert!(parsed.is_err());
    }
}
