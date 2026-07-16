use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;

use nexus_common::dynamodb::{get_s, get_string_list, s};
use nexus_common::models::DomainStatus;

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

/// Latest version of every schema in each environment's registry, merged and
/// keyed by schema name. Skew across environments is visible in the result.
pub async fn latest_versions_by_schema(
    schemas: &aws_sdk_schemas::Client,
    organization: &str,
    domain_name: &str,
    environments: &[String],
) -> Result<BTreeMap<String, BTreeMap<String, i64>>> {
    let mut merged: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();

    for env_name in environments {
        let registry = format!("{organization}-{env_name}-{domain_name}-schema-registry");
        let mut next_token: Option<String> = None;
        loop {
            let mut request = schemas.list_schemas().registry_name(&registry);
            if let Some(token) = &next_token {
                request = request.next_token(token);
            }
            let output = match request.send().await {
                Ok(output) => output,
                Err(e) => {
                    let service_err = e.into_service_error();
                    if service_err.to_string().contains("NotFound") {
                        break;
                    }
                    return Err(anyhow::anyhow!(
                        "failed to list schemas in {registry}: {service_err}"
                    ));
                }
            };

            for summary in output.schemas.unwrap_or_default() {
                let Some(name) = summary.schema_name else {
                    continue;
                };
                if let Some(version) = latest_version(schemas, &registry, &name).await? {
                    merged
                        .entry(name)
                        .or_default()
                        .insert(env_name.clone(), version);
                }
            }

            next_token = output.next_token;
            if next_token.is_none() {
                break;
            }
        }
    }

    Ok(merged)
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

pub fn schema_summary(schema_name: &str, latest_versions: &BTreeMap<String, i64>) -> Value {
    let (source, detail_type) = schema_name.split_once('@').unwrap_or((schema_name, ""));
    json!({
        "schemaName": schema_name,
        "source": source,
        "detailType": detail_type,
        "latestVersions": latest_versions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_summary_splits_identity() {
        let mut versions = BTreeMap::new();
        versions.insert("dev".to_string(), 5);
        versions.insert("prod".to_string(), 3);

        let summary = schema_summary("acme-orders@OrderNotification", &versions);

        assert_eq!(summary["source"], "acme-orders");
        assert_eq!(summary["detailType"], "OrderNotification");
        assert_eq!(summary["latestVersions"]["dev"], 5);
        assert_eq!(summary["latestVersions"]["prod"], 3);
    }
}
