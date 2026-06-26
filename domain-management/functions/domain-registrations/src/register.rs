use anyhow::{anyhow, Context, Result};
use aws_sdk_dynamodb::operation::put_item::PutItemError;
use aws_sdk_dynamodb::types::AttributeValue;
use serde_json::json;
use std::env;

use nexus_common::dynamodb::s;
use nexus_common::error::ControlPlaneError;
use nexus_common::models::{DomainStatus, RegisterDomainRequest};

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    eb: &aws_sdk_eventbridge::Client,
    sfn: &aws_sdk_sfn::Client,
    request: &RegisterDomainRequest,
) -> Result<()> {
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let state_machine_arn =
        env::var("APPROVAL_STATE_MACHINE_ARN").context("APPROVAL_STATE_MACHINE_ARN not set")?;
    let cp_bus = env::var("NEXUS_EVENT_BUS").context("NEXUS_EVENT_BUS not set")?;
    let environments_raw = env::var("ENVIRONMENTS").context("ENVIRONMENTS not set")?;
    let environments = parse_environments(&environments_raw)
        .map_err(|e| anyhow!("invalid ENVIRONMENTS value: {e}"))?;

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

    let request_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let status = DomainStatus::PendingApproval.to_string();

    let mut put_domain = ddb
        .put_item()
        .table_name(&domains_table)
        .item("domainName", s(&request.domain_name))
        .item("ownerAccountId", s(&request.owner_account_id))
        .item("status", s(&status))
        .item("ownerEmail", s(&request.owner_email))
        .item("createdAt", s(&now))
        .item("updatedAt", s(&now))
        .item("createdBy", s(&request.owner_account_id))
        .condition_expression("attribute_not_exists(domainName)");

    if let Some(desc) = &request.description {
        put_domain = put_domain.item("description", s(desc));
    }

    let environments_attr = AttributeValue::L(environments.iter().map(|e| s(e)).collect());
    put_domain = put_domain.item("environments", environments_attr);

    match put_domain.send().await {
        Ok(_) => {}
        Err(e) => {
            let service_err = e.into_service_error();
            if let PutItemError::ConditionalCheckFailedException(_) = service_err {
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
            } else {
                return Err(ControlPlaneError::DynamoDb(service_err.to_string()).into());
            }
        }
    }

    let sfn_input = json!({
        "requestId": request_id,
        "domainName": request.domain_name,
        "ownerAccountId": request.owner_account_id,
        "ownerEmail": request.owner_email,
        "description": request.description,
        "environments": environments,
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
                        "domainName": request.domain_name,
                        "ownerAccountId": request.owner_account_id,
                        "ownerEmail": request.owner_email,
                        "status": status,
                        "createdAt": now,
                        "environments": environments,
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

fn parse_environments(raw: &str) -> Result<Vec<String>, String> {
    let envs: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if envs.is_empty() {
        return Err("must contain at least one entry".to_string());
    }

    for env_name in &envs {
        validate_environment_name(env_name)
            .map_err(|e| format!("invalid entry '{env_name}': {e}"))?;
    }

    Ok(envs)
}

fn validate_environment_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 32 {
        return Err("length must be 1-32 characters".to_string());
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err("must start with a lowercase letter or digit".to_string());
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err("must contain only lowercase letters, digits, and hyphens".to_string());
        }
    }
    Ok(())
}

const RESERVED_DOMAIN_NAMES: &[&str] = &["nexus"];

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
    if RESERVED_DOMAIN_NAMES.contains(&name) {
        return Err(nexus_common::api::validation_error(format!(
            "Domain name '{name}' is reserved for platform use"
        )));
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

    #[test]
    fn rejects_reserved_nexus_domain_name() {
        let err = validate_domain_name("nexus").unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("reserved"),
            "error should mention reserved: {err}"
        );
    }

    #[test]
    fn validate_environment_name_accepts_simple_names() {
        assert!(validate_environment_name("dev").is_ok());
        assert!(validate_environment_name("staging").is_ok());
        assert!(validate_environment_name("prod").is_ok());
        assert!(validate_environment_name("qa-1").is_ok());
        assert!(validate_environment_name("a").is_ok());
    }

    #[test]
    fn validate_environment_name_rejects_invalid_chars() {
        assert!(validate_environment_name("DEV").is_err());
        assert!(validate_environment_name("dev_1").is_err());
        assert!(validate_environment_name("dev.prod").is_err());
        assert!(validate_environment_name("").is_err());
    }

    #[test]
    fn validate_environment_name_rejects_leading_hyphen() {
        assert!(validate_environment_name("-dev").is_err());
    }

    #[test]
    fn validate_environment_name_rejects_too_long() {
        assert!(validate_environment_name(&"a".repeat(33)).is_err());
        assert!(validate_environment_name(&"a".repeat(32)).is_ok());
    }

    #[test]
    fn parse_environments_splits_and_trims() {
        let envs = parse_environments(" dev , staging,prod ").expect("should parse");
        assert_eq!(envs, vec!["dev", "staging", "prod"]);
    }

    #[test]
    fn parse_environments_requires_at_least_one() {
        assert!(parse_environments("").is_err());
        assert!(parse_environments(",,,").is_err());
    }

    #[test]
    fn parse_environments_rejects_invalid_entry() {
        assert!(parse_environments("dev,PROD").is_err());
        assert!(parse_environments("dev,-bad").is_err());
    }

    #[test]
    fn parse_environments_accepts_single_env() {
        let envs = parse_environments("dev").expect("should parse");
        assert_eq!(envs, vec!["dev"]);
    }

    mod handle_tests {
        use super::*;
        use aws_sdk_dynamodb::operation::put_item::PutItemOutput;
        use aws_sdk_eventbridge::operation::put_events::PutEventsOutput;
        use aws_sdk_sfn::operation::start_execution::StartExecutionOutput;
        use aws_smithy_mocks::{mock, mock_client};
        use serde_json::Value;

        fn valid_request() -> RegisterDomainRequest {
            RegisterDomainRequest {
                domain_name: "billing".to_string(),
                description: None,
                owner_account_id: "111122223333".to_string(),
                owner_email: "owner@example.com".to_string(),
                tags: None,
            }
        }

        fn set_env() {
            env::set_var("DOMAINS_TABLE", "domains");
            env::set_var(
                "APPROVAL_STATE_MACHINE_ARN",
                "arn:aws:states:us-east-1:123:stateMachine:approval",
            );
            env::set_var("NEXUS_EVENT_BUS", "nexus-bus");
            env::set_var("ENVIRONMENTS", "dev,staging,prod");
        }

        #[tokio::test]
        async fn creates_domain_and_starts_approval() {
            set_env();

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item)
                .then_output(|| PutItemOutput::builder().build());

            let ddb = mock_client!(aws_sdk_dynamodb, [&put_rule]);

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
            assert_eq!(put_rule.num_calls(), 1);
            assert_eq!(sfn_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn rejects_duplicate_domain_with_failed_event() {
            set_env();

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item).then_error(|| {
                aws_sdk_dynamodb::operation::put_item::PutItemError::ConditionalCheckFailedException(
                    aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                        .build(),
                )
            });
            let ddb = mock_client!(aws_sdk_dynamodb, [&put_rule]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .then_output(|| PutEventsOutput::builder().build());
            let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

            let sfn = mock_client!(aws_sdk_sfn, &[]);

            let result = handle(&ddb, &eb, &sfn, &valid_request()).await;

            assert!(result.is_ok());
            assert_eq!(put_rule.num_calls(), 1);
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

            let request = RegisterDomainRequest {
                domain_name: "INVALID".to_string(),
                description: None,
                owner_account_id: "111122223333".to_string(),
                owner_email: "owner@example.com".to_string(),
                tags: None,
            };

            let result = handle(&ddb, &eb, &sfn, &request).await;

            assert!(result.is_ok());
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn creates_domain_with_optional_description() {
            set_env();

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item)
                .match_requests(|req| {
                    req.item()
                        .map(|items| items.contains_key("description"))
                        .unwrap_or(false)
                })
                .then_output(|| PutItemOutput::builder().build());

            let ddb = mock_client!(aws_sdk_dynamodb, [&put_rule]);

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

            let request = RegisterDomainRequest {
                domain_name: "billing".to_string(),
                description: Some("Billing domain events".to_string()),
                owner_account_id: "111122223333".to_string(),
                owner_email: "owner@example.com".to_string(),
                tags: None,
            };

            let result = handle(&ddb, &eb, &sfn, &request).await;

            assert!(result.is_ok());
            assert_eq!(put_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn stamps_environments_list_on_put_item() {
            set_env();

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item)
                .match_requests(|req| {
                    let Some(items) = req.item() else {
                        return false;
                    };
                    let Some(AttributeValue::L(values)) = items.get("environments") else {
                        return false;
                    };
                    let parsed: Vec<String> = values
                        .iter()
                        .filter_map(|v| v.as_s().ok().cloned())
                        .collect();
                    parsed == vec!["dev", "staging", "prod"]
                })
                .then_output(|| PutItemOutput::builder().build());

            let ddb = mock_client!(aws_sdk_dynamodb, [&put_rule]);

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

            let result = handle(&ddb, &eb, &sfn, &valid_request()).await;

            assert!(result.is_ok());
            assert_eq!(put_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn includes_environments_in_succeeded_event() {
            set_env();

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item)
                .then_output(|| PutItemOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&put_rule]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|entry| {
                        if entry.detail_type() != Some("RegisterDomainSucceeded") {
                            return false;
                        }
                        let Some(detail) = entry.detail() else {
                            return false;
                        };
                        let Ok(value) = serde_json::from_str::<Value>(detail) else {
                            return false;
                        };
                        value.get("environments") == Some(&json!(["dev", "staging", "prod"]))
                    })
                })
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

            let result = handle(&ddb, &eb, &sfn, &valid_request()).await;

            assert!(result.is_ok());
            assert_eq!(eb_rule.num_calls(), 1);
        }
    }
}
