use anyhow::{Context, Result};
use std::env;

use konductor_common::dynamodb::{get_s, get_string_list, s};
use konductor_common::models::DomainStatus;

pub struct ActiveDomain {
    pub environments: Vec<String>,
}

/// Looks up the domain and requires it to be ACTIVE — discovery serves only
/// live catalogs.
pub async fn active_domain(
    ddb: &aws_sdk_dynamodb::Client,
    domain_name: &str,
) -> Result<Option<ActiveDomain>> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let output = ddb
        .get_item()
        .table_name(&table)
        .key("domainName", s(domain_name))
        .send()
        .await
        .context("failed to get domain record")?;

    let Some(item) = output.item else {
        return Ok(None);
    };
    let status = get_s(&item, "status").unwrap_or_default();
    if status.parse::<DomainStatus>() != Ok(DomainStatus::Active) {
        return Ok(None);
    }
    Ok(Some(ActiveDomain {
        environments: get_string_list(&item, "environments").unwrap_or_default(),
    }))
}

/// The latest version of a schema in one registry — DescribeSchema without a
/// version returns the latest.
pub async fn latest_version(
    schemas: &aws_sdk_schemas::Client,
    registry: &str,
    schema_name: &str,
) -> Result<Option<i64>> {
    match schemas
        .describe_schema()
        .registry_name(registry)
        .schema_name(schema_name)
        .send()
        .await
    {
        Ok(output) => Ok(output
            .schema_version
            .and_then(|version| version.parse().ok())),
        Err(e) => {
            let service_err = e.into_service_error();
            if service_err.is_not_found_exception() {
                Ok(None)
            } else {
                Err(anyhow::anyhow!(
                    "failed to describe schema {schema_name} in {registry}: {service_err}"
                ))
            }
        }
    }
}
