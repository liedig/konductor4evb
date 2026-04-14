use anyhow::Result;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::{json, Value};

use control_plane_common::api;
use control_plane_common::models::{ApiError, DecideRequest, Decision};

async fn handler(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let sfn = aws_sdk_sfn::Client::new(&config);

    let request: DecideRequest = match api::parse_body(&event.payload) {
        Ok(r) => r,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    if request.task_token.is_empty() {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "taskToken is required",
        )));
    }

    if request.decided_by.is_empty() {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "decidedBy is required",
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

    match request.decision {
        Decision::Accept => {
            let output = json!({
                "approved": true,
                "decidedBy": request.decided_by,
                "justification": justification,
            });

            if let Err(e) = sfn
                .send_task_success()
                .task_token(&request.task_token)
                .output(output.to_string())
                .send()
                .await
            {
                tracing::error!("SFN send_task_success error: {e:?}");
                return Ok(api::error_response(
                    500,
                    ApiError {
                        code: "InternalServerException".to_string(),
                        message: "Failed to record decision".to_string(),
                        request_id: None,
                        reason: None,
                        field_list: None,
                    },
                ));
            }
        }
        Decision::Reject => {
            let cause = json!({
                "approved": false,
                "decidedBy": request.decided_by,
                "justification": justification,
            });

            if let Err(e) = sfn
                .send_task_failure()
                .task_token(&request.task_token)
                .error("RegistrationRejected")
                .cause(cause.to_string())
                .send()
                .await
            {
                tracing::error!("SFN send_task_failure error: {e:?}");
                return Ok(api::error_response(
                    500,
                    ApiError {
                        code: "InternalServerException".to_string(),
                        message: "Failed to record decision".to_string(),
                        request_id: None,
                        reason: None,
                        field_list: None,
                    },
                ));
            }
        }
    }

    Ok(api::success_response(
        200,
        &json!({"message": "Decision recorded"}),
    ))
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    lambda_runtime::run(service_fn(handler)).await
}
