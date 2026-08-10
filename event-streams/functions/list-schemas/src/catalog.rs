//! The subscriber catalog is an entitlement expansion, never a materialized
//! table: every page re-reads the caller's current entitlements (consumer
//! path) or the domains table (platform path), expands scope patterns
//! against the per-environment registries, and merges the union in
//! (producer domain, schema name) order.
//!
//! Pagination is a watermark cursor: the last emitted (producer, schema)
//! pair plus a hash of the filter set. It carries no per-registry vendor
//! tokens and no authorization state, survives gaps and registry changes,
//! and fails with 400 when the filter set changes mid-sequence. Within a
//! registry, listing cannot resume at a key, so later pages re-scan names up
//! to the watermark — bounded by per-domain schema counts.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use nexus_common::entitlement::ScopePattern;

use crate::discovery;

pub const MAX_PAGE_SIZE: usize = 50;

/// Who the catalog is being expanded for: a consumer domain's current
/// entitlements, or the platform-wide view (every ACTIVE domain, unfiltered).
#[derive(Debug)]
pub enum Scope {
    Consumer(String),
    Platform,
}

/// One catalog row: a covered schema with its latest version per environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub producer_domain: String,
    pub schema_name: String,
    pub latest_versions: BTreeMap<String, i64>,
}

/// A filled page plus the watermark the next page continues from.
pub struct CatalogPage {
    pub entries: Vec<CatalogEntry>,
    pub watermark: Option<(String, String)>,
}

/// The opaque continuation token: last emitted (producer, schema) watermark
/// plus the filter-set hash. No vendor tokens, no authorization state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor {
    pub producer: String,
    pub schema: String,
    pub filter_hash: String,
}

impl Cursor {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn decode(token: &str) -> Option<Self> {
        serde_json::from_str(token).ok()
    }
}

/// Binds a cursor to the filter set that issued it; a mismatch is a 400.
pub fn filter_hash(
    consumer: Option<&str>,
    producer: Option<&str>,
    environment: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    for part in [consumer, producer, environment] {
        hasher.update(part.unwrap_or("").as_bytes());
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The parsed catalog request: covered sources in producer order, the
/// environment filter, the page limit (page size plus one look-ahead), and
/// the watermark to continue from.
pub struct CatalogQuery {
    /// Patterns per producer domain (empty on the platform path, where every
    /// schema is covered).
    pub sources: BTreeMap<String, Vec<ScopePattern>>,
    pub environment: Option<String>,
    pub limit: usize,
    pub watermark: Option<(String, String)>,
}

/// Expands one catalog page: covered names strictly after the watermark,
/// latest versions described per environment for page entries only.
pub async fn list_catalog(
    ddb: &aws_sdk_dynamodb::Client,
    schemas: &aws_sdk_schemas::Client,
    organization: &str,
    scope: &Scope,
    query: &CatalogQuery,
) -> Result<CatalogPage> {
    let mut collected: BTreeSet<(String, String)> = BTreeSet::new();
    let mut environments_by_producer: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (producer, patterns) in &query.sources {
        if collected.len() >= query.limit {
            break;
        }
        // Producers before the watermark contribute nothing to this page.
        if let Some((wp, _)) = &query.watermark {
            if producer < wp {
                continue;
            }
        }
        // A producer that is missing or not ACTIVE contributes an empty
        // expansion (the catalog list stays a list, never a 404).
        let Some(domain) = discovery::active_domain(ddb, producer).await? else {
            continue;
        };
        let environments: Vec<String> = match &query.environment {
            Some(env) if domain.environments.contains(env) => vec![env.clone()],
            Some(_) => continue,
            None => domain.environments.clone(),
        };
        for env_name in &environments {
            let registry = format!("{organization}-{env_name}-{producer}-schema-registry");
            for name in list_schema_names(schemas, &registry).await? {
                if !covered(scope, patterns, &name) {
                    continue;
                }
                let key = (producer.clone(), name);
                if let Some((wp, ws)) = &query.watermark {
                    if key <= (wp.clone(), ws.clone()) {
                        continue;
                    }
                }
                collected.insert(key);
                if collected.len() >= query.limit {
                    break;
                }
            }
            if collected.len() >= query.limit {
                break;
            }
        }
        environments_by_producer.insert(producer.clone(), environments);
    }

    // The handler asks for page size + 1; the extra entry is the look-ahead
    // that decides whether a continuation token exists.
    let page_size = query.limit.saturating_sub(1).min(MAX_PAGE_SIZE);
    let has_more = collected.len() > page_size;
    let page: Vec<(String, String)> = collected.iter().take(page_size).cloned().collect();

    let mut entries = Vec::new();
    for (producer, name) in &page {
        let environments = environments_by_producer
            .get(producer)
            .cloned()
            .unwrap_or_default();
        let mut latest_versions = BTreeMap::new();
        for env_name in &environments {
            let registry = format!("{organization}-{env_name}-{producer}-schema-registry");
            if let Some(version) = discovery::latest_version(schemas, &registry, name).await? {
                latest_versions.insert(env_name.clone(), version);
            }
        }
        entries.push(CatalogEntry {
            producer_domain: producer.clone(),
            schema_name: name.clone(),
            latest_versions,
        });
    }

    let watermark = if has_more { page.last().cloned() } else { None };
    Ok(CatalogPage { entries, watermark })
}

fn covered(scope: &Scope, patterns: &[ScopePattern], schema_name: &str) -> bool {
    match scope {
        Scope::Platform => true,
        // Patterns match the bare detail type, as in the engine, the
        // activation workflow, and the revocation worker.
        Scope::Consumer(_) => {
            let detail_type = schema_name
                .split_once('@')
                .map(|(_, dt)| dt)
                .unwrap_or(schema_name);
            patterns.iter().any(|p| p.matches(detail_type))
        }
    }
}

/// All schema names in one registry; a missing registry is an empty list.
async fn list_schema_names(
    schemas: &aws_sdk_schemas::Client,
    registry: &str,
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut next_token: Option<String> = None;
    loop {
        let mut request = schemas.list_schemas().registry_name(registry);
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
        names.extend(
            output
                .schemas
                .unwrap_or_default()
                .into_iter()
                .filter_map(|s| s.schema_name),
        );
        next_token = output.next_token;
        if next_token.is_none() {
            break;
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}
