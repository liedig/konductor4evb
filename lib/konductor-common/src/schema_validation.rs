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
//! - [`SchemaCache`]: an in-memory per-(registry, schema, metadata
//!   generation) cache that bounds registry reads to one per TTL per warm
//!   instance and supports a forced refresh before a rejection, so a
//!   publish racing a schema append is never rejected against a definition
//!   the caller could not have known
//! - [`ValidatorCache`]: a bounded cache of compiled draft-4 validators
//!   keyed by the exact document content — the component's identity — so
//!   warm publishes skip recompilation and no compiled contract is ever
//!   shared across different documents
//!
//! Entries are keyed by the environment pointer's metadata generation: an
//! activation bumps the generation, so a warm instance's cached entries for
//! the previous generation become unreachable immediately (and are evicted
//! when the new generation's entry lands). A failed registry read inserts
//! nothing, so it can never stamp an expired contract under a live key.
//!
//! The cache is deliberately process-local: stale entries younger than the
//! TTL serve validation, refresh-before-reject bounds the false-rejection
//! window, and a registry outage fails closed as 503 without trusting
//! anything already cached.
//!
//! Registry access stays with the caller: the cache takes an async fetch
//! closure, so this module never owns an SDK client and stays mockable
//! without `aws-smithy-mocks` rule plumbing.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
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

/// The only `$schema` declarations honored, in both spellings: generators
/// emit the draft-04 meta-schema over `https` as readily as `http`, and the
/// scheme does not change the dialect. A document declaring any other dialect
/// is rejected outright: draft-4 compilation would silently ignore newer
/// keywords (e.g. `unevaluatedProperties`), accepting payloads the authored
/// schema intends to reject.
const DRAFT4_DECLARATIONS: [&str; 4] = [
    "http://json-schema.org/draft-04/schema#",
    "http://json-schema.org/draft-04/schema",
    "https://json-schema.org/draft-04/schema#",
    "https://json-schema.org/draft-04/schema",
];

/// Compiles `content` as a JSON Schema draft-4 document.
///
/// Compilation is forced to draft-4 and the document's `$schema` keyword must
/// agree (or be absent): the registry only accepts `JSONSchemaDraft4`
/// documents, so ambiguity is a defect, never a silent interpretation.
fn compile_draft4(content: &str) -> Result<jsonschema::Validator, ValidationError> {
    build_draft4(&parse_draft4(content)?)
}

fn parse_draft4(content: &str) -> Result<Value, ValidationError> {
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
    Ok(schema)
}

fn build_draft4(schema: &Value) -> Result<jsonschema::Validator, ValidationError> {
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft4)
        .build(schema)
        .map_err(|e| ValidationError::SchemaUnusable(e.to_string()))
}

/// The largest `multipleOf` divisor the validator decides exactly
/// (2^53 - 1). Above it, divisibility runs through `f64` and both accepts
/// non-multiples and rejects true multiples.
const MAX_EXACT_MULTIPLE_OF: u64 = 9_007_199_254_740_991;

/// Keywords whose values are instances, not subschemas: a `multipleOf`
/// inside them is data and never constrains validation.
const INSTANCE_KEYWORDS: [&str; 2] = ["enum", "default"];

/// Keywords whose values map names to subschemas: the names are data, so a
/// property called `multipleOf` or `default` is still walked as a schema.
const SCHEMA_MAP_KEYWORDS: [&str; 4] = [
    "properties",
    "patternProperties",
    "definitions",
    "dependencies",
];

/// Validates that `content` is a usable JSON Schema draft-4 document — the
/// create/append-time gate, so nothing unvalidatable is ever stored in a
/// registry. Beyond compiling, the gate refuses `multipleOf` divisors above
/// 2^53 - 1, which the validator cannot decide exactly. Publish-time
/// compilation deliberately skips this check so documents stored before it
/// existed keep validating instead of failing as unusable.
pub fn check_draft4_document(content: &str) -> Result<(), ValidationError> {
    let schema = parse_draft4(content)?;
    build_draft4(&schema)?;
    reject_inexact_multiple_of(&schema, "#")
}

