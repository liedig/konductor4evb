//! Admission-time payload validation against the latest published schema.
//!
//! The publish admission Lambda validates `PublishRequest.data` against the
//! latest version of the derived schema in the stage-selected environment's
//! registry, then stamps that version onto the envelope. This module holds
//! the three pieces shared by that path:
//!
//! - [`LatestSchema`]: the resolved latest version (content + version number)
//! - [`validate_against_latest`]: JSON Schema draft-4 compilation and payload
//!   validation, with every error classified as either a platform-side
//!   unusable schema (503) or a caller-side conformance failure (422)
//! - [`SchemaCache`]: an in-memory per-(registry, schema) cache that bounds
//!   registry reads to one per TTL per warm instance and supports a forced
//!   refresh before a rejection, so a publish racing a schema append is never
//!   rejected against a definition the caller could not have known
//!
//! The cache is deliberately process-local: stale entries younger than the
//! TTL serve validation, refresh-before-reject bounds the false-rejection
//! window, and a registry outage fails closed as 503 without trusting
//! anything already cached.
//!
//! Registry access stays with the caller: the cache takes an async fetch
//! closure, so this module never owns an SDK client and stays mockable
//! without `aws-smithy-mocks` rule plumbing.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::Value;

/// The latest version of a schema in one registry: the draft-4 document and
/// its version number, as returned by a version-less `DescribeSchema`.
pub struct LatestSchema {
    pub content: String,
    pub version: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaCacheError {
    /// The schema does not exist in the registry. Never cached, so a publish
    /// immediately after schema creation cannot be rejected on a stale
    /// negative.
    #[error("schema not found in registry")]
    NotFound,
    /// The registry read failed (throttle, outage, permissions). The message
    /// is for logs; API responses must not disclose registry internals.
    #[error("registry fetch failed: {0}")]
    RegistryUnavailable(String),
    /// The response carried no schema document.
    #[error("registry response carried no schema content")]
    ContentMissing,
    /// The response version was absent or not a number.
    #[error("registry response carried no usable schemaVersion")]
    VersionUnparseable,
}

/// The outcome of validating a payload against a schema document.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// The stored document does not compile as JSON Schema draft-4 — a
    /// platform-side invariant breach, mapped to 503.
    #[error("schema does not compile as draft-4: {0}")]
    SchemaUnusable(String),
    /// The payload violates the schema — a caller-side conformance failure,
    /// mapped to 422 with the violations in the message. Capped at the first
    /// handful so a badly broken payload cannot produce an unbounded error.
    #[error("payload violates schema: {0}")]
    Violations(String),
}

/// How many violations make it into a `422` message before truncation.
const MAX_REPORTED_VIOLATIONS: usize = 10;

/// The only `$schema` declarations honored. A document declaring any other
/// dialect is rejected outright: draft-4 compilation would silently ignore
/// newer keywords (e.g. `unevaluatedProperties`), accepting payloads the
/// authored schema intends to reject.
const DRAFT4_DECLARATIONS: [&str; 2] = [
    "http://json-schema.org/draft-04/schema#",
    "http://json-schema.org/draft-04/schema",
];

/// Compiles `content` as a JSON Schema draft-4 document.
///
/// Compilation is forced to draft-4 and the document's `$schema` keyword must
/// agree (or be absent): the registry only accepts `JSONSchemaDraft4`
/// documents, so ambiguity is a defect, never a silent interpretation.
fn compile_draft4(content: &str) -> Result<jsonschema::Validator, ValidationError> {
    let schema: Value = serde_json::from_str(content)
        .map_err(|e| ValidationError::SchemaUnusable(format!("not parseable JSON: {e}")))?;
    if !schema.is_object() {
        return Err(ValidationError::SchemaUnusable(
            "schema document must be a JSON object".to_string(),
        ));
    }
    if let Some(declared) = schema.get("$schema").and_then(Value::as_str) {
        if !DRAFT4_DECLARATIONS.contains(&declared) {
            return Err(ValidationError::SchemaUnusable(format!(
                "declares $schema '{declared}'; only JSON Schema draft-4 is supported"
            )));
        }
    }
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft4)
        .build(&schema)
        .map_err(|e| ValidationError::SchemaUnusable(e.to_string()))
}

/// Validates that `content` is a usable JSON Schema draft-4 document — the
/// create/append-time gate, so nothing unvalidatable is ever stored in a
/// registry.
pub fn check_draft4_document(content: &str) -> Result<(), ValidationError> {
    compile_draft4(content).map(|_| ())
}

