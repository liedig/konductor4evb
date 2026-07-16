use anyhow::{Context, Result};
use std::env;

use nexus_common::dynamodb::{get_s, s};
use nexus_common::models::DomainStatus;

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

/// The caller's AWS account must own the consumer domain — subscribing and
/// unsubscribing are consumer-side actions.
pub fn caller_owns_consumer(caller_account_id: &str, consumer: &DomainRecord) -> bool {
    caller_account_id == consumer.owner_account_id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(owner: &str) -> DomainRecord {
        DomainRecord {
            owner_account_id: owner.to_string(),
            environments: vec!["dev".to_string()],
        }
    }

    #[test]
    fn caller_matching_owner_passes() {
        assert!(caller_owns_consumer(
            "111122223333",
            &record("111122223333")
        ));
    }

    #[test]
    fn caller_mismatch_fails() {
        assert!(!caller_owns_consumer(
            "999988887777",
            &record("111122223333")
        ));
    }
}