/// Walks every position that can hold a subschema, including unknown
/// keywords (a `$ref` can target them), and rejects the first `multipleOf`
/// divisor the validator cannot decide exactly.
fn reject_inexact_multiple_of(schema: &Value, path: &str) -> Result<(), ValidationError> {
    match schema {
        Value::Object(keywords) => {
            for (keyword, value) in keywords {
                if INSTANCE_KEYWORDS.contains(&keyword.as_str()) {
                    continue;
                }
                if keyword == "multipleOf" {
                    if let Value::Number(divisor) = value {
                        if exceeds_exact_multiple_of(divisor) {
                            return Err(ValidationError::SchemaUnusable(format!(
                                "multipleOf at '{path}' is {divisor}, above \
                                 {MAX_EXACT_MULTIPLE_OF}, the largest divisor validated exactly"
                            )));
                        }
                    }
                    continue;
                }
                let child = format!("{path}/{}", escape_pointer_token(keyword));
                match value {
                    Value::Object(entries) if SCHEMA_MAP_KEYWORDS.contains(&keyword.as_str()) => {
                        for (name, subschema) in entries {
                            let entry = format!("{child}/{}", escape_pointer_token(name));
                            reject_inexact_multiple_of(subschema, &entry)?;
                        }
                    }
                    _ => reject_inexact_multiple_of(value, &child)?,
                }
            }
            Ok(())
        }
        Value::Array(items) => items.iter().enumerate().try_for_each(|(index, item)| {
            reject_inexact_multiple_of(item, &format!("{path}/{index}"))
        }),
        _ => Ok(()),
    }
}

fn exceeds_exact_multiple_of(divisor: &serde_json::Number) -> bool {
    match (divisor.as_u64(), divisor.as_i64(), divisor.as_f64()) {
        (Some(unsigned), _, _) => unsigned > MAX_EXACT_MULTIPLE_OF,
        (None, Some(_), _) => false,
        (None, None, Some(float)) => float > MAX_EXACT_MULTIPLE_OF as f64,
        (None, None, None) => true,
    }
}

fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Compiles `latest.content` as JSON Schema draft-4 and validates `data`.
pub fn validate_against_latest(latest: &LatestSchema, data: &Value) -> Result<(), ValidationError> {
    let validator = compile_draft4(&latest.content)?;
    collect_violations(&validator, data)
}

