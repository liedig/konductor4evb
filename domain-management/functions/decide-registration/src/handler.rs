use anyhow::Result;
use aws_sdk_sfn::operation::send_task_failure::SendTaskFailureError;
use aws_sdk_sfn::operation::send_task_success::SendTaskSuccessError;
use lambda_runtime::{Error, LambdaEvent};
use nexus_common::api;
use nexus_common::models::{ApiError, DecideRequest, Decision};
use nexus_common::principal_context::log_principal;
use serde_json::{json, Value};

pub async fn handle(sfn: &aws_sdk_sfn::Client, event: LambdaEvent<Value>) -> Result<Value, Error> {
    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "approval attempted without trusted principal");
            return Ok(access_denied(
                "Approval requires an authenticated OAuth principal",
            ));
        }
    };

    if principal.iam_account_id().is_some() {
        return Ok(access_denied(
            "Approval requires an interactive OAuth principal, not IAM",
        ));
    }

    if !principal.is_platform_admin() {
        return Ok(access_denied("Approval requires the platform-admin role"));
    }

    let decided_by = principal.principal_id().to_string();

    let request: DecideRequest = match api::parse_body(&event.payload) {
        Ok(r) => r,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    if request.task_token.is_empty() {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "taskToken is required",
        )));
    }

    if request.decision == Decision::Reject
        && request.justification.as_deref().unwrap_or("").is_empty()
    {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "justification is required when decision is REJECT",
        )));
    }

    let justification = request.justification.unwrap_or_default();

    Ok(
        match record_decision(
            sfn,
            &request.task_token,
            &request.decision,
            &decided_by,
            &justification,
        )
        .await
        {
            DecisionOutcome::Recorded => {
                api::success_response(200, &json!({"message": "Decision recorded"}))
            }
            DecisionOutcome::AlreadyProcessed => api::error_response(
                409,
                ApiError {
                    code: "ConflictException".to_string(),
                    message: "Decision already recorded or token expired".to_string(),
                    request_id: None,
                    reason: None,
                    field_list: None,
                },
            ),
            DecisionOutcome::Failed(msg) => {
                tracing::error!(error = %msg, "nexus-domain-approval decision call failed");
                api::error_response(
                    500,
                    ApiError {
                        code: "InternalServerException".to_string(),
                        message: "Failed to record decision".to_string(),
                        request_id: None,
                        reason: None,
                        field_list: None,
                    },
                )
            }
        },
    )
}

fn access_denied(message: &str) -> Value {
    api::error_response(
        403,
        ApiError {
            code: "AccessDeniedException".to_string(),
            message: message.to_string(),
            request_id: None,
            reason: None,
            field_list: None,
        },
    )
}

enum DecisionOutcome {
    Recorded,
    AlreadyProcessed,
    Failed(String),
}

async fn record_decision(
    sfn: &aws_sdk_sfn::Client,
    task_token: &str,
    decision: &Decision,
    decided_by: &str,
    justification: &str,
) -> DecisionOutcome {
    match decision {
        Decision::Accept => {
            let output = json!({
                "approved": true,
                "decidedBy": decided_by,
                "justification": justification,
            });

            match sfn
                .send_task_success()
                .task_token(task_token)
                .output(output.to_string())
                .send()
                .await
            {
                Ok(_) => DecisionOutcome::Recorded,
                Err(e) => {
                    let service_err = e.into_service_error();
                    if is_task_gone_success(&service_err) {
                        DecisionOutcome::AlreadyProcessed
                    } else {
                        DecisionOutcome::Failed(format!("{service_err}"))
                    }
                }
            }
        }
        Decision::Reject => {
            let cause = json!({
                "approved": false,
                "decidedBy": decided_by,
                "justification": justification,
            });

            match sfn
                .send_task_failure()
                .task_token(task_token)
                .error("RegistrationRejected")
                .cause(cause.to_string())
                .send()
                .await
            {
                Ok(_) => DecisionOutcome::Recorded,
                Err(e) => {
                    let service_err = e.into_service_error();
                    if is_task_gone_failure(&service_err) {
                        DecisionOutcome::AlreadyProcessed
                    } else {
                        DecisionOutcome::Failed(format!("{service_err}"))
                    }
                }
            }
        }
    }
}

fn is_task_gone_success(err: &SendTaskSuccessError) -> bool {
    matches!(
        err,
        SendTaskSuccessError::TaskDoesNotExist(_) | SendTaskSuccessError::TaskTimedOut(_)
    )
}

