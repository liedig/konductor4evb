use anyhow::{Context, Result};
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue, ReturnValuesOnConditionCheckFailure};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::env;

use nexus_common::dynamodb::{get_s, s, string_map_to_attr};
use nexus_common::error::ControlPlaneError;
use nexus_common::models::DomainStatus;
use nexus_common::principal::{IamOnlyAuthorizer, ResourceAuthorizer, TrustedPrincipal};

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

const OAUTH_MUTATIONS_DISABLED: &str =
    "unauthorized: interactive OAuth mutations are not yet enabled";

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    eb: &aws_sdk_eventbridge::Client,
    message: &UpdateDomainMessage,
    principal: &TrustedPrincipal,
) -> Result<()> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let cp_bus = env::var("NEXUS_EVENT_BUS").context("NEXUS_EVENT_BUS not set")?;
    let domain_name = &message.domain_name;

    let decision = IamOnlyAuthorizer
        .authorize(principal, "domains:Update", domain_name)
        .await;
    if !decision.is_allowed() {
        tracing::warn!(
            action = "domains:Update",
            domain = domain_name,
            "consumer authorization denied"
        );
        return emit_failed(eb, &cp_bus, domain_name, OAUTH_MUTATIONS_DISABLED).await;
    }

    if message.body.description.is_none()
        && message.body.tags.is_none()
        && message.body.owner_email.is_none()
        && message.body.secondary_owner_emails.is_none()
    {
        return emit_failed(eb, &cp_bus, domain_name, "no fields provided to update").await;
    }
    if message
        .body
        .secondary_owner_emails
        .as_ref()
        .is_some_and(std::vec::Vec::is_empty)
    {
        return emit_failed(
            eb,
            &cp_bus,
            domain_name,
            "secondaryOwnerEmails must contain at least one email address",
        )
        .await;
    }

    let now = chrono::Utc::now().to_rfc3339();
    let (update_expression, expr_values, changed_fields) =
        build_update_expression(&message.body, &now);

    let mut update = ddb
        .update_item()
        .table_name(&table)
        .key("domainName", s(domain_name))
        .update_expression(&update_expression)
        .condition_expression("attribute_exists(domainName) AND #status = :active")
        .expression_attribute_names("#status", "status")
        .expression_attribute_values(":active", s(DomainStatus::Active.as_str()))
        .return_values(ReturnValue::AllNew)
        .return_values_on_condition_check_failure(ReturnValuesOnConditionCheckFailure::AllOld);

    for (k, v) in expr_values {
        update = update.expression_attribute_values(k, v);
    }

    let output = match update.send().await {
        Ok(output) => output,
        Err(e) => {
            let service_err = e.into_service_error();
            if let UpdateItemError::ConditionalCheckFailedException(ccf) = &service_err {
                let reason = if ccf.item().is_none() {
                    "domain not found"
                } else {
                    "domain is not ACTIVE and cannot be modified"
                };
                return emit_failed(eb, &cp_bus, domain_name, reason).await;
            }
            return Err(ControlPlaneError::DynamoDb(service_err.to_string()).into());
        }
    };

    emit_success(
        eb,
        &cp_bus,
        domain_name,
        &now,
        &output.attributes.unwrap_or_default(),
        &changed_fields,
    )
    .await
}

fn build_update_expression(
    body: &UpdateDomainBody,
    now: &str,
) -> (String, Vec<(String, AttributeValue)>, Vec<&'static str>) {
    let mut parts = vec!["updatedAt = :now".to_string()];
    let mut values = vec![(":now".to_string(), s(now))];
    let mut changed: Vec<&'static str> = vec![];

    if let Some(desc) = body.description.as_deref() {
        parts.push("description = :desc".into());
        values.push((":desc".to_string(), s(desc)));
        changed.push("description");
    }
    if let Some(tags) = &body.tags {
        parts.push("tags = :tags".into());
        values.push((":tags".to_string(), string_map_to_attr(tags)));
        changed.push("tags");
    }
    if let Some(email) = body.owner_email.as_deref() {
        parts.push("ownerEmail = :ownerEmail".into());
        values.push((":ownerEmail".to_string(), s(email)));
        changed.push("ownerEmail");
    }
    if let Some(emails) = &body.secondary_owner_emails {
        parts.push("secondaryOwnerEmails = :secondaryOwnerEmails".into());
        values.push((
            ":secondaryOwnerEmails".to_string(),
            AttributeValue::Ss(emails.clone()),
        ));
        changed.push("secondaryOwnerEmails");
    }

    (format!("SET {}", parts.join(", ")), values, changed)
}

async fn emit_success(
    eb: &aws_sdk_eventbridge::Client,
    cp_bus: &str,
    domain_name: &str,
    now: &str,
    attrs: &std::collections::HashMap<String, AttributeValue>,
    changed_fields: &[&str],
) -> Result<()> {
    let result = eb.put_events().entries(
        aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
            .event_bus_name(cp_bus).source("nexus.domain-management").detail_type("UpdateDomainSucceeded")
            .detail(json!({"domainName": domain_name, "updatedAt": now, "status": get_s(attrs, "status"), "changedFields": changed_fields}).to_string())
            .build(),
    ).send().await.map_err(|e| ControlPlaneError::EventBridge(e.to_string()))?;

    if result.failed_entry_count > 0 {
        return Err(ControlPlaneError::EventBridge(format!(
            "{} UpdateDomainSucceeded event entries failed to publish",
            result.failed_entry_count
        ))
        .into());
    }
    Ok(())
}

