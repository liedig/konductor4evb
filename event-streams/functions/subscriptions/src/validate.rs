use anyhow::{Context, Result};
use std::env;

use konductor_common::dynamodb::{get_s, s};
use konductor_common::models::DomainStatus;

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
        konductor_common::dynamodb::get_string_list(&item, "environments").unwrap_or_default();

    Ok(DomainLookup::Found(DomainRecord {
        owner_account_id: get_s(&item, "ownerAccountId").unwrap_or_default(),
        environments,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_smithy_mocks::{mock, mock_client};
    use std::env;

    #[tokio::test]
    async fn inactive_domain_is_not_usable_for_subscriptions() {
        env::set_var("DOMAINS_TABLE", "domains");
        let rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            GetItemOutput::builder()
                .item("domainName", s("orders"))
                .item("status", s("INACTIVE"))
                .build()
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&rule]);

        match lookup_active_domain(&ddb, "orders").await.unwrap() {
            DomainLookup::NotActive(status) => assert_eq!(status, "INACTIVE"),
            DomainLookup::Found(_) => panic!("INACTIVE domain must not validate as usable"),
            DomainLookup::NotFound => panic!("INACTIVE domain must not look missing"),
        }
    }
}
