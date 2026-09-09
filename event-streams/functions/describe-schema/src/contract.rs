//! `GET /schemas/{schemaName}/contract`: exact composed-contract discovery.
//!
//! Current reads resolve the environment's active `(business version,
//! metadata revision)` pair; historical reads require both version
//! selectors, never silently pairing a historical business version with the
//! active metadata revision. The legacy metadata revision 0 serves events
//! predating metadata stamping. Authorization is the existing catalog
//! visibility — current bindings and entitlements, not historical ones.

use std::env;

use anyhow::{Context, Result};
use lambda_runtime::Error;
use serde_json::{json, Value};

use konductor_common::api;
use konductor_common::dynamodb::{get_s, get_string_list, s};
use konductor_common::metadata::admission::compiled_schema_for;
use konductor_common::metadata::composition::compose_detail_schema;
use konductor_common::metadata::definition::{
    GeneratedField, MetadataDefinition, ValidatedMetadataDefinition,
};
use konductor_common::metadata::revisions::MetadataStore;
use konductor_common::metadata::schema::{legacy_metadata_schema, CompiledMetadataSchema};
use konductor_common::models::ApiError;
use konductor_common::principal::TrustedPrincipal;

use crate::handler::{authorize, not_found, validation};

/// The API Gateway resource for the contract route, dispatched from
/// [`crate::handler::handle`].
pub const CONTRACT_RESOURCE: &str = "/schemas/{schemaName}/contract";

/// Everything the response needs: the exact pair plus the components the
/// contract was composed from.
struct ContractPair {
    business_version: i64,
    business_content: String,
    metadata: MetadataSide,
}

/// The metadata half of a contract: the exact revision, its compiled schema,
/// and the read-only generator catalogue.
struct MetadataSide {
    revision: u32,
    compiled: CompiledMetadataSchema,
    definition_digest: Option<String>,
    generation: Option<u64>,
    generators: Vec<GeneratedField>,
}

