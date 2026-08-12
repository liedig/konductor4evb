//! Catalog source resolution: which producer domains feed the expansion and
//! under which patterns. The consumer path reads the consumer domain's
//! current entitlements (a strongly consistent query — coverage is a
//! decision read, never a GSI); the platform path enumerates ACTIVE domains
//! from the domains table and covers everything.

use std::collections::BTreeMap;
use std::env;

use anyhow::{Context, Result};

use konductor_common::dynamodb::{get_s, get_string_list};
use konductor_common::entitlement::ScopePattern;
use konductor_common::entitlement_store::EntitlementStore;
use konductor_common::models::DomainStatus;

pub struct CallerSources;

impl CallerSources {
    /// Current entitlements grouped by producer domain, narrowed to the
    /// producer filter when present. Re-read on every page: a revoked grant
    /// must never appear in a later page of the same sequence.
    pub async fn for_consumer(
        ddb: &aws_sdk_dynamodb::Client,
        entitlements_table: &str,
        consumer: &str,
        producer: Option<&str>,
    ) -> Result<BTreeMap<String, Vec<ScopePattern>>> {
        let store = EntitlementStore::new(ddb.clone(), entitlements_table);
        let entitlements = store
            .entitlements_for_consumer(consumer)
            .await
            .map_err(|e| anyhow::anyhow!("entitlement read failed: {e}"))?;
        let mut sources: BTreeMap<String, Vec<ScopePattern>> = BTreeMap::new();
        for entitlement in entitlements {
            if let Some(filter) = producer {
                if entitlement.producer_domain != filter {
                    continue;
                }
            }
            sources
                .entry(entitlement.producer_domain.clone())
                .or_default()
                .push(entitlement.scope);
        }
        Ok(sources)
    }

    /// The producer's own domain as the single source with universal
    /// coverage — publishers inspect their catalog without entitlements.
    /// Entitlement reads are never issued on this path.
    pub fn for_producer(domain: &str) -> BTreeMap<String, Vec<ScopePattern>> {
        BTreeMap::from([(domain.to_string(), Vec::new())])
    }

    /// Every ACTIVE domain as a source with universal coverage. Sized by the
    /// 1,000-domain deployment cap; per-page cost stays at the few registries
    /// needed to fill the page because expansion stops at the page limit.
    pub async fn platform_wide(
        ddb: &aws_sdk_dynamodb::Client,
        producer: Option<&str>,
    ) -> Result<BTreeMap<String, Vec<ScopePattern>>> {
        let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
        let mut sources: BTreeMap<String, Vec<ScopePattern>> = BTreeMap::new();
        let mut exclusive_start_key = None;
        loop {
            let output = ddb
                .scan()
                .table_name(&table)
                .set_exclusive_start_key(exclusive_start_key)
                .send()
                .await
                .context("failed to scan domains table")?;
            for item in output.items.unwrap_or_default() {
                let active = get_s(&item, "status")
                    .map(|s| s.parse::<DomainStatus>() == Ok(DomainStatus::Active))
                    .unwrap_or(false);
                let has_envs = !get_string_list(&item, "environments")
                    .unwrap_or_default()
                    .is_empty();
                if !active || !has_envs {
                    continue;
                }
                let Some(name) = get_s(&item, "domainName") else {
                    continue;
                };
                if let Some(filter) = producer {
                    if name != filter {
                        continue;
                    }
                }
                sources.insert(name, Vec::new());
            }
            exclusive_start_key = output.last_evaluated_key;
            if exclusive_start_key.is_none() {
                break;
            }
        }
        Ok(sources)
    }
}
