use anyhow::{Context, Result};
use std::env;

use nexus_common::dynamodb::{get_s, s};
use nexus_common::models::DomainStatus;

#[derive(Debug)]
pub struct DomainRecord {
    pub owner_account_id: String,
    pub environments: Vec<String>,
}

pub enum DomainLookup {
    Found(DomainRecord),
    NotFound,
    NotActive(String),
}

pub async fn lookup_active_domain(
    ddb: &aws_sdk_dynamodb::Client,
    domain_name: &str,
) -> Result<DomainLookup> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let output = ddb
        .get_item()
        .table_name(&table)
        .key("domainName", s(domain_name))
        .send()
        .await
        .context("failed to get domain record")?;

    let Some(item) = output.item else {
        return Ok(DomainLookup::NotFound);
    };

    let status = get_s(&item, "status").unwrap_or_default();
    if status.parse::<DomainStatus>() != Ok(DomainStatus::Active) {
        return Ok(DomainLookup::NotActive(status));
    }

    let environments =
        nexus_common::dynamodb::get_string_list(&item, "environments").unwrap_or_default();

    Ok(DomainLookup::Found(DomainRecord {
        owner_account_id: get_s(&item, "ownerAccountId").unwrap_or_default(),
        environments,
    }))
}
