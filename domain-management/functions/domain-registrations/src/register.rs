use anyhow::{Context, Result};
use aws_sdk_dynamodb::operation::put_item::PutItemError;
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValuesOnConditionCheckFailure};
use aws_sdk_sfn::operation::start_execution::StartExecutionError;
use serde_json::json;
use std::env;

use konductor_common::dynamodb::{get_s, s, string_map_to_attr};
use konductor_common::error::ControlPlaneError;
use konductor_common::models::{DomainStatus, RegisterDomainRequest};
use konductor_common::principal::TrustedPrincipal;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    eb: &aws_sdk_eventbridge::Client,
    sfn: &aws_sdk_sfn::Client,
    request: &RegisterDomainRequest,
    principal: &TrustedPrincipal,
    request_id: &str,
) -> Result<()> {
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let state_machine_arn =
        env::var("APPROVAL_STATE_MACHINE_ARN").context("APPROVAL_STATE_MACHINE_ARN not set")?;
    let cp_bus = env::var("KONDUKTOR_EVENT_BUS").context("KONDUKTOR_EVENT_BUS not set")?;
    let environments_raw = env::var("ENVIRONMENTS").context("ENVIRONMENTS not set")?;
    let environments = parse_environments(&environments_raw).map_err(|e| {
        ControlPlaneError::ValidationException {
            message: format!("invalid ENVIRONMENTS value: {e}"),
            reason: konductor_common::error::ValidationExceptionReason::FieldValidationFailed,
            field_list: None,
        }
    })?;

    if let Err(reason) = authorize_registration(principal, request) {
        tracing::warn!(action = "domains:Register", domain = %request.domain_name, "registration authorization denied");
        emit_failed(eb, &cp_bus, request, &reason).await?;
        return Ok(());
    }

    if let Err(e) = validate_domain_name(&request.domain_name) {
        emit_failed(eb, &cp_bus, request, &e.to_string()).await?;
        return Ok(());
    }

    let put_domain = build_put_item(ddb, &domains_table, request, request_id, &environments);
    let mut created_at = chrono::Utc::now().to_rfc3339();

    match put_domain.send().await {
        Ok(_) => {}
        Err(e) => {
            let service_err = e.into_service_error();
            if let PutItemError::ConditionalCheckFailedException(ccf) = &service_err {
                let existing_request_id = ccf.item().and_then(|item| get_s(item, "requestId"));
                if existing_request_id.as_deref() == Some(request_id) {
                    if let Some(original) = ccf.item().and_then(|item| get_s(item, "createdAt")) {
                        created_at = original;
                    }
                } else {
                    let reason = format!("domain '{}' already exists", request.domain_name);
                    emit_failed(eb, &cp_bus, request, &reason).await?;
                    return Ok(());
                }
            } else {
                return Err(ControlPlaneError::DynamoDb(service_err.to_string()).into());
            }
        }
    }

    start_approval_workflow(sfn, &state_machine_arn, request, request_id, &environments).await?;

    emit_succeeded(eb, &cp_bus, request, request_id, &environments, &created_at).await
}

/// OAuth callers register on behalf of exactly one of their team groups,
/// which becomes the domain's owner at approval. Global platform groups are
/// never carried as team groups, so membership alone enforces "platform
/// teams cannot own domains". IAM automation keeps its trust path and may
/// omit the team; approval then creates no owner binding.
fn authorize_registration(
    principal: &TrustedPrincipal,
    request: &RegisterDomainRequest,
) -> Result<(), String> {
    match principal {
        TrustedPrincipal::Iam(_) => Ok(()),
        TrustedPrincipal::OAuth(p) => {
            let Some(team) = request.owning_team.as_deref() else {
                return Err("unauthorized: OAuth registration requires owningTeam naming one of the caller's team groups".to_string());
            };
            if p.team_groups.iter().any(|g| g == team) {
                Ok(())
            } else {
                Err(format!(
                    "unauthorized: owningTeam '{team}' is not one of the caller's team groups"
                ))
            }
        }
    }
}

