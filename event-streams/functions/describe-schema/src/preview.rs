//! `POST /schemas/contract-preview`: the effective-contract preview for
//! publisher authoring.
//!
//! Composes unsaved business content with the environment's active metadata
//! revision and returns the composed document — the same composition the
//! contract route serves, minus any registry write. Preview never registers
//! or publishes the draft, never widens catalog visibility beyond the
//! publisher's own domain, and composes against the metadata configuration
//! exactly as a subsequent admission would.

use std::env;

use anyhow::{Context, Result};
use lambda_runtime::Error;
use serde::Deserialize;
use serde_json::{json, Value};

use konductor_common::api;
use konductor_common::grant::ResourceRole;
use konductor_common::grant_engine::has_role_binding;
use konductor_common::grant_store::BindingStore;
use konductor_common::metadata::composition::compose_detail_schema;
use konductor_common::metadata::revisions::MetadataStore;
use konductor_common::principal::TrustedPrincipal;

use crate::contract::{active_domain_environments, current_metadata, unavailable};
use crate::handler::{is_platform, not_found, validation};

/// The API Gateway resource for the preview route, dispatched from
/// [`crate::handler::handle`].
pub const PREVIEW_RESOURCE: &str = "/schemas/contract-preview";

/// The unsaved draft and where it would run. `schema_version` pins the
/// composed document's version stamp; without it the preview uses the
/// environment's current latest version (or 1 for a first draft).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewRequest {
    domain: String,
    schema_name: String,
    environment: String,
    content: String,
    schema_version: Option<i64>,
}

pub async fn preview(
    ddb: &aws_sdk_dynamodb::Client,
    schemas: &aws_sdk_schemas::Client,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let bindings_table = env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    let metadata_table = env::var("METADATA_TABLE").context("METADATA_TABLE not set")?;

    let request: PreviewRequest = match api::parse_body(payload) {
        Ok(request) => request,
        Err(error) => return Ok(api::from_control_plane_error(&error)),
    };
    if request.domain.is_empty()
        || request.schema_name.is_empty()
        || request.environment.is_empty()
        || request.content.trim().is_empty()
    {
        return Ok(validation(
            "domain, schemaName, environment, and content are required and content must be a non-empty schema document",
        ));
    }
    if let Some(version) = request.schema_version {
        if version < 1 {
            return Ok(validation("schemaVersion must be a positive integer"));
        }
    }

    if !is_platform(principal)
        && !has_role_binding(
            &BindingStore::new(ddb.clone(), &bindings_table),
            principal,
            ResourceRole::Publisher,
            &request.domain,
        )
        .await
    {
        tracing::warn!(
            domain = request.domain,
            "contract preview: deny (no publisher binding)"
        );
        return Ok(api::from_control_plane_error(
            &konductor_common::error::ControlPlaneError::AccessDenied,
        ));
    }

    let environments =
        match active_domain_environments(ddb, &domains_table, &request.domain).await? {
            Some(environments) => environments,
            None => {
                return Ok(not_found(format!(
                    "domain '{}' not found or not ACTIVE",
                    request.domain
                )))
            }
        };
    if !environments.iter().any(|env| env == &request.environment) {
        return Ok(not_found(format!(
            "environment '{}' is not provisioned for domain '{}'",
            request.environment, request.domain
        )));
    }

    let store = MetadataStore::new(ddb.clone(), metadata_table.clone());
    let metadata = match current_metadata(&store, &request.environment).await {
        Ok(metadata) => metadata,
        Err(response) => return Ok(response),
    };
    let registry = format!(
        "{organization}-{}-{}-schema-registry",
        request.environment, request.domain
    );
    let business_version = match request.schema_version {
        Some(version) => version,
        None => match latest_version(schemas, &registry, &request.schema_name).await {
            Ok(version) => version,
            Err(response) => return Ok(response),
        },
    };

    let composed = match compose_detail_schema(
        business_version,
        &request.content,
        &metadata.compiled,
    ) {
        Ok(composed) => composed,
        Err(error) => {
            tracing::info!(error = %error, schema = request.schema_name, "draft content cannot compose");
            return Ok(api::from_control_plane_error(
                &konductor_common::error::ControlPlaneError::ValidationException {
                    message: format!(
                        "draft content cannot participate in a composed contract: {error}"
                    ),
                    reason:
                        konductor_common::error::ValidationExceptionReason::FieldValidationFailed,
                    field_list: None,
                },
            ));
        }
    };

    Ok(preview_response(
        &request,
        business_version,
        metadata,
        composed,
    ))
}

/// The version stamp the composed preview pins: the environment's current
/// latest version, or 1 for a first draft of an unregistered schema.
async fn latest_version(
    schemas: &aws_sdk_schemas::Client,
    registry: &str,
    schema_name: &str,
) -> Result<i64, Value> {
    match schemas
        .describe_schema()
        .registry_name(registry)
        .schema_name(schema_name)
        .send()
        .await
    {
        Ok(output) => Ok(output
            .schema_version
            .as_deref()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1)),
        Err(e) => {
            let service_err = e.into_service_error();
            if service_err.is_not_found_exception() {
                Ok(1)
            } else {
                tracing::error!(error = %service_err, registry, "describe schema failed");
                Err(unavailable("schema lookup failed"))
            }
        }
    }
}

fn preview_response(
    request: &PreviewRequest,
    business_version: i64,
    metadata: crate::contract::MetadataSide,
    composed: konductor_common::metadata::composition::ComposedDocument,
) -> Value {
    let (source, detail_type) = request
        .schema_name
        .split_once('@')
        .unwrap_or((request.schema_name.as_str(), ""));
    let generators: Vec<Value> = metadata
        .generators
        .iter()
        .map(|field| {
            json!({
                "name": field.path,
                "generator": field.generator.as_str(),
                "source": field.generator.source_description(),
                "constraint": field.generator.generated_type().constraint_description(),
            })
        })
        .collect();
    api::success_response(
        200,
        &json!({
            "schemaName": request.schema_name,
            "source": source,
            "detailType": detail_type,
            "environment": request.environment,
            "schemaVersion": business_version,
            "metadataRevision": metadata.revision,
            "generation": metadata.generation,
            "metadataSchema": metadata.compiled.document_string(),
            "generators": generators,
            "composerFormat": composed.composer_format,
            "composedDocument": composed.document_string(),
            "registered": false,
            "note": "preview composes unsaved content; nothing is registered or published",
        }),
    )
}