/// Runs a compiled validator over the payload, folding the first handful of
/// violations into one 422 message.
fn collect_violations(
    validator: &jsonschema::Validator,
    data: &Value,
) -> Result<(), ValidationError> {
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
/// name, metadata generation)`. `None` is never cached: an absent schema
/// always hits the registry, so a publish immediately after schema creation
/// cannot be rejected on a stale negative. Inserting an entry for a newer
/// metadata generation evicts the same schema's older-generation entries, so
/// a warm instance holds at most one generation per schema and an activation
/// takes effect on the very next publish.
pub struct SchemaCache {
    ttl: Duration,
    entries: Mutex<HashMap<(String, String, u64), CachedEntry>>,
}

impl SchemaCache {
    /// Builds a cache with the given TTL for positive entries.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<(String, String, u64), CachedEntry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Stores an entry under its generation key, evicting the same
    /// `(registry, schema)` entries from older generations.
    fn store(
        entries: &mut HashMap<(String, String, u64), CachedEntry>,
        key: (String, String, u64),
        latest: &LatestSchema,
    ) {
        entries.retain(|(registry, schema, _), _| registry != &key.0 || schema != &key.1);
        entries.insert(
            key,
            CachedEntry {
                content: latest.content.clone(),
                version: latest.version,
                fetched_at: Instant::now(),
            },
        );
    }

    /// Returns the cached latest schema when fresh, otherwise fetches via
    /// `fetch` and caches the result.
    pub async fn get_or_fetch<F, Fut>(
        &self,
        registry: &str,
        schema_name: &str,
        generation: u64,
        fetch: F,
    ) -> Result<LatestSchema, SchemaCacheError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<LatestSchema, SchemaCacheError>>,
    {
        let key = (registry.to_string(), schema_name.to_string(), generation);
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
        let mut entries = self.entries();
        Self::store(&mut entries, key, &latest);
        Ok(latest)
    }

    /// Bypasses the TTL: fetches and replaces any cached entry. Used on the
    /// refresh-before-reject path so validation never rejects against a
    /// definition older than the newest the registry can serve.
    pub async fn force_fetch<F, Fut>(
        &self,
        registry: &str,
        schema_name: &str,
        generation: u64,
        fetch: F,
    ) -> Result<LatestSchema, SchemaCacheError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<LatestSchema, SchemaCacheError>>,
    {
        let key = (registry.to_string(), schema_name.to_string(), generation);
        let latest = fetch().await?;
        let mut entries = self.entries();
        Self::store(&mut entries, key, &latest);
        Ok(latest)
    }
}

/// Default bound on compiled validators. Warm instances validate against a
/// small recent window of schema documents, one per schema per generation.
pub const DEFAULT_VALIDATOR_CACHE_CAPACITY: usize = 64;

/// Bounded process-local cache of compiled draft-4 validators, keyed by the
/// exact document content — the component's identity. Compilation is pure in
/// the content, so entries never invalidate; the bound only caps memory, and
/// two different documents can never share a compiled contract.
pub struct ValidatorCache {
    capacity: usize,
    entries: Mutex<BTreeMap<String, Arc<jsonschema::Validator>>>,
}

impl ValidatorCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    fn entries(&self) -> MutexGuard<'_, BTreeMap<String, Arc<jsonschema::Validator>>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Returns the compiled validator for `content`, compiling at most once
    /// per distinct document. A document that does not compile is never
    /// cached: the defect reports on every request until it is fixed.
    pub fn get_or_compile(
        &self,
        content: &str,
    ) -> Result<Arc<jsonschema::Validator>, ValidationError> {
        if let Some(hit) = self.entries().get(content) {
            return Ok(hit.clone());
        }
        let validator = Arc::new(compile_draft4(content)?);
        let mut entries = self.entries();
        entries.insert(content.to_string(), validator.clone());
        while entries.len() > self.capacity {
            entries.pop_first();
        }
        Ok(validator)
    }

    /// Validates `data` against `content` through the compiled-validator
    /// cache. Identical to [`validate_against_latest`] in outcome; warm
    /// documents skip recompilation.
    pub fn validate(&self, content: &str, data: &Value) -> Result<(), ValidationError> {
        let validator = self.get_or_compile(content)?;
        collect_violations(&validator, data)
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
        // No declaration is fine; the draft-04 URI is fine over either
        // scheme, with or without the fragment.
        assert!(check_draft4_document(r#"{"type": "object"}"#).is_ok());
        assert!(check_draft4_document(
            r#"{"$schema": "http://json-schema.org/draft-04/schema", "type": "object"}"#
        )
        .is_ok());
        assert!(check_draft4_document(
            r#"{"$schema": "https://json-schema.org/draft-04/schema#", "type": "object"}"#
        )
        .is_ok());
        assert!(check_draft4_document(
            r#"{"$schema": "https://json-schema.org/draft-04/schema", "type": "object"}"#
        )
        .is_ok());
    }

    #[test]
    fn integer_multiple_of_divisors_above_the_exact_bound_are_rejected_wherever_they_appear() {
        let cases = [
            (r#"{"multipleOf": 9007199254740992}"#, "#"),
            (
                r#"{"properties": {"ledger": {"properties": {"units": {"multipleOf": 9007199254740993}}}}}"#,
                "#/properties/ledger/properties/units",
            ),
            (
                r#"{"properties": {"default": {"multipleOf": 1e20}}}"#,
                "#/properties/default",
            ),
            (r#"{"items": {"multipleOf": 1e20}}"#, "#/items"),
            (
                r#"{"items": [{"type": "integer"}, {"multipleOf": 18446744073709551615}]}"#,
                "#/items/1",
            ),
            (
                r#"{"additionalItems": {"multipleOf": 1e20}}"#,
                "#/additionalItems",
            ),
            (
                r#"{"additionalProperties": {"multipleOf": 1e20}}"#,
                "#/additionalProperties",
            ),
            (
                r#"{"patternProperties": {"^a/b$": {"multipleOf": 1e20}}}"#,
                "#/patternProperties/^a~1b$",
            ),
            (
                r#"{"definitions": {"big": {"multipleOf": 1e20}}}"#,
                "#/definitions/big",
            ),
            (
                r#"{"dependencies": {"a": {"properties": {"b": {"multipleOf": 1e20}}}}}"#,
                "#/dependencies/a/properties/b",
            ),
            (
                r#"{"allOf": [{"not": {"multipleOf": 1e20}}]}"#,
                "#/allOf/0/not",
            ),
            (r#"{"anyOf": [{}, {"multipleOf": 1e20}]}"#, "#/anyOf/1"),
            (r#"{"oneOf": [{"multipleOf": 1e20}]}"#, "#/oneOf/0"),
            (r#"{"x-vendor": {"multipleOf": 1e20}}"#, "#/x-vendor"),
        ];
        for (content, path) in cases {
            let err = check_draft4_document(content).expect_err(content);
            let ValidationError::SchemaUnusable(message) = &err else {
                panic!("expected SchemaUnusable for {content}, got {err:?}");
            };
            assert!(
                message.contains("multipleOf")
                    && message.contains(&format!("'{path}'"))
                    && message.contains("9007199254740991"),
                "rejection must name the keyword, its location, and the bound for {content}: {message}"
            );
        }
    }

    #[test]
    fn exact_multiple_of_divisors_and_non_schema_values_are_not_rejected() {
        let accepted = [
            r#"{"multipleOf": 9007199254740991}"#,
            r#"{"multipleOf": 0.5}"#,
            r#"{"enum": [{"multipleOf": 1e20}], "default": {"multipleOf": 1e20}}"#,
            r#"{"properties": {"multipleOf": {"type": "integer", "multipleOf": 3}}, "required": ["multipleOf"]}"#,
        ];
        for content in accepted {
            assert!(
                check_draft4_document(content).is_ok(),
                "{content} must stay accepted"
            );
        }
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
    fn draft4_format_keywords_are_asserted() {
        let timestamped = LatestSchema {
            content: r#"{"type": "string", "format": "date-time"}"#.to_string(),
            version: 1,
        };
        assert!(validate_against_latest(
            &timestamped,
            &serde_json::json!("2026-07-30T10:30:00.123Z")
        )
        .is_ok());
        assert!(matches!(
            validate_against_latest(&timestamped, &serde_json::json!("not-a-date")),
            Err(ValidationError::Violations(_))
        ));
    }

    #[test]
    fn a_remote_reference_is_unusable_and_never_fetched() {
        let remote = LatestSchema {
            content: r#"{"$ref": "https://example.com/schema.json"}"#.to_string(),
            version: 1,
        };
        assert!(matches!(
            validate_against_latest(&remote, &serde_json::json!(1)),
            Err(ValidationError::SchemaUnusable(_))
        ));
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
            .get_or_fetch("reg", "schema", 0, counter_fetch(&calls, 1))
            .await
            .unwrap();
        let second = cache
            .get_or_fetch("reg", "schema", 0, counter_fetch(&calls, 1))
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
            .get_or_fetch("reg", "schema", 0, counter_fetch(&calls, 1))
            .await
            .unwrap();
        let latest = cache
            .get_or_fetch("reg", "schema", 0, counter_fetch(&calls, 2))
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
            .get_or_fetch("reg", "schema", 0, counter_fetch(&calls, 1))
            .await
            .unwrap();
        let latest = cache
            .force_fetch("reg", "schema", 0, counter_fetch(&calls, 2))
            .await
            .unwrap();
        assert_eq!(latest.version, 2, "force fetch bypasses the TTL");

        let cached = cache
            .get_or_fetch("reg", "schema", 0, counter_fetch(&calls, 99))
            .await
            .unwrap();
        assert_eq!(cached.version, 2, "force fetch updates the cached entry");
    }

    #[tokio::test]
    async fn keys_are_scoped_per_registry_and_schema() {
        let cache = SchemaCache::new(Duration::from_secs(60));
        let calls = AtomicUsize::new(0);
        cache
            .get_or_fetch("reg-a", "schema", 0, counter_fetch(&calls, 1))
            .await
            .unwrap();
        cache
            .get_or_fetch("reg-b", "schema", 0, counter_fetch(&calls, 2))
            .await
            .unwrap();
        cache
            .get_or_fetch("reg-a", "other", 0, counter_fetch(&calls, 3))
            .await
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "distinct keys miss independently"
        );
    }

    #[tokio::test]
    async fn a_generation_bump_makes_older_entries_unusable() {
        let calls = Arc::new(AtomicUsize::new(0));
        let cache = SchemaCache::new(Duration::from_secs(60));

        cache
            .get_or_fetch("reg", "schema", 1, counter_fetch(&calls, 1))
            .await
            .unwrap();
        cache
            .get_or_fetch("reg", "schema", 1, counter_fetch(&calls, 1))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "same generation is a hit");

        // M1 -> M2 activation: the new generation fetches fresh, evicting the
        // old entry, so even a request that re-observes the old generation
        // can never be served the stale contract.
        cache
            .get_or_fetch("reg", "schema", 2, counter_fetch(&calls, 2))
            .await
            .unwrap();
        cache
            .get_or_fetch("reg", "schema", 1, counter_fetch(&calls, 3))
            .await
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "old-generation entries were evicted"
        );
    }

    #[tokio::test]
    async fn a_failed_registry_read_never_stamps_a_live_key() {
        let calls = Arc::new(AtomicUsize::new(0));
        let cache = SchemaCache::new(Duration::from_secs(60));

        cache
            .get_or_fetch("reg", "schema", 1, counter_fetch(&calls, 1))
            .await
            .unwrap();

        let failing = || async { Err(SchemaCacheError::RegistryUnavailable("outage".to_string())) };
        assert!(cache
            .force_fetch("reg", "schema", 2, failing)
            .await
            .is_err());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the failed refresh inserted nothing under the new generation"
        );

        cache
            .get_or_fetch("reg", "schema", 2, counter_fetch(&calls, 2))
            .await
            .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the failed refresh left no expired contract to serve"
        );
    }

    #[test]
    fn compiled_validators_are_shared_per_exact_document() {
        let cache = ValidatorCache::new(DEFAULT_VALIDATOR_CACHE_CAPACITY);
        let first = cache.get_or_compile(SCHEMA).unwrap();
        let second = cache.get_or_compile(SCHEMA).unwrap();
        assert!(
            std::ptr::eq(Arc::as_ptr(&first), Arc::as_ptr(&second)),
            "the same document compiles once and is shared"
        );
        assert!(
            cache
                .validate(SCHEMA, &serde_json::json!({"orderId": 7}))
                .is_ok(),
            "cached validation keeps the same outcome as direct compilation"
        );
    }

    #[test]
    fn the_validator_cache_is_bounded_and_documents_never_share() {
        let cache = ValidatorCache::new(2);
        let first = cache.get_or_compile(SCHEMA).unwrap();
        cache.get_or_compile(r#"{"type": "object"}"#).unwrap();
        cache.get_or_compile(r#"{"type": "array"}"#).unwrap();

        let recompiled = cache.get_or_compile(SCHEMA).unwrap();
        assert!(
            !std::ptr::eq(Arc::as_ptr(&first), Arc::as_ptr(&recompiled)),
            "the evicted document compiles again rather than sharing a contract"
        );
    }
}