fn build_put_item(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    request: &RegisterDomainRequest,
    request_id: &str,
    environments: &[String],
) -> aws_sdk_dynamodb::operation::put_item::builders::PutItemFluentBuilder {
    let now = chrono::Utc::now().to_rfc3339();
    let status = DomainStatus::PendingApproval.to_string();
    let rejected_status = DomainStatus::Rejected.to_string();

    let mut put = ddb
        .put_item()
        .table_name(table)
        .item("domainName", s(&request.domain_name))
        .item("requestId", s(request_id))
        .item("ownerAccountId", s(&request.owner_account_id))
        .item("status", s(&status))
        .item("ownerEmail", s(&request.owner_email))
        .item(
            "secondaryOwnerEmails",
            AttributeValue::Ss(request.secondary_owner_emails.clone()),
        )
        .item("createdAt", s(&now))
        .item("updatedAt", s(&now))
        .item("createdBy", s(&request.owner_account_id))
        .condition_expression("attribute_not_exists(domainName) OR #status = :rejected")
        .expression_attribute_names("#status", "status")
        .expression_attribute_values(":rejected", s(&rejected_status))
        .return_values_on_condition_check_failure(ReturnValuesOnConditionCheckFailure::AllOld);

    if let Some(desc) = &request.description {
        put = put.item("description", s(desc));
    }
    if let Some(team) = &request.owning_team {
        put = put.item("owningTeam", s(team));
    }
    if let Some(tags) = &request.tags {
        put = put.item("tags", string_map_to_attr(tags));
    }
    put = put.item(
        "environments",
        AttributeValue::L(environments.iter().map(|e| s(e)).collect()),
    );
    put
}

async fn start_approval_workflow(
    sfn: &aws_sdk_sfn::Client,
    state_machine_arn: &str,
    request: &RegisterDomainRequest,
    request_id: &str,
    environments: &[String],
) -> Result<()> {
    let sfn_input = json!({
        "requestId": request_id,
        "domainName": request.domain_name,
        "ownerAccountId": request.owner_account_id,
        "ownerEmail": request.owner_email,
        "secondaryOwnerEmails": request.secondary_owner_emails,
        "description": request.description,
        "environments": environments,
    });

    if let Err(e) = sfn
        .start_execution()
        .state_machine_arn(state_machine_arn)
        .name(format!("approval-{request_id}"))
        .input(sfn_input.to_string())
        .send()
        .await
    {
        let service_err = e.into_service_error();
        if !matches!(service_err, StartExecutionError::ExecutionAlreadyExists(_)) {
            return Err(ControlPlaneError::StepFunctions(service_err.to_string()).into());
        }
    }
    Ok(())
}

async fn emit_succeeded(
    eb: &aws_sdk_eventbridge::Client,
    cp_bus: &str,
    request: &RegisterDomainRequest,
    request_id: &str,
    environments: &[String],
    created_at: &str,
) -> Result<()> {
    let status = DomainStatus::PendingApproval.to_string();
    let result = eb.put_events().entries(
        aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
            .event_bus_name(cp_bus).source("konductor.domain-management").detail_type("RegisterDomainSucceeded")
            .detail(json!({"requestId": request_id, "domainName": request.domain_name, "ownerAccountId": request.owner_account_id, "ownerEmail": request.owner_email, "secondaryOwnerEmails": request.secondary_owner_emails, "status": status, "createdAt": created_at, "environments": environments}).to_string())
            .build(),
    ).send().await.map_err(|e| ControlPlaneError::EventBridge(e.to_string()))?;

    if result.failed_entry_count > 0 {
        return Err(ControlPlaneError::EventBridge(format!(
            "{} RegisterDomainSucceeded event entries failed to publish",
            result.failed_entry_count
        ))
        .into());
    }
    Ok(())
}

async fn emit_failed(
    eb: &aws_sdk_eventbridge::Client,
    cp_bus: &str,
    request: &RegisterDomainRequest,
    reason: &str,
) -> Result<()> {
    let result = eb
        .put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(cp_bus)
                .source("konductor.domain-management")
                .detail_type("RegisterDomainFailed")
                .detail(
                    json!({
                        "domainName": request.domain_name,
                        "ownerAccountId": request.owner_account_id,
                        "ownerEmail": request.owner_email,
                        "secondaryOwnerEmails": request.secondary_owner_emails,
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
            "{} RegisterDomainFailed event entries failed to publish",
            result.failed_entry_count
        ))
        .into());
    }

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
    let Some(first) = chars.next() else {
        return Err("length must be 1-32 characters".to_string());
    };
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

const RESERVED_DOMAIN_NAMES: &[&str] = &["konductor"];