fn is_task_gone_failure(err: &SendTaskFailureError) -> bool {
    matches!(
        err,
        SendTaskFailureError::TaskDoesNotExist(_) | SendTaskFailureError::TaskTimedOut(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_mocks::mock_client;
    use lambda_runtime::Context as LambdaContext;

    fn sfn_client() -> aws_sdk_sfn::Client {
        mock_client!(aws_sdk_sfn, [])
    }

    fn event_with_context(request_context: Value, body: &str) -> LambdaEvent<Value> {
        LambdaEvent::new(
            json!({ "body": body, "requestContext": request_context }),
            LambdaContext::default(),
        )
    }

    fn oauth_admin_context() -> Value {
        json!({
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": "admin-user-123",
                "issuer": "https://cognito-idp.example.com/pool",
                "clientId": "client123",
                "globalRoles": "[\"platform-admin\"]",
                "teamGroups": "[]"
            }
        })
    }

    fn oauth_auditor_context() -> Value {
        json!({
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": "auditor-user-456",
                "issuer": "https://cognito-idp.example.com/pool",
                "clientId": "client123",
                "globalRoles": "[\"platform-auditor\"]",
                "teamGroups": "[]"
            }
        })
    }

    fn iam_context() -> Value {
        json!({
            "identity": {
                "accountId": "111122223333",
                "userArn": "arn:aws:iam::111122223333:user/admin"
            }
        })
    }

    #[tokio::test]
    async fn missing_principal_returns_403() {
        let sfn = sfn_client();
        let event = LambdaEvent::new(
            json!({"body": "{\"taskToken\":\"tok-1\",\"decision\":\"ACCEPT\"}"}),
            LambdaContext::default(),
        );
        let resp = handle(&sfn, event).await.unwrap();
        assert_eq!(resp["statusCode"], 403);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "AccessDeniedException");
    }

    #[tokio::test]
    async fn iam_principal_returns_403() {
        let sfn = sfn_client();
        let event = event_with_context(
            iam_context(),
            "{\"taskToken\":\"tok-1\",\"decision\":\"ACCEPT\"}",
        );
        let resp = handle(&sfn, event).await.unwrap();
        assert_eq!(resp["statusCode"], 403);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert!(body["message"].as_str().unwrap().contains("IAM"));
    }

    #[tokio::test]
    async fn oauth_auditor_returns_403() {
        let sfn = sfn_client();
        let event = event_with_context(
            oauth_auditor_context(),
            "{\"taskToken\":\"tok-1\",\"decision\":\"ACCEPT\"}",
        );
        let resp = handle(&sfn, event).await.unwrap();
        assert_eq!(resp["statusCode"], 403);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert!(body["message"].as_str().unwrap().contains("platform-admin"));
    }

    #[tokio::test]
    async fn oauth_admin_empty_task_token_returns_400() {
        let sfn = sfn_client();
        let event = event_with_context(
            oauth_admin_context(),
            "{\"taskToken\":\"\",\"decision\":\"ACCEPT\"}",
        );
        let resp = handle(&sfn, event).await.unwrap();
        assert_eq!(resp["statusCode"], 400);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "ValidationException");
        assert!(body["message"].as_str().unwrap().contains("taskToken"));
    }

    #[tokio::test]
    async fn oauth_admin_reject_without_justification_returns_400() {
        let sfn = sfn_client();
        let event = event_with_context(
            oauth_admin_context(),
            "{\"taskToken\":\"tok-1\",\"decision\":\"REJECT\"}",
        );
        let resp = handle(&sfn, event).await.unwrap();
        assert_eq!(resp["statusCode"], 400);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert!(body["message"].as_str().unwrap().contains("justification"));
    }

    #[tokio::test]
    async fn caller_supplied_decided_by_is_rejected_by_schema() {
        let sfn = sfn_client();
        let event = event_with_context(
            oauth_admin_context(),
            "{\"taskToken\":\"tok-1\",\"decision\":\"ACCEPT\",\"decidedBy\":\"attacker\"}",
        );
        let resp = handle(&sfn, event).await.unwrap();
        assert_eq!(resp["statusCode"], 400);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert!(body["message"]
            .as_str()
            .unwrap()
            .contains("Invalid request body"));
    }

    #[tokio::test]
    async fn oauth_admin_accept_records_decision() {
        use aws_smithy_mocks::{mock, mock_client};
        let success_mock = mock!(aws_sdk_sfn::Client::send_task_success).then_output(|| {
            aws_sdk_sfn::operation::send_task_success::SendTaskSuccessOutput::builder().build()
        });
        let sfn = mock_client!(aws_sdk_sfn, [&success_mock]);

        let event = event_with_context(
            oauth_admin_context(),
            "{\"taskToken\":\"tok-1\",\"decision\":\"ACCEPT\"}",
        );
        let resp = handle(&sfn, event).await.unwrap();
        assert_eq!(resp["statusCode"], 200);
        assert_eq!(success_mock.num_calls(), 1);
    }

    #[tokio::test]
    async fn oauth_admin_reject_records_decision() {
        use aws_smithy_mocks::{mock, mock_client};
        let failure_mock = mock!(aws_sdk_sfn::Client::send_task_failure).then_output(|| {
            aws_sdk_sfn::operation::send_task_failure::SendTaskFailureOutput::builder().build()
        });
        let sfn = mock_client!(aws_sdk_sfn, [&failure_mock]);

        let event = event_with_context(
            oauth_admin_context(),
            "{\"taskToken\":\"tok-1\",\"decision\":\"REJECT\",\"justification\":\"not ready\"}",
        );
        let resp = handle(&sfn, event).await.unwrap();
        assert_eq!(resp["statusCode"], 200);
        assert_eq!(failure_mock.num_calls(), 1);
    }

    #[tokio::test]
    async fn expired_callback_returns_409() {
        use aws_smithy_mocks::{mock, mock_client};
        let expired_mock = mock!(aws_sdk_sfn::Client::send_task_success).then_error(|| {
            SendTaskSuccessError::TaskTimedOut(
                aws_sdk_sfn::types::error::TaskTimedOut::builder().build(),
            )
        });
        let sfn = mock_client!(aws_sdk_sfn, [&expired_mock]);

        let event = event_with_context(
            oauth_admin_context(),
            "{\"taskToken\":\"expired-tok\",\"decision\":\"ACCEPT\"}",
        );
        let resp = handle(&sfn, event).await.unwrap();
        assert_eq!(resp["statusCode"], 409);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "ConflictException");
    }
}