pub async fn contract(
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
    let metadata_table = env::var("METADATA_TABLE").context("METADATA_TABLE not set")?;

    let Some(schema_name) = api::path_param(payload, "schemaName") else {
        return Ok(validation("missing schemaName path parameter"));
    };
    let Some(domain_name) = api::query_param(payload, "domain") else {
        return Ok(validation("missing required query parameter 'domain'"));
    };
    let Some(environment) = api::query_param(payload, "environment") else {
        return Ok(validation("missing required query parameter 'environment'"));
    };
    let consumer = api::query_param(payload, "consumerDomain");
    let historical = match parse_historical_selectors(payload) {
        Ok(selectors) => selectors,
        Err(response) => return Ok(response),
    };

    if !authorize(
        ddb,
        &bindings_table,
        &entitlements_table,
        principal,
        &consumer,
        &domain_name,
        &schema_name,
    )
    .await
    {
        return Ok(api::from_control_plane_error(
            &konductor_common::error::ControlPlaneError::AccessDenied,
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
    if !environments.iter().any(|env| env == &environment) {
        return Ok(not_found(format!(
            "environment '{environment}' is not provisioned for domain '{domain_name}'"
        )));
    }

    let registry = format!("{organization}-{environment}-{domain_name}-schema-registry");
    let store = MetadataStore::new(ddb.clone(), metadata_table.clone());
    let pair = match resolve_pair(
        &store,
        schemas,
        &registry,
        &schema_name,
        &environment,
        historical,
    )
    .await
    {
        Ok(pair) => pair,
        Err(response) => return Ok(response),
    };

    contract_response(&schema_name, &environment, &registry, pair)
}

/// Historical selectors are all-or-nothing: a business version without a
/// metadata revision would silently compose against the active revision,
/// which is exactly what the contract must never guess.
fn parse_historical_selectors(payload: &Value) -> Result<Option<(i64, u32)>, Value> {
    let schema_version = api::query_param(payload, "schemaVersion");
    let metadata_revision = api::query_param(payload, "metadataRevision");
    match (schema_version, metadata_revision) {
        (None, None) => Ok(None),
        (Some(version), Some(revision)) => {
            let version: i64 = version
                .parse()
                .map_err(|_| validation("schemaVersion must be a positive integer"))?;
            let revision: u32 = revision
                .parse()
                .map_err(|_| validation("metadataRevision must be a non-negative integer"))?;
            if version < 1 {
                return Err(validation("schemaVersion must be a positive integer"));
            }
            Ok(Some((version, revision)))
        }
        (Some(_), None) => Err(validation(
            "historical contract reads require both schemaVersion and metadataRevision",
        )),
        (None, Some(_)) => Err(validation(
            "historical contract reads require both schemaVersion and metadataRevision",
        )),
    }
}

/// The domain's environments when it is ACTIVE; `None` is a 404.
async fn active_domain_environments(
    ddb: &aws_sdk_dynamodb::Client,
    domains_table: &str,
    domain_name: &str,
) -> Result<Option<Vec<String>>, Error> {
    let output = ddb
        .get_item()
        .table_name(domains_table)
        .key("domainName", s(domain_name))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("failed to get domain record: {e}"))?;
    Ok(match output.item {
        Some(item)
            if get_s(&item, "status")
                .unwrap_or_default()
                .parse::<konductor_common::models::DomainStatus>()
                == Ok(konductor_common::models::DomainStatus::Active) =>
        {
            get_string_list(&item, "environments")
        }
        _ => None,
    })
}

/// Resolves the exact pair: metadata first (pointer for current reads,
/// revision number for historical), then the business content at the
/// matching version.
async fn resolve_pair(
    store: &MetadataStore,
    schemas: &aws_sdk_schemas::Client,
    registry: &str,
    schema_name: &str,
    environment: &str,
    historical: Option<(i64, u32)>,
) -> Result<ContractPair, Value> {
    let (business_version, metadata_revision) = match historical {
        Some((version, revision)) => (
            Some(version),
            revision_metadata(
                store,
                revision,
                not_found(format!("metadata revision {revision} not found")),
            )
            .await?,
        ),
        None => (None, current_metadata(store, environment).await?),
    };
    let business = describe_business(schemas, registry, schema_name, business_version).await?;
    Ok(ContractPair {
        business_version: business.0,
        business_content: business.1,
        metadata: metadata_revision,
    })
}

/// The active selection: one strongly consistent pointer read, mirroring
/// admission. A missing or unreadable pointer is a 503 — never a legacy
/// fallback. A legacy pointer (revision 0) serves the legacy contract.
async fn current_metadata(store: &MetadataStore, environment: &str) -> Result<MetadataSide, Value> {
    let pointer = store
        .get_environment_pointer(environment)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, environment, "metadata pointer read failed");
            unavailable("metadata configuration unavailable")
        })?
        .ok_or_else(|| unavailable("metadata configuration unavailable"))?;
    if pointer.active_revision == 0 {
        return Ok(MetadataSide {
            revision: 0,
            compiled: legacy_metadata_schema(),
            definition_digest: None,
            generation: Some(pointer.generation),
            generators: Vec::new(),
        });
    }
    let mut side = revision_metadata(
        store,
        pointer.active_revision,
        unavailable("metadata configuration unavailable"),
    )
    .await?;
    side.generation = Some(pointer.generation);
    Ok(side)
}

