//! Authoritative metadata selection for admission: the strongly consistent
//! environment-pointer read that happens only after authorization, plus the
//! bounded cache of immutable revision records.
//!
//! Revisions are immutable, so cached entries never invalidate — the cache
//! is only bounded, never refreshed. The pointer is never cached: admission
//! re-reads it on every publish so an activation takes effect on the next
//! request, and a missing pointer is configuration unavailability (503),
//! never a stale-pointer fallback.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::metadata::revisions::{MetadataStore, MetadataStoreError, RevisionRecord};

/// Default bound on cached revision records. Revisions are immutable and
/// pointer reads select among a small recent window, so a modest bound
/// covers the hot set; the smallest revision evicts first.
pub const DEFAULT_REVISION_CACHE_CAPACITY: usize = 32;

/// The environment's authoritative metadata selection: the pointer state
/// plus the immutable revision it names. `revision` is shared through the
/// cache, so copies are cheap.
pub struct MetadataSelection {
    pub environment: String,
    pub active_revision: u32,
    pub generation: u64,
    pub revision: Arc<RevisionRecord>,
}

/// A bounded cache of immutable revision records. Entries are never
/// invalidated (revisions cannot change); the bound only caps memory.
pub struct RevisionCache {
    capacity: usize,
    entries: Mutex<BTreeMap<u32, Arc<RevisionRecord>>>,
}

impl RevisionCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    /// Returns the revision from cache when present, otherwise loads it
    /// with a strongly consistent read. A pointer naming a revision that
    /// does not exist is corrupt configuration, not a miss to retry.
    pub async fn get_or_load(
        &self,
        store: &MetadataStore,
        revision: u32,
    ) -> Result<Arc<RevisionRecord>, MetadataStoreError> {
        if let Some(hit) = locked(&self.entries).get(&revision) {
            return Ok(hit.clone());
        }
        let record = store.get_revision(revision).await?.ok_or_else(|| {
            MetadataStoreError::CorruptItem(format!(
                "pointer names revision {revision}, which does not exist"
            ))
        })?;
        let record = Arc::new(record);
        let mut entries = locked(&self.entries);
        entries.insert(revision, record.clone());
        while entries.len() > self.capacity {
            entries.pop_first();
        }
        Ok(record)
    }
}

/// The admission-facing metadata resolver: one strongly consistent pointer
/// read per publish, revisions through the bounded immutable cache.
pub struct MetadataAdmission {
    store: MetadataStore,
    revisions: RevisionCache,
}

impl MetadataAdmission {
    pub fn new(ddb: aws_sdk_dynamodb::Client, table: impl Into<String>) -> Self {
        Self {
            store: MetadataStore::new(ddb, table),
            revisions: RevisionCache::new(DEFAULT_REVISION_CACHE_CAPACITY),
        }
    }

    /// Resolves the environment's active metadata selection.
    ///
    /// `Ok(None)` is the legacy baseline: the pointer names revision 0 and
    /// publishes carry no platform metadata. Every `Err` is configuration
    /// unavailability — a missing pointer, a corrupt item, or a store
    /// failure — and admission must 503 rather than fall back to any stale
    /// selection.
    pub async fn resolve(
        &self,
        environment: &str,
    ) -> Result<Option<MetadataSelection>, MetadataStoreError> {
        let pointer = self
            .store
            .get_environment_pointer(environment)
            .await?
            .ok_or_else(|| {
                MetadataStoreError::Unavailable(format!(
                    "environment '{environment}' has no metadata pointer"
                ))
            })?;
        if pointer.active_revision == 0 {
            return Ok(None);
        }
        let revision = self
            .revisions
            .get_or_load(&self.store, pointer.active_revision)
            .await?;
        Ok(Some(MetadataSelection {
            environment: pointer.environment,
            active_revision: pointer.active_revision,
            generation: pointer.generation,
            revision,
        }))
    }
}

