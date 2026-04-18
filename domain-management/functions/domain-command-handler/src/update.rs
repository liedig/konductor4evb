use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::env;

use nexus_common::dynamodb::{get_s, s, string_map_to_attr};
use nexus_common::error::ControlPlaneError;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    eb: &aws_sdk_eventbridge::Client,
    message: &Value,
) -> Result<()> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let cp_bus = env::var("NEXUS_EVENT_BUS").context("NEXUS_EVENT_BUS not set")?;

    let domain_id = message
        .get("domainId")
        .and_then(|v| v.as_str())
        .context("missing domainId in message")?
        .to_string();

    let empty = Value::Object(Default::default());
    let body = message.get("body").unwrap_or(&empty);

    let existing = ddb
        .get_item()
        .table_name(&table)
        .key("domainId", s(&domain_id))
        .send()
        .await
        .map_err(|e| ControlPlaneError::DynamoDb(e.to_string()))?;

    let item = match existing.item {
        Some(item) => item,
        None => {
            return emit_failed(eb, &cp_bus, &domain_id, "domain not found").await;
        }
    };

    let current_status = get_s(&item, "status").unwrap_or_default();

    if current_status == "DEACTIVATED" {
        return emit_failed(
            eb,
            &cp_bus,
            &domain_id,
            "domain is DEACTIVATED and cannot be modified",
        )
        .await;
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut update_expr_parts = vec!["updatedAt = :now".to_string()];
    let mut expr_values = vec![(":now".to_string(), s(&now))];

    if let Some(desc) = body.get("description").and_then(|v| v.as_str()) {
        update_expr_parts.push("description = :desc".to_string());
        expr_values.push((":desc".to_string(), s(desc)));
    }

    if let Some(tags) = body.get("tags").and_then(|v| v.as_object()) {
        let tag_map: std::collections::HashMap<String, String> = tags
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect();
        update_expr_parts.push("tags = :tags".to_string());
        expr_values.push((":tags".to_string(), string_map_to_attr(&tag_map)));
    }

    let update_expression = format!("SET {}", update_expr_parts.join(", "));

    let mut update = ddb
        .update_item()
        .table_name(&table)
        .key("domainId", s(&domain_id))
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
        None => return emit_failed(eb, &cp_bus, &domain_id, "domain not found after update").await,
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

        let message = json!({
            "domainId": "d-123",
            "body": {
                "description": "Updated billing domain"
            }
        });

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

        let message = json!({
            "domainId": "d-nonexistent",
            "body": {}
        });

        let result = handle(&ddb, &eb, &message).await;

        assert!(result.is_ok());
        assert_eq!(eb_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn rejects_update_on_deactivated_domain() {
        set_env();

        let mut item = active_domain_item();
        item.insert("status".into(), s("DEACTIVATED"));

        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| PutEventsOutput::builder().build());
        let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

        let message = json!({
            "domainId": "d-123",
            "body": {
                "description": "Should not work"
            }
        });

        let result = handle(&ddb, &eb, &message).await;

        assert!(result.is_ok());
        assert_eq!(eb_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn returns_error_when_domain_id_missing() {
        set_env();

        let ddb = mock_client!(aws_sdk_dynamodb, &[]);
        let eb = mock_client!(aws_sdk_eventbridge, &[]);

        let message = json!({"body": {}});

        let result = handle(&ddb, &eb, &message).await;

        assert!(result.is_err());
    }
}
