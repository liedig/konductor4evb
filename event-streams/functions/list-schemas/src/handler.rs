use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};
use std::env;

use crate::discovery;
use nexus_common::api;
use nexus_common::error::{ControlPlaneError, ValidationExceptionReason};
use nexus_common::models::ApiError;
use nexus_common::principal_context::log_principal;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    schemas: &aws_sdk_schemas::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;

    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let _ = log_principal(request_context);

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

    let env_filter = api::query_param(&event.payload, "environment");

    let environments: Vec<String> = match &env_filter {
        Some(env) if domain.environments.contains(env) => vec![env.clone()],
        Some(env) => {
            return Ok(api::error_response(
                404,
                ApiError {
                    code: "ResourceNotFoundException".to_string(),
                    message: format!(
                        "environment '{env}' is not provisioned for domain '{domain_name}'"
                    ),
                    request_id: None,
                    reason: None,
                    field_list: None,
                },
            ))
        }
        None => domain.environments.clone(),
    };

    match discovery::schemas_by_environment(schemas, &organization, &domain_name, &environments)
        .await
    {
        Ok(per_env) => {
            let envs: Value = environments
                .iter()
                .map(|env| {
                    let registry = format!("{organization}-{env}-{domain_name}-schema-registry");
                    let schemas: Vec<Value> = per_env
                        .get(env)
                        .map(|entries| {
                            entries
                                .iter()
                                .map(|(name, version)| discovery::schema_entry(name, *version))
                                .collect()
                        })
                        .unwrap_or_default();
                    (
                        env.clone(),
                        json!({ "registry": registry, "schemas": schemas }),
                    )
                })
                .collect();
            Ok(api::success_response(
                200,
                &json!({ "domain": domain_name, "environments": envs }),
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