fn locked(
    entries: &Mutex<BTreeMap<u32, Arc<RevisionRecord>>>,
) -> MutexGuard<'_, BTreeMap<u32, Arc<RevisionRecord>>> {
    entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamodb::{n, s};
    use crate::metadata::store::{revision_sort_key, ENVIRONMENT_PARTITION, REVISION_PARTITION};
    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};

    fn pointer_item(revision: i64, generation: i64) -> GetItemOutput {
        GetItemOutput::builder()
            .item("PK", s(ENVIRONMENT_PARTITION))
            .item("SK", s("prod"))
            .item("activeRevision", n(revision))
            .item("generation", n(generation))
            .item("definitionDigest", s("digest-2"))
            .item("activatedBy", s("admin-sub"))
            .item("activatedAt", s("2026-01-01T00:00:00.000Z"))
            .build()
    }

    fn revision_item(revision: i64) -> GetItemOutput {
        GetItemOutput::builder()
            .item("PK", s(REVISION_PARTITION))
            .item("SK", s(&revision_sort_key(revision as u32)))
            .item("revision", n(revision))
            .item("predecessor", n(revision - 1))
            .item("definition", s("{}"))
            .item("metadataSchema", s("{}"))
            .item("definitionDigest", s("digest"))
            .item("compilerFormat", n(1))
            .item("createdBy", s("admin-sub"))
            .item("createdAt", s("2026-01-01T00:00:00.000Z"))
            .item("operationId", s("op"))
            .build()
    }

    /// A pointer rule matching the environment partition with the exact
    /// strongly consistent read admission performs.
    fn pointer_rule(revision: i64, generation: i64) -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.table_name() == Some("metadata")
                    && req.consistent_read() == Some(true)
                    && req.key().and_then(|k| k.get("PK")) == Some(&s(ENVIRONMENT_PARTITION))
            })
            .then_output(move || pointer_item(revision, generation))
    }

    /// A revision rule matching one revision's sort key, so concurrent
    /// revision reads stay distinguishable.
    fn revision_rule(revision: i64) -> Rule {
        let sk = revision_sort_key(revision as u32);
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(move |req| {
                req.table_name() == Some("metadata")
                    && req.key().and_then(|k| k.get("PK")) == Some(&s(REVISION_PARTITION))
                    && req.key().and_then(|k| k.get("SK")) == Some(&s(&sk))
            })
            .then_output(move || revision_item(revision))
    }

    fn admission_with(rules: &[&Rule]) -> MetadataAdmission {
        MetadataAdmission {
            store: MetadataStore::new(
                mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, rules),
                "metadata",
            ),
            revisions: RevisionCache::new(DEFAULT_REVISION_CACHE_CAPACITY),
        }
    }

    #[tokio::test]
    async fn resolve_returns_the_pointer_state_and_revision_record() {
        let pointer = pointer_rule(2, 1);
        let revision = revision_rule(2);

        let admission = admission_with(&[&pointer, &revision]);
        let selection = admission
            .resolve("prod")
            .await
            .unwrap()
            .expect("active selection");

        assert_eq!(selection.environment, "prod");
        assert_eq!(selection.active_revision, 2);
        assert_eq!(selection.generation, 1);
        assert_eq!(selection.revision.revision, 2);
        assert_eq!(selection.revision.definition_digest, "digest");
    }

    #[tokio::test]
    async fn a_legacy_pointer_resolves_to_none_without_reading_a_revision() {
        let pointer = pointer_rule(0, 0);

        let admission = admission_with(&[&pointer]);
        let selection = admission.resolve("prod").await.unwrap();

        assert!(
            selection.is_none(),
            "revision 0 is the legacy baseline; no revision read happens"
        );
        assert_eq!(pointer.num_calls(), 1);
    }

    #[tokio::test]
    async fn a_missing_pointer_is_unavailability_never_a_fallback() {
        let missing = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.table_name() == Some("metadata"))
            .then_output(|| GetItemOutput::builder().build());

        let admission = admission_with(&[&missing]);
        let result = admission.resolve("prod").await;

        assert!(
            matches!(result, Err(MetadataStoreError::Unavailable(_))),
            "a missing pointer must 503, not fall back"
        );
    }

    #[tokio::test]
    async fn a_pointer_naming_a_missing_revision_is_corrupt_configuration() {
        let pointer = pointer_rule(5, 3);
        let missing_revision = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.key().and_then(|k| k.get("PK")) == Some(&s(REVISION_PARTITION))
            })
            .then_output(|| GetItemOutput::builder().build());

        let admission = admission_with(&[&pointer, &missing_revision]);
        let result = admission.resolve("prod").await;

        assert!(
            matches!(result, Err(MetadataStoreError::CorruptItem(_))),
            "a pointer naming a nonexistent revision is corrupt configuration"
        );
    }

    #[tokio::test]
    async fn a_cached_revision_is_served_without_a_second_read() {
        let revision = revision_rule(2);
        let store = MetadataStore::new(
            mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&revision]),
            "metadata",
        );
        let cache = RevisionCache::new(DEFAULT_REVISION_CACHE_CAPACITY);

        let first = cache.get_or_load(&store, 2).await.unwrap();
        let second = cache.get_or_load(&store, 2).await.unwrap();

        assert_eq!(first.revision, second.revision);
        assert_eq!(revision.num_calls(), 1, "the second read is a cache hit");
    }

    #[tokio::test]
    async fn the_cache_evicts_the_oldest_revision_at_capacity() {
        let rev1 = revision_rule(1);
        let rev2 = revision_rule(2);
        let rev3 = revision_rule(3);
        let store = MetadataStore::new(
            mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&rev1, &rev2, &rev3]),
            "metadata",
        );
        let cache = RevisionCache::new(2);

        cache.get_or_load(&store, 1).await.unwrap();
        cache.get_or_load(&store, 2).await.unwrap();
        cache.get_or_load(&store, 3).await.unwrap();
        cache.get_or_load(&store, 2).await.unwrap();
        cache.get_or_load(&store, 1).await.unwrap();

        assert_eq!(rev1.num_calls(), 2, "revision 1 evicted then reloaded");
        assert_eq!(rev2.num_calls(), 1, "revision 2 stayed cached");
        assert_eq!(rev3.num_calls(), 1);
    }
}
