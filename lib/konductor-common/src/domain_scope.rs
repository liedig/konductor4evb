//! Caller-visibility resolution for domain reads. IAM automation and
//! platform roles see the full table; OAuth team callers see only the
//! domains their team groups own, resolved through owner bindings.
//!
//! Used by the domain list/describe functions and the schema catalog
//! handlers so every management read applies the same ownership scope.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

use crate::dynamodb::s;
use crate::principal::{GlobalRole, TrustedPrincipal};

const OWNER_SK_PREFIX: &str = "ROLE#owner#DOMAIN#";

/// What the caller may see in list/describe reads.
pub enum Visibility {
    /// Unscoped reads (IAM automation and platform roles).
    Full,
    /// Domains owned by the caller's team groups, sorted by name.
    Owned(Vec<String>),
}

impl Visibility {
    pub fn permits(&self, domain: &str) -> bool {
        match self {
            Self::Full => true,
            Self::Owned(domains) => domains.iter().any(|d| d == domain),
        }
    }
}

/// IAM callers and platform roles (admin, auditor) read unscoped; any other
/// OAuth caller resolves to the owner-bindings scope.
pub fn is_full_visibility(principal: &TrustedPrincipal) -> bool {
    match principal {
        TrustedPrincipal::Iam(_) => true,
        TrustedPrincipal::OAuth(_) => {
            principal.is_platform_admin()
                || principal
                    .global_roles()
                    .contains(&GlobalRole::PlatformAuditor)
        }
    }
}

/// The domains owned by the caller's team groups, via strongly consistent
/// owner-binding reads (authorization-adjacent; never the admin GSI).
pub async fn owned_domain_names(
    ddb: &aws_sdk_dynamodb::Client,
    bindings_table: &str,
    principal: &TrustedPrincipal,
) -> Result<Vec<String>> {
    let mut owned = BTreeSet::new();
    for pk in owner_partition_keys(principal) {
        let mut exclusive_start_key = None;
        loop {
            let mut query = ddb
                .query()
                .table_name(bindings_table)
                .key_condition_expression("PK = :pk AND begins_with(SK, :prefix)")
                .expression_attribute_values(":pk", s(&pk))
                .expression_attribute_values(":prefix", s(OWNER_SK_PREFIX))
                .consistent_read(true);
            if let Some(key) = exclusive_start_key.take() {
                query = query.set_exclusive_start_key(Some(key));
            }
            let page = query.send().await.context("owner bindings query failed")?;
            for item in page.items() {
                if let Some(sk) = item.get("SK").and_then(|v| v.as_s().ok()) {
                    if let Some(domain) = sk.strip_prefix(OWNER_SK_PREFIX) {
                        owned.insert(domain.to_string());
                    }
                }
            }
            match page.last_evaluated_key() {
                Some(key) if !key.is_empty() => {
                    exclusive_start_key = Some(key.clone());
                }
                _ => break,
            }
        }
    }
    Ok(owned.into_iter().collect())
}

/// `PRINCIPAL#...` partition keys for every issuer-qualified team group the
/// caller can claim bindings through.
fn owner_partition_keys(principal: &TrustedPrincipal) -> Vec<String> {
    match principal {
        TrustedPrincipal::Iam(_) => Vec::new(),
        TrustedPrincipal::OAuth(p) => p
            .team_groups
            .iter()
            .map(|g| format!("PRINCIPAL#cognito-group#{}#{g}", p.issuer))
            .collect(),
    }
}

/// Fetches domain records by name in batches; preserves no ordering (the
/// caller sorts). Names beyond the batch window are chunked.
pub async fn fetch_domains_by_name(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    names: &[String],
) -> Result<Vec<HashMap<String, AttributeValue>>> {
    let mut items = Vec::new();
    for chunk in names.chunks(100) {
        let keys: Vec<HashMap<String, AttributeValue>> = chunk
            .iter()
            .map(|name| HashMap::from([("domainName".to_string(), s(name))]))
            .collect();
        let response = ddb
            .batch_get_item()
            .request_items(
                table,
                aws_sdk_dynamodb::types::KeysAndAttributes::builder()
                    .set_keys(Some(keys))
                    .consistent_read(true)
                    .build()
                    .map_err(|e| anyhow::anyhow!("invalid batch keys: {e}"))?,
            )
            .send()
            .await
            .context("domain batch read failed")?;
        if let Some(responses) = response.responses() {
            items.extend(responses.get(table).cloned().unwrap_or_default());
        }
    }
    Ok(items)
}
