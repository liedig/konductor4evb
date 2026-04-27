use anyhow::Result;
use aws_sdk_sfn::operation::send_task_failure::SendTaskFailureError;
use aws_sdk_sfn::operation::send_task_success::SendTaskSuccessError;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::{json, Value};

use nexus_common::api;
use nexus_common::models::{ApiError, DecideRequest, Decision};

const EMAIL_LINK_ACTOR: &str = "email-link";
const EMAIL_LINK_JUSTIFICATION: &str = "Rejected via email link (no justification provided)";

async fn handler(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let sfn = aws_sdk_sfn::Client::new(&config);

    let http_method = event
        .payload
        .get("httpMethod")
        .and_then(|v| v.as_str())
        .unwrap_or("POST");

    if http_method.eq_ignore_ascii_case("GET") {
        return Ok(handle_get(&sfn, &event.payload).await);
    }

    Ok(handle_post(&sfn, &event.payload).await)
}

async fn handle_post(sfn: &aws_sdk_sfn::Client, event: &Value) -> Value {
    let request: DecideRequest = match api::parse_body(event) {
        Ok(r) => r,
        Err(e) => return api::from_control_plane_error(&e),
    };

    if request.task_token.is_empty() {
        return api::from_control_plane_error(&api::validation_error("taskToken is required"));
    }

    if request.decided_by.is_empty() {
        return api::from_control_plane_error(&api::validation_error("decidedBy is required"));
    }

    if request.decision == Decision::Reject
        && request.justification.as_deref().unwrap_or("").is_empty()
    {
        return api::from_control_plane_error(&api::validation_error(
            "justification is required when decision is REJECT",
        ));
    }

    let justification = request.justification.unwrap_or_default();

    match record_decision(
        sfn,
        &request.task_token,
        &request.decision,
        &request.decided_by,
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
            tracing::error!(error = %msg, "SFN decision call failed");
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
    }
}

async fn handle_get(sfn: &aws_sdk_sfn::Client, event: &Value) -> Value {
    let decision_raw = api::query_param(event, "decision").unwrap_or_default();
    let token = api::query_param(event, "token").unwrap_or_default();

    if token.is_empty() {
        return html_response(
            400,
            "Missing token",
            "The approval link is missing its token. Please use the link from the notification email.",
        );
    }

    let decision = match decision_raw.to_ascii_lowercase().as_str() {
        "approve" | "accept" => Decision::Accept,
        "reject" | "decline" => Decision::Reject,
        _ => {
            return html_response(
                400,
                "Invalid decision",
                "The decision parameter must be `approve` or `reject`.",
            );
        }
    };

    let justification = match decision {
        Decision::Accept => String::new(),
        Decision::Reject => EMAIL_LINK_JUSTIFICATION.to_string(),
    };

    match record_decision(sfn, &token, &decision, EMAIL_LINK_ACTOR, &justification).await {
        DecisionOutcome::Recorded => {
            let (title, body) = match decision {
                Decision::Accept => (
                    "Approved",
                    "The domain registration has been approved. Provisioning will begin shortly.",
                ),
                Decision::Reject => (
                    "Rejected",
                    "The domain registration has been rejected. The domain owner will be notified.",
                ),
            };
            html_response(200, title, body)
        }
        DecisionOutcome::AlreadyProcessed => html_response(
            200,
            "Already processed",
            "This registration has already been decided or the approval window has expired.",
        ),
        DecisionOutcome::Failed(msg) => {
            tracing::error!(error = %msg, "SFN decision call failed");
            html_response(
                500,
                "Error",
                "We could not record your decision. Please contact the platform team.",
            )
        }
    }
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
                        DecisionOutcome::Failed(format!("{service_err:?}"))
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
                        DecisionOutcome::Failed(format!("{service_err:?}"))
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

fn html_response(status_code: u16, title: &str, body: &str) -> Value {
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
  body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; padding: 48px; max-width: 560px; margin: 0 auto; color: #1a1a1a; }}
  h1 {{ font-size: 24px; margin-bottom: 12px; }}
  p {{ line-height: 1.5; font-size: 16px; }}
</style>
</head>
<body>
<h1>{title}</h1>
<p>{body}</p>
</body>
</html>"#
    );
    json!({
        "statusCode": status_code,
        "headers": { "Content-Type": "text/html; charset=utf-8" },
        "body": html
    })
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    lambda_runtime::run(service_fn(handler)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_response_has_correct_content_type() {
        let resp = html_response(200, "t", "b");
        assert_eq!(resp["statusCode"], 200);
        assert_eq!(resp["headers"]["Content-Type"], "text/html; charset=utf-8");
    }

    #[test]
    fn html_response_escapes_into_body() {
        let resp = html_response(200, "Approved", "Provisioning starting");
        let body = resp["body"].as_str().unwrap();
        assert!(body.contains("<h1>Approved</h1>"));
        assert!(body.contains("Provisioning starting"));
    }

    #[test]
    fn html_response_renders_error_codes() {
        let resp = html_response(500, "Oops", "Try again");
        assert_eq!(resp["statusCode"], 500);
    }
}