/// One exact revision. The caller supplies the response for a missing
/// record: a 404 for historical reads (an unknown component) and a 503 for
/// the current path (a pointed-at revision that is absent is corruption,
/// mirroring admission).
async fn revision_metadata(
    store: &MetadataStore,
    revision: u32,
    missing: Value,
) -> Result<MetadataSide, Value> {
    if revision == 0 {
        return Ok(MetadataSide {
            revision: 0,
            compiled: legacy_metadata_schema(),
            definition_digest: None,
            generation: None,
            generators: Vec::new(),
        });
    }
    let record = store.get_revision(revision).await.map_err(|error| {
        tracing::error!(error = %error, revision, "metadata revision read failed");
        unavailable("metadata configuration unavailable")
    })?;
    let Some(record) = record else {
        return Err(missing);
    };
    let parsed: MetadataDefinition = serde_json::from_str(&record.definition).map_err(|error| {
        tracing::error!(error = %error, revision, "stored revision definition does not parse");
        unavailable("metadata configuration unavailable")
    })?;
    let validated = ValidatedMetadataDefinition::validate(parsed).map_err(|error| {
        tracing::error!(error = %error, revision, "stored revision definition is invalid");
        unavailable("metadata configuration unavailable")
    })?;
    let compiled = compiled_schema_for(&record).map_err(|error| {
        tracing::error!(error = %error, revision, "stored revision schema does not parse");
        unavailable("metadata configuration unavailable")
    })?;
    Ok(MetadataSide {
        revision,
        compiled,
        definition_digest: Some(record.definition_digest),
        generation: None,
        generators: validated.generated_fields(),
    })
}

/// The business half: raw stored content at the latest version for current
/// reads, or at the explicit version for historical reads.
async fn describe_business(
    schemas: &aws_sdk_schemas::Client,
    registry: &str,
    schema_name: &str,
    version: Option<i64>,
) -> Result<(i64, String), Value> {
    let mut request = schemas
        .describe_schema()
        .registry_name(registry)
        .schema_name(schema_name);
    if let Some(version) = version {
        request = request.schema_version(version.to_string());
    }
    match request.send().await {
        Ok(output) => {
            let version = output
                .schema_version
                .as_deref()
                .and_then(|v| v.parse().ok())
                .unwrap_or_default();
            match output.content {
                Some(content) => Ok((version, content)),
                None => Err(unavailable("schema content unavailable")),
            }
        }
        Err(e) => {
            let service_err = e.into_service_error();
            if service_err.is_not_found_exception() {
                Err(not_found(format!(
                    "schema '{schema_name}' not found in registry '{registry}'"
                )))
            } else {
                tracing::error!(error = %service_err, registry, "describe schema failed");
                Err(unavailable("schema lookup failed"))
            }
        }
    }
}

/// Composes the pair into the offline draft-4 document and builds the
/// response with every component a consumer needs to validate exactly:
/// tuple, digests, raw components, generator descriptions, and the composed
/// contract.
fn contract_response(
    schema_name: &str,
    environment: &str,
    registry: &str,
    pair: ContractPair,
) -> Result<Value, Error> {
    let composed = match compose_detail_schema(
        pair.business_version,
        &pair.business_content,
        &pair.metadata.compiled,
    ) {
        Ok(composed) => composed,
        Err(error) => {
            tracing::error!(error = %error, schema_name, "composed contract unavailable");
            return Ok(unavailable("composed contract unavailable"));
        }
    };
    let (source, detail_type) = schema_name.split_once('@').unwrap_or((schema_name, ""));
    let generators: Vec<Value> = pair
        .metadata
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
    Ok(api::success_response(
        200,
        &json!({
            "schemaName": schema_name,
            "source": source,
            "detailType": detail_type,
            "environment": environment,
            "registry": registry,
            "schemaVersion": pair.business_version,
            "metadataRevision": pair.metadata.revision,
            "generation": pair.metadata.generation,
            "definitionDigest": pair.metadata.definition_digest,
            "businessContent": pair.business_content,
            "metadataSchema": pair.metadata.compiled.document_string(),
            "generators": generators,
            "composerFormat": composed.composer_format,
            "composedDocument": composed.document_string(),
        }),
    ))
}

fn unavailable(message: &str) -> Value {
    api::error_response(
        503,
        ApiError {
            code: "ServiceUnavailableException".to_string(),
            message: message.to_string(),
            request_id: None,
            reason: None,
            field_list: None,
        },
    )
}
