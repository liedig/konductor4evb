use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};
use std::env;

use nexus_common::api;
use nexus_common::error::{ControlPlaneError, ValidationExceptionReason};
use nexus_common::streams::SubscriptionId;
use nexus_common::streams_db;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;

    let Some(raw_id) = api::path_param(&event.payload, "subscriptionId") else {
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::ValidationException {
                message: "missing subscriptionId path parameter".to_string(),
                reason: ValidationExceptionReason::FieldValidationFailed,
                field_list: None,
            },
        ));
    };

    let id = match SubscriptionId::parse(&raw_id) {
        Ok(id) => id,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    let schema_name = format!("{organization}-{}@{}", id.producer_domain, id.detail_type);

    match streams_db::get_stream(
        ddb,
        &table,
        &id.producer_domain,
        &schema_name,
        &id.consumer_domain,
    )
    .await
    {
        Ok(Some(stream)) => Ok(api::success_response(
            200,
            &json!({
                "subscriptionId": stream.subscription_id().to_string(),
                "consumerDomain": stream.consumer_domain,
                "producerDomain": stream.producer_domain,
                "source": stream.source,
                "detailType": stream.detail_type,
                "schemaName": stream.schema_name,
                "status": stream.status,
                "environments": stream.environments,
                "createdAt": stream.created_at.to_rfc3339(),
                "updatedAt": stream.updated_at.to_rfc3339(),
                "removedAt": stream.removed_at.map(|t| t.to_rfc3339()),
            }),
        )),
        Ok(None) => Ok(api::error_response(
            404,
            nexus_common::models::ApiError {
                code: "ResourceNotFoundException".to_string(),
                message: format!("subscription '{raw_id}' not found"),
                request_id: None,
                reason: None,
                field_list: None,
            },
        )),
        Err(e) => {
            tracing::error!(error = %e, "event stream lookup failed");
            Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                e.to_string(),
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lambda_runtime::Context as LambdaContext;

    #[tokio::test]
    async fn malformed_subscription_id_returns_validation_error() {
        std::env::set_var("EVENT_STREAMS_TABLE", "streams");
        std::env::set_var("ORGANIZATION", "acme");
        let config = aws_config::SdkConfig::builder()
            .behavior_version(aws_config::BehaviorVersion::latest())
            .build();
        let ddb = aws_sdk_dynamodb::Client::new(&config);

        let event = LambdaEvent::new(
            json!({ "pathParameters": { "subscriptionId": "not-enough-segments" } }),
            LambdaContext::default(),
        );
        let response = handle(&ddb, event).await.unwrap();

        assert_eq!(response["statusCode"], 400);
    }
}