pub(crate) fn validate_domain_name(name: &str) -> Result<(), ControlPlaneError> {
    if name.len() < 2 || name.len() > 64 {
        return Err(konductor_common::api::validation_error(
            "Domain name must be between 2 and 64 characters",
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(konductor_common::api::validation_error(
            "Domain name must contain only lowercase letters, digits, and hyphens",
        ));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(konductor_common::api::validation_error(
            "Domain name must not start or end with a hyphen",
        ));
    }
    if RESERVED_DOMAIN_NAMES.contains(&name) {
        return Err(konductor_common::api::validation_error(format!(
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
    fn rejects_reserved_konductor_domain_name() {
        let err = validate_domain_name("konductor").unwrap_err();
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

        const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

        fn valid_request() -> RegisterDomainRequest {
            RegisterDomainRequest {
                domain_name: "billing".to_string(),
                description: None,
                owner_account_id: "111122223333".to_string(),
                owner_email: "owner@example.com".to_string(),
                secondary_owner_emails: vec!["backup@example.com".to_string()],
                tags: None,
                owning_team: None,
            }
        }

        fn oauth(groups: &[&str]) -> TrustedPrincipal {
            TrustedPrincipal::oauth(
                "a1b2c3d4-1111-2222-3333-444455556666",
                ISSUER,
                "client123",
                vec![],
                groups.iter().map(|g| g.to_string()).collect(),
            )
        }

        fn set_env() {
            env::set_var("DOMAINS_TABLE", "domains");
            env::set_var(
                "APPROVAL_STATE_MACHINE_ARN",
                "arn:aws:states:us-east-1:123:stateMachine:approval",
            );
            env::set_var("KONDUKTOR_EVENT_BUS", "konductor-bus");
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

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &valid_request(),
                &TrustedPrincipal::iam("111122223333", None),
                "msg-1",
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(put_rule.num_calls(), 1);
            assert_eq!(sfn_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn oauth_registration_with_owning_team_is_accepted() {
            set_env();

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item)
                .match_requests(|req| {
                    req.item()
                        .and_then(|i| i.get("owningTeam"))
                        .and_then(|v| v.as_s().ok())
                        .is_some_and(|v| v == "team-payments")
                })
                .then_output(|| PutItemOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&put_rule]);
            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries()
                        .iter()
                        .any(|e| e.detail_type() == Some("RegisterDomainSucceeded"))
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

            let mut request = valid_request();
            request.owning_team = Some("team-payments".to_string());
            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &request,
                &oauth(&["team-payments", "team-platform"]),
                "msg-1",
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(put_rule.num_calls(), 1);
            assert_eq!(sfn_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn oauth_registration_without_owning_team_fails() {
            set_env();

            let ddb = mock_client!(aws_sdk_dynamodb, &[]);
            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|e| {
                        e.detail_type() == Some("RegisterDomainFailed")
                            && e.detail().is_some_and(|d| d.contains("owningTeam"))
                    })
                })
                .then_output(|| PutEventsOutput::builder().build());
            let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);
            let sfn = mock_client!(aws_sdk_sfn, &[]);

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &valid_request(),
                &oauth(&["team-payments"]),
                "msg-1",
            )
            .await;

            assert!(result.is_ok(), "denial is reported via the failed event");
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn oauth_registration_with_foreign_or_platform_team_fails() {
            set_env();

            for owning_team in ["team-other", "platform-admin"] {
                let ddb = mock_client!(aws_sdk_dynamodb, &[]);
                let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                    .match_requests(|req| {
                        req.entries()
                            .iter()
                            .any(|e| e.detail_type() == Some("RegisterDomainFailed"))
                    })
                    .then_output(|| PutEventsOutput::builder().build());
                let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);
                let sfn = mock_client!(aws_sdk_sfn, &[]);

                let mut request = valid_request();
                request.owning_team = Some(owning_team.to_string());
                let result = handle(
                    &ddb,
                    &eb,
                    &sfn,
                    &request,
                    &oauth(&["team-payments"]),
                    "msg-1",
                )
                .await;

                assert!(result.is_ok(), "{owning_team}: denial via failed event");
                assert_eq!(eb_rule.num_calls(), 1, "{owning_team}");
            }
        }

        #[tokio::test]
        async fn rejects_duplicate_domain_with_failed_event() {
            set_env();

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item).then_error(|| {
                aws_sdk_dynamodb::operation::put_item::PutItemError::ConditionalCheckFailedException(
                    aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                        .item("domainName", s("billing"))
                        .item("requestId", s("someone-elses-request"))
                        .build(),
                )
            });
            let ddb = mock_client!(aws_sdk_dynamodb, [&put_rule]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|entry| {
                        entry.detail_type() == Some("RegisterDomainFailed")
                            && entry.detail().is_some_and(|d| {
                                serde_json::from_str::<Value>(d).is_ok_and(|v| {
                                    v.get("secondaryOwnerEmails")
                                        == Some(&json!(["backup@example.com"]))
                                })
                            })
                    })
                })
                .then_output(|| PutEventsOutput::builder().build());
            let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

            let sfn = mock_client!(aws_sdk_sfn, &[]);

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &valid_request(),
                &TrustedPrincipal::iam("111122223333", None),
                "msg-1",
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(put_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn retry_of_own_registration_resumes_after_conditional_failure() {
            set_env();

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item).then_error(|| {
                aws_sdk_dynamodb::operation::put_item::PutItemError::ConditionalCheckFailedException(
                    aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                        .item("domainName", s("billing"))
                        .item("requestId", s("msg-1"))
                        .item("createdAt", s("2024-01-01T00:00:00+00:00"))
                        .build(),
                )
            });
            let ddb = mock_client!(aws_sdk_dynamodb, [&put_rule]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|entry| {
                        entry.detail_type() == Some("RegisterDomainSucceeded")
                            && entry
                                .detail()
                                .is_some_and(|d| d.contains("2024-01-01T00:00:00+00:00"))
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

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &valid_request(),
                &TrustedPrincipal::iam("111122223333", None),
                "msg-1",
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(put_rule.num_calls(), 1);
            assert_eq!(sfn_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn tolerates_execution_already_exists_on_retry() {
            set_env();

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item)
                .then_output(|| PutItemOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&put_rule]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .then_output(|| PutEventsOutput::builder().build());
            let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

            let sfn_rule = mock!(aws_sdk_sfn::Client::start_execution).then_error(|| {
                aws_sdk_sfn::operation::start_execution::StartExecutionError::ExecutionAlreadyExists(
                    aws_sdk_sfn::types::error::ExecutionAlreadyExists::builder().build(),
                )
            });
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &valid_request(),
                &TrustedPrincipal::iam("111122223333", None),
                "msg-1",
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(sfn_rule.num_calls(), 1);
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
                secondary_owner_emails: vec!["backup@example.com".to_string()],
                tags: None,
                owning_team: None,
            };

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &request,
                &TrustedPrincipal::iam("111122223333", None),
                "msg-1",
            )
            .await;

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
                secondary_owner_emails: vec!["backup@example.com".to_string()],
                tags: None,
                owning_team: None,
            };

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &request,
                &TrustedPrincipal::iam("111122223333", None),
                "msg-1",
            )
            .await;

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

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &valid_request(),
                &TrustedPrincipal::iam("111122223333", None),
                "msg-1",
            )
            .await;

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

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &valid_request(),
                &TrustedPrincipal::iam("111122223333", None),
                "msg-1",
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn put_item_uses_widened_condition_admitting_rejected() {
            set_env();

            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item)
                .match_requests(|req| {
                    req.condition_expression()
                        == Some("attribute_not_exists(domainName) OR #status = :rejected")
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

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &valid_request(),
                &TrustedPrincipal::iam("111122223333", None),
                "msg-1",
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(put_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn reregistration_over_rejected_succeeds_with_fresh_record() {
            set_env();

            // A REJECTED record passes the widened condition, so PutItem
            // succeeds and replaces the record wholesale. The handler then
            // starts a fresh approval workflow and emits RegisterDomainSucceeded.
            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item)
                .then_output(|| PutItemOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&put_rule]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries()
                        .iter()
                        .any(|entry| entry.detail_type() == Some("RegisterDomainSucceeded"))
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

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &valid_request(),
                &TrustedPrincipal::iam("111122223333", None),
                "msg-2",
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(put_rule.num_calls(), 1);
            assert_eq!(sfn_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn duplicate_over_pending_approval_still_emits_failed_event() {
            set_env();

            // A non-REJECTED record (e.g. PENDING_APPROVAL) still blocks
            // registration; the conditional failure branch fires and emits
            // RegisterDomainFailed with "domain already exists".
            let put_rule = mock!(aws_sdk_dynamodb::Client::put_item).then_error(|| {
                aws_sdk_dynamodb::operation::put_item::PutItemError::ConditionalCheckFailedException(
                    aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                        .item("domainName", s("billing"))
                        .item("requestId", s("someone-elses-request"))
                        .item("status", s("PENDING_APPROVAL"))
                        .build(),
                )
            });
            let ddb = mock_client!(aws_sdk_dynamodb, [&put_rule]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries()
                        .iter()
                        .any(|entry| entry.detail_type() == Some("RegisterDomainFailed"))
                })
                .then_output(|| PutEventsOutput::builder().build());
            let eb = mock_client!(aws_sdk_eventbridge, [&eb_rule]);

            let sfn = mock_client!(aws_sdk_sfn, &[]);

            let result = handle(
                &ddb,
                &eb,
                &sfn,
                &valid_request(),
                &TrustedPrincipal::iam("111122223333", None),
                "msg-2",
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(put_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
            // sfn has no mock rules — any call would error, so Ok(()) proves
            // the workflow was not started for a duplicate registration.
        }
    }
}
