//! Shared plumbing for the promotion steps: table names, version-record
//! reads/writes, registry reads, and head-item progress commits.

use anyhow::{Context, Result};
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_schemas::Client as SchemasClient;
use chrono::Utc;
use serde::Deserialize;

use konductor_common::dynamodb::{n, s};
use konductor_common::envelope::SchemaName;
use konductor_common::namespace::NamespaceResolver;
use konductor_common::promotion::{promotion_pk, version_sk, SchemaVersionContent, PROMOTION_SK};
use konductor_common::promotion_store::{version_content_from_item, version_content_to_item};

pub struct Clients {
    pub ddb: DynamoDbClient,
    pub schemas: SchemasClient,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: DynamoDbClient::new(config),
            schemas: SchemasClient::new(config),
        }
    }
}

/// Fields every step input carries.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepInput {
    pub promotion_id: String,
    pub domain: String,
    pub schema_name: String,
    pub source_environment: String,
    pub target_environment: String,
}

pub fn versions_table() -> Result<String> {
    std::env::var("SCHEMA_VERSIONS_TABLE").context("SCHEMA_VERSIONS_TABLE not set")
}

pub fn promotions_table() -> Result<String> {
    std::env::var("PROMOTIONS_TABLE").context("PROMOTIONS_TABLE not set")
}

pub fn registry_name(environment: &str, domain: &str) -> Result<String> {
    let organization = std::env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    Ok(NamespaceResolver::default_pattern().schema_registry_name(
        environment,
        &organization,
        domain,
    ))
}

pub fn full_schema_name(domain: &str, event_name: &str) -> Result<String> {
    let organization = std::env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let resolver = NamespaceResolver::default_pattern();
    Ok(SchemaName::for_domain(&resolver.namespace(&organization, domain), event_name).to_string())
}

pub async fn read_record(
    clients: &Clients,
    domain: &str,
    schema_name: &str,
    environment: &str,
    version: i64,
) -> Result<Option<SchemaVersionContent>> {
    let output = clients
        .ddb
        .get_item()
        .table_name(versions_table()?)
        .key("pk", s(&promotion_pk(domain, schema_name, environment)))
        .key("sk", s(&version_sk(version)))
        .consistent_read(true)
        .send()
        .await
        .map_err(|e| anyhow::Error::new(e.into_service_error()))
        .context("read version record failed")?;
    output
        .item
        .map(|item| {
            version_content_from_item(&item)
                .ok_or_else(|| anyhow::anyhow!("malformed version record {version}"))
        })
        .transpose()
}

pub async fn write_record(clients: &Clients, record: &SchemaVersionContent) -> Result<()> {
    let mut item = version_content_to_item(record);
    item.insert(
        "pk".to_string(),
        s(&promotion_pk(
            &record.domain,
            &record.schema_name,
            &record.environment,
        )),
    );
    item.insert("sk".to_string(), s(&version_sk(record.version)));
    clients
        .ddb
        .put_item()
        .table_name(versions_table()?)
        .set_item(Some(item))
        .send()
        .await
        .map_err(|e| anyhow::Error::new(e.into_service_error()))
        .context("write version record failed")?;
    Ok(())
}

/// The registry's latest version for the schema, or `None` when the schema
/// does not exist in this environment's registry.
pub async fn registry_latest(
    clients: &Clients,
    environment: &str,
    domain: &str,
    event_name: &str,
) -> Result<Option<i64>> {
    let registry = registry_name(environment, domain)?;
    let schema = full_schema_name(domain, event_name)?;
    let mut latest = None;
    let mut next_token = None;
    loop {
        let result = clients
            .schemas
            .list_schema_versions()
            .registry_name(&registry)
            .schema_name(&schema)
            .set_next_token(next_token)
            .send()
            .await;
        let output = match result {
            Ok(o) => o,
            Err(e) => {
                let err = e.into_service_error();
                if err.is_not_found_exception() {
                    return Ok(None);
                }
                return Err(anyhow::Error::new(err)).context("list schema versions failed");
            }
        };
        for entry in output.schema_versions() {
            if let Some(Ok(v)) = entry.schema_version().map(str::parse::<i64>) {
                latest = Some(latest.map_or(v, |m: i64| m.max(v)));
            }
        }
        next_token = output.next_token().map(str::to_string);
        if next_token.is_none() {
            return Ok(latest);
        }
    }
}

/// The registry's latest version content (registry-normalized), or `None`
/// when the schema does not exist.
pub async fn describe_latest(
    clients: &Clients,
    environment: &str,
    domain: &str,
    event_name: &str,
) -> Result<Option<(i64, String)>> {
    let registry = registry_name(environment, domain)?;
    let schema = full_schema_name(domain, event_name)?;
    let result = clients
        .schemas
        .describe_schema()
        .registry_name(registry)
        .schema_name(schema)
        .send()
        .await;
    match result {
        Ok(output) => {
            let version = output
                .schema_version()
                .and_then(|v| v.parse::<i64>().ok())
                .context("registry returned no version")?;
            let content = output.content().unwrap_or_default().to_string();
            Ok(Some((version, content)))
        }
        Err(e) => {
            let err = e.into_service_error();
            if err.is_not_found_exception() {
                Ok(None)
            } else {
                Err(anyhow::Error::new(err).context("describe schema failed"))
            }
        }
    }
}

/// Committed replay progress on the head item, guarded by the promotion id
/// so a superseded workflow can never move a newer promotion's watermark.
pub async fn commit_progress(clients: &Clients, input: &StepInput, version: i64) -> Result<()> {
    clients
        .ddb
        .update_item()
        .table_name(promotions_table()?)
        .key(
            "pk",
            s(&promotion_pk(
                &input.domain,
                &input.schema_name,
                &input.target_environment,
            )),
        )
        .key("sk", s(PROMOTION_SK))
        .update_expression("SET lastWrittenVersion = :v, updatedAt = :t")
        .condition_expression("promotionId = :pid")
        .expression_attribute_values(":v", n(version))
        .expression_attribute_values(":t", s(&Utc::now().to_rfc3339()))
        .expression_attribute_values(":pid", s(&input.promotion_id))
        .send()
        .await
        .map_err(|e| anyhow::Error::new(e.into_service_error()))
        .context("commit replay progress failed")?;
    Ok(())
}
