use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};
use std::env;

use crate::discovery;
use nexus_common::api;
use nexus_common::error::{ControlPlaneError, ValidationExceptionReason};
use nexus_common::models::ApiError;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    schemas: &aws_sdk_schemas::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;

    let Some(domain_name) = api::query_param(&event.payload, "domain") else {
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::ValidationException {
                message: "missing required query parameter 'domain'".to_string(),
                reason: ValidationExceptionReason::FieldValidationFailed,
                field_list: None,
            },
        ));
    };

    let domain = match discovery::active_domain(ddb, &domain_name).await {
        Ok(Some(domain)) => domain,
        Ok(None) => {
            return Ok(api::error_response(
                404,
                ApiError {
                    code: "ResourceNotFoundException".to_string(),
                    message: format!("domain '{domain_name}' not found or not ACTIVE"),
                    request_id: None,
                    reason: None,
                    field_list: None,
                },
            ))
        }
        Err(e) => {
            tracing::error!(error = %e, "domain lookup failed");
            return Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                e.to_string(),
            )));
        }
    };

    match discovery::latest_versions_by_schema(
        schemas,
        &organization,
        &domain_name,
        &domain.environments,
    )
    .await
    {
        Ok(merged) => {
            let items: Vec<Value> = merged
                .iter()
                .map(|(name, versions)| discovery::schema_summary(name, versions))
                .collect();
            Ok(api::success_response(
                200,
                &json!({ "domain": domain_name, "items": items }),
            ))
        }
        Err(e) => {
            tracing::error!(error = %e, "schema discovery failed");
            Ok(api::error_response(
                500,
                ApiError {
                    code: "InternalError".to_string(),
                    message: "schema discovery failed".to_string(),
                    request_id: None,
                    reason: None,
                    field_list: None,
                },
            ))
        }
    }
}
