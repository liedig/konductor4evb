//! Shared request plumbing for the promotion routes: stage-to-environment
//! resolution, actor labeling, source-registry reads, and table lookups.

use serde_json::Value;

use konductor_common::api;
use konductor_common::dynamodb::get_s;
use konductor_common::envelope::SchemaName;
use konductor_common::environments;
use konductor_common::error::ControlPlaneError;
use konductor_common::models::{CreatePromotionRequest, DomainStatus};
use konductor_common::namespace::NamespaceResolver;
use konductor_common::principal::TrustedPrincipal;

use crate::handler::Clients;

/// The stage selects the target; the source is the previous environment.
pub(crate) fn source_target(stage: &str) -> Result<(String, String), ControlPlaneError> {
    let source = environments::promotion_source(stage)
        .map_err(|_| {
            api::validation_error(format!("stage {stage} is not a configured environment"))
        })?
        .ok_or_else(|| {
            api::validation_error(format!(
                "stage {stage} is the authoring environment and has no source to promote from"
            ))
        })?;
    Ok((source, stage.to_string()))
}

pub(crate) fn stage_of(event: &Value) -> Result<&str, ControlPlaneError> {
    event["requestContext"]["stage"]
        .as_str()
        .ok_or_else(|| api::validation_error("request is missing its API Gateway stage"))
}

pub(crate) fn actor_of(principal: &TrustedPrincipal) -> String {
    match principal {
        TrustedPrincipal::OAuth(p) => format!("oauth:{}", p.subject),
        TrustedPrincipal::Iam(p) => format!("iam:{}", p.account_id),
    }
}

/// The source registry's latest version; also the existence check the
/// request contract requires.
pub(crate) async fn source_latest_version(
    clients: &Clients,
    source: &str,
    request: &CreatePromotionRequest,
) -> Result<i64, ControlPlaneError> {
    let organization = std::env::var("ORGANIZATION")
        .map_err(|_| ControlPlaneError::Schemas("ORGANIZATION not set".to_string()))?;
    let resolver = NamespaceResolver::default_pattern();
    let registry = resolver.schema_registry_name(source, &organization, &request.domain);
    let schema_name = SchemaName::for_domain(
        &resolver.namespace(&organization, &request.domain),
        &request.event_name,
    );
    let output = clients
        .schemas
        .describe_schema()
        .registry_name(registry)
        .schema_name(schema_name.to_string())
        .send()
        .await
        .map_err(|e| {
            let err = e.into_service_error();
            if err.is_not_found_exception() {
                ControlPlaneError::SchemaNotFound {
                    schema_name: format!("{schema_name} in source environment {source}"),
                }
            } else {
                tracing::error!(error = %err, "source registry describe failed");
                ControlPlaneError::Schemas(err.to_string())
            }
        })?;
    output
        .schema_version()
        .and_then(|v| v.parse::<i64>().ok())
        .ok_or_else(|| {
            ControlPlaneError::Schemas("source registry returned no latest version".to_string())
        })
}

pub(crate) async fn require_active_domain(
    clients: &Clients,
    domain_name: &str,
) -> Result<(), ControlPlaneError> {
    let table = std::env::var("DOMAINS_TABLE")
        .map_err(|_| ControlPlaneError::DynamoDb("DOMAINS_TABLE not set".to_string()))?;
    let output = clients
        .ddb
        .get_item()
        .table_name(table)
        .key("domainName", konductor_common::dynamodb::s(domain_name))
        .send()
        .await
        .map_err(|e| {
            let err = e.into_service_error();
            tracing::error!(error = %err, "domains table read failed");
            ControlPlaneError::DynamoDb(err.to_string())
        })?;
    let status = output
        .item
        .as_ref()
        .and_then(|item| get_s(item, "status"))
        .unwrap_or_default();
    if status.parse::<DomainStatus>() != Ok(DomainStatus::Active) {
        return Err(ControlPlaneError::DomainNotFound {
            domain_name: domain_name.to_string(),
        });
    }
    Ok(())
}

pub(crate) fn promotions_table() -> Result<String, ControlPlaneError> {
    std::env::var("PROMOTIONS_TABLE")
        .map_err(|_| ControlPlaneError::DynamoDb("PROMOTIONS_TABLE not set".to_string()))
}
