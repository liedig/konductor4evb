//! `GET /schemas/{schemaName}/content`: per-environment version content.
//!
//! Publishers inspect what an environment actually runs. Without a `version`
//! selector the environment's latest is returned; an explicit version reads
//! the registry's immutable per-version record, so schemas that predate
//! content recording remain readable. Authorization is the producer path
//! only — a `publisher` binding on the producer domain or a platform role;
//! there is no consumer-context path.

use std::env;

use anyhow::{Context, Result};
use lambda_runtime::Error;
use serde_json::{json, Value};

use konductor_common::api;
use konductor_common::error::ControlPlaneError;
use konductor_common::models::ApiError;
use konductor_common::principal::TrustedPrincipal;

use crate::contract::active_domain_environments;
use crate::handler::{authorize, not_found, validation};

/// The API Gateway resource for the content route, dispatched from
/// [`crate::handler::handle`].
pub const CONTENT_RESOURCE: &str = "/schemas/{schemaName}/content";

pub async fn content(
    ddb: &aws_sdk_dynamodb::Client,
    schemas: &aws_sdk_schemas::Client,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let bindings_table = env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    let entitlements_table =
        env::var("ENTITLEMENTS_TABLE").context("ENTITLEMENTS_TABLE not set")?;

    let Some(schema_name) = api::path_param(payload, "schemaName") else {
        return Ok(validation("missing schemaName path parameter"));
    };
    let Some(domain_name) = api::query_param(payload, "domain") else {
        return Ok(validation("missing required query parameter 'domain'"));
    };
    let Some(environment) = api::query_param(payload, "environment") else {
        return Ok(validation("missing required query parameter 'environment'"));
    };
    let version = match parse_version(payload) {
        Ok(version) => version,
        Err(response) => return Ok(response),
    };

    if !authorize(
        ddb,
        &bindings_table,
        &entitlements_table,
        principal,
        &None,
        &domain_name,
        &schema_name,
    )
    .await
    {
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::AccessDenied,
        ));
    }

    let environments = match active_domain_environments(ddb, &domains_table, &domain_name).await? {
        Some(environments) => environments,
        None => {
            return Ok(not_found(format!(
                "domain '{domain_name}' not found or not ACTIVE"
            )))
        }
    };
    if !environments.iter().any(|name| name == &environment) {
        return Ok(not_found(format!(
            "environment '{environment}' not found for domain '{domain_name}'"
        )));
    }

    let registry = format!("{organization}-{environment}-{domain_name}-schema-registry");
    read_registry_content(
        schemas,
        &registry,
        &domain_name,
        &environment,
        &schema_name,
        version,
    )
    .await
}

/// One registry read: the requested version, or the environment's latest
/// when the selector was omitted. The registry's NotFoundException covers
/// the missing registry, schema, and version alike.
async fn read_registry_content(
    schemas: &aws_sdk_schemas::Client,
    registry: &str,
    domain_name: &str,
    environment: &str,
    schema_name: &str,
    version: Option<i64>,
) -> Result<Value, Error> {
    let mut call = schemas
        .describe_schema()
        .registry_name(registry)
        .schema_name(schema_name);
    if let Some(version) = version {
        call = call.schema_version(version.to_string());
    }
    match call.send().await {
        Ok(described) => {
            // A 200 without a usable version is a malformed registry
            // response; emitting `version: 0` would violate the published
            // response contract (minimum: 1).
            let resolved: i64 = match described
                .schema_version
                .as_deref()
                .and_then(|v| v.parse().ok())
            {
                Some(version) => version,
                None => {
                    tracing::error!(registry, schema_name, "registry returned no usable version");
                    return Ok(internal_error("schema content unavailable"));
                }
            };
            let Some(content) = described.content else {
                tracing::error!(registry, schema_name, "registry returned no content");
                return Ok(internal_error("schema content unavailable"));
            };
            Ok(api::success_response(
                200,
                &json!({
                    "schemaName": schema_name,
                    "domain": domain_name,
                    "environment": environment,
                    "version": resolved,
                    "content": content,
                }),
            ))
        }
        Err(e) => {
            let service_err = e.into_service_error();
            if service_err.is_not_found_exception() {
                let target = version.map(|v| format!(" version {v}")).unwrap_or_default();
                return Ok(not_found(format!(
                    "schema '{schema_name}'{target} not found in environment '{environment}'"
                )));
            }
            tracing::error!(error = %service_err, registry, "describe schema content failed");
            Ok(internal_error("schema lookup failed"))
        }
    }
}

fn internal_error(message: &str) -> Value {
    api::error_response(
        500,
        ApiError {
            code: "InternalError".to_string(),
            message: message.to_string(),
            request_id: None,
            reason: None,
            field_list: None,
        },
    )
}

/// The optional version selector: absent means the environment's latest; a
/// present value must be a positive integer.
fn parse_version(payload: &Value) -> Result<Option<i64>, Value> {
    match api::query_param(payload, "version") {
        None => Ok(None),
        Some(raw) => match raw.parse::<i64>() {
            Ok(version) if version >= 1 => Ok(Some(version)),
            _ => Err(validation(
                "query parameter 'version' must be a positive integer",
            )),
        },
    }
}
