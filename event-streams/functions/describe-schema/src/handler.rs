use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;

use nexus_common::api;
use nexus_common::dynamodb::{get_s, get_string_list, s};
use nexus_common::error::{ControlPlaneError, ValidationExceptionReason};
use nexus_common::models::{ApiError, DomainStatus};
use nexus_common::principal_context::log_principal;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    schemas: &aws_sdk_schemas::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;

    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let _ = log_principal(request_context);

    let Some(schema_name) = api::path_param(&event.payload, "schemaName") else {
        return Ok(validation("missing schemaName path parameter"));
    };
    let Some(domain_name) = api::query_param(&event.payload, "domain") else {
        return Ok(validation("missing required query parameter 'domain'"));
    };

    let output = ddb
        .get_item()
        .table_name(&domains_table)
        .key("domainName", s(&domain_name))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("failed to get domain record: {e}"))?;
    let environments = match output.item {
        Some(item)
            if get_s(&item, "status")
                .unwrap_or_default()
                .parse::<DomainStatus>()
                == Ok(DomainStatus::Active) =>
        {
            get_string_list(&item, "environments").unwrap_or_default()
        }
        _ => {
            return Ok(not_found(format!(
                "domain '{domain_name}' not found or not ACTIVE"
            )))
        }
    };

    let mut latest_versions: BTreeMap<String, i64> = BTreeMap::new();
    let mut content: Option<String> = None;
    let mut content_version: i64 = 0;

    for env_name in &environments {
        let registry = format!("{organization}-{env_name}-{domain_name}-schema-registry");
        match schemas
            .describe_schema()
            .registry_name(&registry)
            .schema_name(&schema_name)
            .send()
            .await
        {
            Ok(described) => {
                let version: i64 = described
                    .schema_version
                    .as_deref()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                latest_versions.insert(env_name.clone(), version);
                if version > content_version {
                    content_version = version;
                    content = described.content;
                }
            }
            Err(e) => {
                let service_err = e.into_service_error();
                if !service_err.is_not_found_exception() {
                    tracing::error!(error = %service_err, registry, "describe schema failed");
                    return Ok(api::error_response(
                        500,
                        ApiError {
                            code: "InternalError".to_string(),
                            message: "schema lookup failed".to_string(),
                            request_id: None,
                            reason: None,
                            field_list: None,
                        },
                    ));
                }
            }
        }
    }

    if latest_versions.is_empty() {
        return Ok(not_found(format!(
            "schema '{schema_name}' not found in any environment of domain '{domain_name}'"
        )));
    }

    let (source, detail_type) = schema_name
        .split_once('@')
        .unwrap_or((schema_name.as_str(), ""));
    Ok(api::success_response(
        200,
        &json!({
            "schemaName": schema_name,
            "source": source,
            "detailType": detail_type,
            "latestVersions": latest_versions,
            "content": content,
        }),
    ))
}

fn validation(message: &str) -> Value {
    api::from_control_plane_error(&ControlPlaneError::ValidationException {
        message: message.to_string(),
        reason: ValidationExceptionReason::FieldValidationFailed,
        field_list: None,
    })
}

fn not_found(message: String) -> Value {
    api::error_response(
        404,
        ApiError {
            code: "ResourceNotFoundException".to_string(),
            message,
            request_id: None,
            reason: None,
            field_list: None,
        },
    )
}