async fn emit_failed(
    eb: &aws_sdk_eventbridge::Client,
    cp_bus: &str,
    domain_name: &str,
    reason: &str,
) -> Result<()> {
    let result = eb
        .put_events()
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
        .map_err(|e| ControlPlaneError::EventBridge(e.to_string()))?;

    if result.failed_entry_count > 0 {
        return Err(ControlPlaneError::EventBridge(format!(
            "{} UpdateDomainFailed event entries failed to publish",
            result.failed_entry_count
        ))
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
    use aws_sdk_eventbridge::operation::put_events::PutEventsOutput;
    use aws_smithy_mocks::{mock, mock_client};
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

    fn conditional_check_failed(
        item: Option<HashMap<String, aws_sdk_dynamodb::types::AttributeValue>>,
    ) -> UpdateItemError {
        UpdateItemError::ConditionalCheckFailedException(
            aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                .set_item(item)
                .build(),
        )
    }

    fn failed_event_rule_with_reason(reason: &'static str) -> aws_smithy_mocks::Rule {
        mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(move |req| {
                req.entries().iter().any(|entry| {
                    entry.detail_type() == Some("UpdateDomainFailed")
                        && entry.detail().is_some_and(|d| d.contains(reason))
                })
            })
            .then_output(|| PutEventsOutput::builder().build())
    }

    fn set_env() {
        env::set_var("DOMAINS_TABLE", "domains");
        env::set_var("NEXUS_EVENT_BUS", "nexus-bus");
    }

    #[tokio::test]
    async fn updates_domain_and_emits_success_event() {
        set_env();

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
        let ddb = mock_client!(aws_sdk_dynamodb, [&update_rule]);

        let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| PutEventsOutput::builder().build());
        let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

        let message = update_message("billing", Some("Updated billing domain"));
        let result = handle(
            &ddb,
            &eb,
            &message,
            &TrustedPrincipal::iam("111122223333", None),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(update_rule.num_calls(), 1);
        assert_eq!(eb_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn update_is_conditional_on_active_status() {
        set_env();

        let update_rule = mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                req.condition_expression()
                    .is_some_and(|expr| expr.contains("#status = :active"))
                    && req
                        .expression_attribute_values()
                        .and_then(|values| values.get(":active"))
                        .and_then(|v| v.as_s().ok())
                        .is_some_and(|v| v == "ACTIVE")
            })
            .then_output(|| UpdateItemOutput::builder().build());
        let ddb = mock_client!(aws_sdk_dynamodb, [&update_rule]);

        let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| PutEventsOutput::builder().build());
        let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

        let message = update_message("billing", Some("Updated billing domain"));
        let result = handle(
            &ddb,
            &eb,
            &message,
            &TrustedPrincipal::iam("111122223333", None),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(update_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn emits_failed_event_when_domain_not_found() {
        set_env();

        let update_rule = mock!(aws_sdk_dynamodb::Client::update_item)
            .then_error(|| conditional_check_failed(None));
        let ddb = mock_client!(aws_sdk_dynamodb, [&update_rule]);

        let eb_rule = failed_event_rule_with_reason("domain not found");
        let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

        let message = update_message("nonexistent", Some("desc"));
        let result = handle(
            &ddb,
            &eb,
            &message,
            &TrustedPrincipal::iam("111122223333", None),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(update_rule.num_calls(), 1);
        assert_eq!(eb_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn rejects_update_on_non_active_domain() {
        set_env();

        let mut item = active_domain_item();
        item.insert("status".into(), s("PENDING_APPROVAL"));

        let update_rule = mock!(aws_sdk_dynamodb::Client::update_item)
            .then_error(move || conditional_check_failed(Some(item.clone())));
        let ddb = mock_client!(aws_sdk_dynamodb, [&update_rule]);

        let eb_rule = failed_event_rule_with_reason("not ACTIVE");
        let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

        let message = update_message("billing", Some("Should not work"));
        let result = handle(
            &ddb,
            &eb,
            &message,
            &TrustedPrincipal::iam("111122223333", None),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(update_rule.num_calls(), 1);
        assert_eq!(eb_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn rejects_empty_secondary_owner_emails_without_touching_dynamodb() {
        set_env();

        let ddb = mock_client!(aws_sdk_dynamodb, &[]);

        let eb_rule = failed_event_rule_with_reason("secondaryOwnerEmails");
        let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

        let message = UpdateDomainMessage {
            domain_name: "billing".to_string(),
            body: UpdateDomainBody {
                description: None,
                tags: None,
                owner_email: None,
                secondary_owner_emails: Some(vec![]),
            },
        };
        let result = handle(
            &ddb,
            &eb,
            &message,
            &TrustedPrincipal::iam("111122223333", None),
        )
        .await;

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
