use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::env;

use nexus_common::dynamodb::s;
use nexus_common::error::ControlPlaneError;
use nexus_common::models::{DomainStatus, RegisterDomainRequest};

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    eb: &aws_sdk_eventbridge::Client,
    sfn: &aws_sdk_sfn::Client,
    body: &Value,
) -> Result<()> {
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let state_machine_arn =
        env::var("APPROVAL_STATE_MACHINE_ARN").context("APPROVAL_STATE_MACHINE_ARN not set")?;
    let cp_bus = env::var("NEXUS_EVENT_BUS").context("NEXUS_EVENT_BUS not set")?;

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
                .source("nexus.domain-management")
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
                .source("nexus.domain-management")
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
        return Err(nexus_common::api::validation_error(
            "Domain name must be between 2 and 64 characters",
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(nexus_common::api::validation_error(
            "Domain name must contain only lowercase letters, digits, and hyphens",
        ));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(nexus_common::api::validation_error(
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

    mod handle_tests {
        use super::*;
        use aws_sdk_dynamodb::operation::put_item::PutItemOutput;
        use aws_sdk_dynamodb::operation::query::QueryOutput;
        use aws_sdk_eventbridge::operation::put_events::PutEventsOutput;
        use aws_sdk_sfn::operation::start_execution::StartExecutionOutput;
        use aws_smithy_mocks::{mock, mock_client, RuleMode};

        fn valid_request() -> Value {
            json!({
                "domainName": "billing",
                "ownerAccountId": "111122223333",
                "ownerEmail": "owner@example.com"
            })
        }

        fn set_env() {
            env::set_var("DOMAINS_TABLE", "domains");
            env::set_var(
                "APPROVAL_STATE_MACHINE_ARN",
                "arn:aws:states:us-east-1:123:stateMachine:approval",
            );
            env::set_var("NEXUS_EVENT_BUS", "nexus-bus");
        }

        #[tokio::test]
        async fn creates_domain_and_starts_approval() {
            set_env();

            let query_rule = mock!(aws_sdk_dynamodb::Client::query)
                .match_requests(|req| req.index_name() == Some("by-name"))
                .then_output(|| QueryOutput::builder().count(0).build());

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item)
                .then_output(|| PutItemOutput::builder().build());

            let ddb = mock_client!(
                aws_sdk_dynamodb,
                RuleMode::Sequential,
                [&query_rule, &put_rule]
            );

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .then_output(|| PutEventsOutput::builder().build());
            let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

            let sfn_rule = mock!(aws_sdk_sfn::Client::start_execution).then_output(|| {
                StartExecutionOutput::builder()
                    .execution_arn("arn:aws:states:us-east-1:123:execution:approval:run-1")
                    .start_date(aws_sdk_sfn::primitives::DateTime::from_secs(0))
                    .build()
                    .unwrap()
            });
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);

            let result = handle(&ddb, &eb, &sfn, &valid_request()).await;

            assert!(result.is_ok());
            assert_eq!(query_rule.num_calls(), 1);
            assert_eq!(put_rule.num_calls(), 1);
            assert_eq!(sfn_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn rejects_duplicate_domain_with_failed_event() {
            set_env();

            let query_rule = mock!(aws_sdk_dynamodb::Client::query)
                .then_output(|| QueryOutput::builder().count(1).build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&query_rule]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .then_output(|| PutEventsOutput::builder().build());
            let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

            let sfn = mock_client!(aws_sdk_sfn, &[]);

            let result = handle(&ddb, &eb, &sfn, &valid_request()).await;

            assert!(result.is_ok());
            assert_eq!(query_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn validation_failure_emits_failed_event() {
            set_env();

            let ddb = mock_client!(aws_sdk_dynamodb, &[]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .then_output(|| PutEventsOutput::builder().build());
            let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

            let sfn = mock_client!(aws_sdk_sfn, &[]);

            let request = json!({
                "domainName": "INVALID",
                "ownerAccountId": "111122223333",
                "ownerEmail": "owner@example.com"
            });

            let result = handle(&ddb, &eb, &sfn, &request).await;

            assert!(result.is_ok());
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn creates_domain_with_optional_description() {
            set_env();

            let query_rule = mock!(aws_sdk_dynamodb::Client::query)
                .then_output(|| QueryOutput::builder().count(0).build());

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item)
                .match_requests(|req| {
                    req.item()
                        .map(|items| items.contains_key("description"))
                        .unwrap_or(false)
                })
                .then_output(|| PutItemOutput::builder().build());

            let ddb = mock_client!(
                aws_sdk_dynamodb,
                RuleMode::Sequential,
                [&query_rule, &put_rule]
            );

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .then_output(|| PutEventsOutput::builder().build());
            let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

            let sfn_rule = mock!(aws_sdk_sfn::Client::start_execution).then_output(|| {
                StartExecutionOutput::builder()
                    .execution_arn("arn:exec")
                    .start_date(aws_sdk_sfn::primitives::DateTime::from_secs(0))
                    .build()
                    .unwrap()
            });
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);

            let request = json!({
                "domainName": "billing",
                "ownerAccountId": "111122223333",
                "ownerEmail": "owner@example.com",
                "description": "Billing domain events"
            });

            let result = handle(&ddb, &eb, &sfn, &request).await;

            assert!(result.is_ok());
            assert_eq!(put_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn returns_error_on_invalid_request_body() {
            set_env();

            let ddb = mock_client!(aws_sdk_dynamodb, &[]);
            let eb = mock_client!(aws_sdk_eventbridge, &[]);
            let sfn = mock_client!(aws_sdk_sfn, &[]);

            let bad_body = json!({"unexpected": "fields"});

            let result = handle(&ddb, &eb, &sfn, &bad_body).await;

            assert!(result.is_err());
        }
    }
}