/// Compiles `latest.content` as JSON Schema draft-4 and validates `data`.
pub fn validate_against_latest(latest: &LatestSchema, data: &Value) -> Result<(), ValidationError> {
    let validator = compile_draft4(&latest.content)?;
    let mut violations = validator
        .iter_errors(data)
        .take(MAX_REPORTED_VIOLATIONS)
        .map(|e| e.to_string());
    let first = violations.next();
    match first {
        None => Ok(()),
        Some(first) => {
            let mut message = first;
            for violation in violations {
                message.push_str("; ");
                message.push_str(&violation);
            }
            Err(ValidationError::Violations(message))
        }
    }
}

#[derive(Debug, Clone)]
struct CachedEntry {
    content: String,
    version: i64,
    fetched_at: Instant,
}

/// Process-local cache of latest-schema lookups, keyed by `(registry, schema
/// name)`. `None` is never cached: an absent schema always hits the registry,
/// so a publish immediately after schema creation cannot be rejected on a
/// stale negative.
pub struct SchemaCache {
    ttl: Duration,
    entries: Mutex<HashMap<(String, String), CachedEntry>>,
}

impl SchemaCache {
    /// Builds a cache with the given TTL for positive entries.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<(String, String), CachedEntry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Returns the cached latest schema when fresh, otherwise fetches via
    /// `fetch` and caches the result.
    pub async fn get_or_fetch<F, Fut>(
        &self,
        registry: &str,
        schema_name: &str,
        fetch: F,
    ) -> Result<LatestSchema, SchemaCacheError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<LatestSchema, SchemaCacheError>>,
    {
        let key = (registry.to_string(), schema_name.to_string());
        if let Some(entry) = self.entries().get(&key) {
            if entry.fetched_at.elapsed() < self.ttl {
                tracing::info!(
                    metric = "SchemaCacheHit",
                    registry,
                    schema_name,
                    "schema cache hit"
                );
                return Ok(LatestSchema {
                    content: entry.content.clone(),
                    version: entry.version,
                });
            }
        }
        tracing::info!(
            metric = "SchemaCacheMiss",
            registry,
            schema_name,
            "schema cache miss"
        );
        let latest = fetch().await?;
        self.entries().insert(
            key,
            CachedEntry {
                content: latest.content.clone(),
                version: latest.version,
                fetched_at: Instant::now(),
            },
        );
        Ok(latest)
    }

