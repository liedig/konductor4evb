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
    pub domain_name: String,
    #[serde(default)]
    pub body: UpdateDomainBody,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateDomainBody {
    pub description: Option<String>,
    pub tags: Option<HashMap<String, String>>,
    pub owner_email: Option<String>,
    pub secondary_owner_emails: Option<Vec<String>>,
}

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    eb: &aws_sdk_eventbridge::Client,
    message: &UpdateDomainMessage,
) -> Result<()> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let cp_bus = env::var("NEXUS_EVENT_BUS").context("NEXUS_EVENT_BUS not set")?;
    let domain_name = &message.domain_name;

    let existing = ddb
        .get_item()
        .table_name(&table)
        .key("domainName", s(domain_name))
        .send()
        .await
        .map_err(|e| ControlPlaneError::DynamoDb(e.to_string()))?;

    let item = match existing.item {
        Some(item) => item,
        None => {
            return emit_failed(eb, &cp_bus, domain_name, "domain not found").await;
        }
    };

    let current_status = get_s(&item, "status")
        .as_deref()
        .and_then(|status| status.parse::<DomainStatus>().ok());

    if matches!(current_status, Some(DomainStatus::Inactive)) {
        return emit_failed(
            eb,
            &cp_bus,
            domain_name,
            "domain is INACTIVE and cannot be modified",
        )
        .await;
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut update_expr_parts = vec!["updatedAt = :now".to_string()];
    let mut expr_values = vec![(":now".to_string(), s(&now))];
    let mut changed_fields = vec![];

    if let Some(desc) = message.body.description.as_deref() {
        update_expr_parts.push("description = :desc".to_string());
        expr_values.push((":desc".to_string(), s(desc)));
        changed_fields.push("description");
    }

    if let Some(tags) = &message.body.tags {
        update_expr_parts.push("tags = :tags".to_string());
        expr_values.push((":tags".to_string(), string_map_to_attr(tags)));
        changed_fields.push("tags");
    }

    if let Some(email) = message.body.owner_email.as_deref() {
        update_expr_parts.push("ownerEmail = :ownerEmail".to_string());
        expr_values.push((":ownerEmail".to_string(), s(email)));
        changed_fields.push("ownerEmail");
    }

    if let Some(emails) = &message.body.secondary_owner_emails {
        update_expr_parts.push("secondaryOwnerEmails = :secondaryOwnerEmails".to_string());
        expr_values.push((
            ":secondaryOwnerEmails".to_string(),
            aws_sdk_dynamodb::types::AttributeValue::Ss(emails.clone()),
        ));
        changed_fields.push("secondaryOwnerEmails");
    }

    let update_expression = format!("SET {}", update_expr_parts.join(", "));

    let mut update = ddb
        .update_item()
        .table_name(&table)
        .key("domainName", s(domain_name))
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
        None => {
            return emit_failed(eb, &cp_bus, domain_name, "domain not found after update").await
        }
    };

    eb.put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(&cp_bus)
                .source("nexus.domain-management")
                .detail_type("UpdateDomainSucceeded")
                .detail(
                    json!({
                        "domainName": domain_name,
                        "updatedAt": now,
                        "status": get_s(&attrs, "status"),
                        "changedFields": changed_fields,
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
                        "domainName": domain_name,
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

    fn update_message(domain_name: &str, description: Option<&str>) -> UpdateDomainMessage {
        UpdateDomainMessage {
            domain_name: domain_name.to_string(),
            body: UpdateDomainBody {
                description: description.map(std::string::ToString::to_string),
                tags: None,
                owner_email: None,
                secondary_owner_emails: None,
            },
        }
    }

    fn active_domain_item() -> HashMap<String, aws_sdk_dynamodb::types::AttributeValue> {
        HashMap::from([
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
            ("domainName".into(), s("billing")),
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

        let message = update_message("billing", Some("Updated billing domain"));
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

        let message = update_message("nonexistent", None);
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

        let message = update_message("billing", Some("Should not work"));
        let result = handle(&ddb, &eb, &message).await;

        assert!(result.is_ok());
        assert_eq!(eb_rule.num_calls(), 1);
    }

    #[test]
    fn update_message_deserialization_rejects_unknown_fields() {
        let message = serde_json::json!({
            "domainName": "billing",
            "body": {
                "description": "test",
                "unknownField": "x"
            }
        });

        let parsed = serde_json::from_value::<UpdateDomainMessage>(message);
        assert!(parsed.is_err());
    }
}