    /// Bypasses the TTL: fetches and replaces any cached entry. Used on the
    /// refresh-before-reject path so validation never rejects against a
    /// definition older than the newest the registry can serve.
    pub async fn force_fetch<F, Fut>(
        &self,
        registry: &str,
        schema_name: &str,
        fetch: F,
    ) -> Result<LatestSchema, SchemaCacheError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<LatestSchema, SchemaCacheError>>,
    {
        let key = (registry.to_string(), schema_name.to_string());
        let latest = fetch().await?;
        self.entries().insert(
            key,
            CachedEntry {
                content: latest.content.clone(),
                version: latest.version,
                fetched_at: Instant::now(),
            },
        );
        Ok(latest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SCHEMA: &str = r#"{
        "$schema": "http://json-schema.org/draft-04/schema#",
        "type": "object",
        "properties": {"orderId": {"type": "integer"}},
        "required": ["orderId"]
    }"#;

    fn latest(version: i64) -> LatestSchema {
        LatestSchema {
            content: SCHEMA.to_string(),
            version,
        }
    }

    #[test]
    fn check_draft4_document_accepts_a_valid_document() {
        assert!(check_draft4_document(SCHEMA).is_ok());
    }

    #[test]
    fn check_draft4_document_rejects_malformed_and_non_object_content() {
        assert!(check_draft4_document("not json").is_err());
        assert!(check_draft4_document(r#""a string""#).is_err());
        assert!(check_draft4_document(r#"{"type": "nonsense"}"#).is_err());
    }

    #[test]
    fn check_draft4_document_rejects_newer_dialect_declarations() {
        let declared_2020 = r#"{"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object", "unevaluatedProperties": false}"#;
        let err = check_draft4_document(declared_2020).unwrap_err();
        assert!(
            err.to_string().contains("draft-4"),
            "rejection must name the supported dialect: {err}"
        );
        // No declaration is fine; the draft-04 URI (with or without the
        // fragment) is fine.
        assert!(check_draft4_document(r#"{"type": "object"}"#).is_ok());
        assert!(check_draft4_document(
            r#"{"$schema": "http://json-schema.org/draft-04/schema", "type": "object"}"#
        )
        .is_ok());
    }

    #[test]
    fn conforming_payload_validates() {
        assert!(validate_against_latest(&latest(1), &serde_json::json!({"orderId": 7})).is_ok());
    }

    #[test]
    fn missing_required_field_reports_violations() {
        let err =
            validate_against_latest(&latest(1), &serde_json::json!({"amount": 9.5})).unwrap_err();
        match err {
            ValidationError::Violations(message) => {
                assert!(message.contains("orderId"), "unexpected: {message}")
            }
            other => panic!("expected Violations, got {other:?}"),
        }
    }

    #[test]
    fn wrong_type_reports_violations() {
        let result =
            validate_against_latest(&latest(1), &serde_json::json!({"orderId": "not-an-int"}));
        assert!(matches!(result, Err(ValidationError::Violations(_))));
    }

    #[test]
    fn draft4_exclusive_minimum_semantics_apply() {
        // Draft-4 exclusiveMinimum is a boolean modifier on minimum; draft-6+
        // made it a number. Compiling under draft-4 must apply the draft-4
        // reading, proving with_draft pins semantics.
        let draft4_schema = LatestSchema {
            content: r#"{"type": "integer", "minimum": 5, "exclusiveMinimum": true}"#.to_string(),
            version: 1,
        };
        assert!(validate_against_latest(&draft4_schema, &serde_json::json!(5)).is_err());
        assert!(validate_against_latest(&draft4_schema, &serde_json::json!(6)).is_ok());
    }

    #[test]
    fn unparseable_schema_is_unusable_not_a_violation() {
        let broken = LatestSchema {
            content: "not json".to_string(),
            version: 1,
        };
        assert!(matches!(
            validate_against_latest(&broken, &serde_json::json!({})),
            Err(ValidationError::SchemaUnusable(_))
        ));
    }

    #[test]
    fn non_object_schema_is_unusable() {
        let broken = LatestSchema {
            content: r#""a string""#.to_string(),
            version: 1,
        };
        assert!(matches!(
            validate_against_latest(&broken, &serde_json::json!({})),
            Err(ValidationError::SchemaUnusable(_))
        ));
    }

    fn counter_fetch<'a>(
        calls: &'a AtomicUsize,
        version: i64,
    ) -> impl FnOnce() -> std::future::Ready<Result<LatestSchema, SchemaCacheError>> + 'a {
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(latest(version)))
        }
    }

    #[tokio::test]
    async fn cache_miss_fetches_and_caches() {
        let cache = SchemaCache::new(Duration::from_secs(60));
        let calls = AtomicUsize::new(0);
        let first = cache
            .get_or_fetch("reg", "schema", counter_fetch(&calls, 1))
            .await
            .unwrap();
        let second = cache
            .get_or_fetch("reg", "schema", counter_fetch(&calls, 1))
            .await
            .unwrap();
        assert_eq!(first.version, 1);
        assert_eq!(second.version, 1);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second read must hit cache"
        );
    }

    #[tokio::test]
    async fn expired_entry_refetches() {
        let cache = SchemaCache::new(Duration::ZERO);
        let calls = AtomicUsize::new(0);
        cache
            .get_or_fetch("reg", "schema", counter_fetch(&calls, 1))
            .await
            .unwrap();
        let latest = cache
            .get_or_fetch("reg", "schema", counter_fetch(&calls, 2))
            .await
            .unwrap();
        assert_eq!(latest.version, 2);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "expired entry must refetch"
        );
    }

    #[tokio::test]
    async fn force_fetch_replaces_fresh_entry() {
        let cache = SchemaCache::new(Duration::from_secs(60));
        let calls = AtomicUsize::new(0);
        cache
            .get_or_fetch("reg", "schema", counter_fetch(&calls, 1))
            .await
            .unwrap();
        let latest = cache
            .force_fetch("reg", "schema", counter_fetch(&calls, 2))
            .await
            .unwrap();
        assert_eq!(latest.version, 2, "force fetch bypasses the TTL");

        let cached = cache
            .get_or_fetch("reg", "schema", counter_fetch(&calls, 99))
            .await
            .unwrap();
        assert_eq!(cached.version, 2, "force fetch updates the cached entry");
    }

    #[tokio::test]
    async fn keys_are_scoped_per_registry_and_schema() {
        let cache = SchemaCache::new(Duration::from_secs(60));
        let calls = AtomicUsize::new(0);
        cache
            .get_or_fetch("reg-a", "schema", counter_fetch(&calls, 1))
            .await
            .unwrap();
        cache
            .get_or_fetch("reg-b", "schema", counter_fetch(&calls, 2))
            .await
            .unwrap();
        cache
            .get_or_fetch("reg-a", "other", counter_fetch(&calls, 3))
            .await
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "distinct keys miss independently"
        );
    }
}
