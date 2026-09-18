//! Composition of business schemas and compiled metadata schemas into one
//! exact, offline draft-4 event contract.
//!
//! The composed document validates the envelope `detail` object: required
//! `metadata` and `data` sections, metadata restricted to the exact revision
//! with both numeric version stamps pinned, and business data validated in
//! its original schema scope.
//!
//! The business document is bundled as a distinct embedded draft-4 resource
//! — preserving its `id` scope or assigning a deterministic internal
//! identifier — because naively pasting business JSON below
//! `properties.data` would re-interpret root-local references against the
//! composed envelope. All references must resolve inside the exported
//! document; there is no network resolution path (the `jsonschema` build
//! disables remote retrieval, and [`compose_detail_schema`] rejects
//! external references before compilation).

use serde_json::{json, Value};
use uuid::Uuid;

use crate::metadata::definition::{MetadataDefinition, ValidatedMetadataDefinition};
use crate::metadata::schema::{compile_metadata_schema, CompiledMetadataSchema};
use crate::metadata::selector::MetadataRevision;

/// Composer format stamped on every composed contract document. Bump this
/// when composition semantics change; caches key on it.
pub const COMPOSER_FORMAT_VERSION: u32 = 1;

/// Where the embedded business resource lives inside the composed document.
const BUSINESS_RESOURCE_KEY: &str = "konductorBusiness";

/// The deterministic internal identifier namespace for business documents
/// without an `id` of their own.
const ASSIGNED_ID_PREFIX: &str = "urn:konductor:schema:";

#[derive(Debug, thiserror::Error)]
pub enum CompositionError {
    /// The business content cannot participate in a contract. At admission
    /// this is a platform-side fault (503); at authoring it precedes any
    /// registry write (400).
    #[error("business schema is unusable: {0}")]
    BusinessUnusable(String),
    /// The business document depends on a reference that cannot resolve
    /// offline inside the composed document.
    #[error("business reference cannot resolve offline: {0}")]
    UnresolvableReference(String),
    /// The event detail violates its composed contract.
    #[error("detail violates its contract: {0}")]
    DetailViolations(String),
}

/// A composed contract document plus the composer format that produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposedDocument {
    pub composer_format: u32,
    pub document: Value,
}

impl ComposedDocument {
    pub fn document_string(&self) -> String {
        serde_json::to_string(&self.document).unwrap_or_default()
    }
}

/// The deterministic identifier for a business schema document: its own
/// `id` when present, otherwise an internal URN derived from its content.
pub fn business_identifier(business_content: &str) -> String {
    match serde_json::from_str::<Value>(business_content)
        .ok()
        .and_then(|doc| doc.get("id").and_then(Value::as_str).map(str::to_string))
    {
        Some(existing) => existing,
        None => format!(
            "{ASSIGNED_ID_PREFIX}{}",
            Uuid::new_v5(&Uuid::NAMESPACE_URL, business_content.as_bytes())
        ),
    }
}

/// Composes the exact draft-4 contract for one `(business version, metadata
/// revision)` pair. `business_content` is the stored raw business document;
/// it is never modified — only the embedded copy gains an assigned `id`.
pub fn compose_detail_schema(
    business_version: i64,
    business_content: &str,
    metadata: &CompiledMetadataSchema,
) -> Result<ComposedDocument, CompositionError> {
    let mut business: Value = serde_json::from_str(business_content)
        .map_err(|e| CompositionError::BusinessUnusable(format!("not parseable JSON: {e}")))?;
    if !business.is_object() {
        return Err(CompositionError::BusinessUnusable(
            "schema document must be a JSON object".to_string(),
        ));
    }
    let identifier = match business.get("id") {
        Some(Value::String(existing)) => existing.to_string(),
        Some(_) => {
            return Err(CompositionError::BusinessUnusable(
                "schema `id` must be a string".to_string(),
            ))
        }
        None => {
            let assigned = business_identifier(business_content);
            business
                .as_object_mut()
                .expect("checked above")
                .insert("id".to_string(), json!(assigned));
            assigned
        }
    };
    check_offline_references(&business, &identifier)?;
    Ok(ComposedDocument {
        composer_format: COMPOSER_FORMAT_VERSION,
        document: json!({
            "$schema": "http://json-schema.org/draft-04/schema#",
            "type": "object",
            "properties": {
                "metadata": inline_metadata_document(business_version, metadata),
                "data": {"$ref": format!("{identifier}#")}
            },
            "required": ["data", "metadata"],
            "additionalProperties": false,
            "definitions": {BUSINESS_RESOURCE_KEY: business}
        }),
    })
}

/// The inline metadata section of the composed document: the exact revision
/// schema with `schemaVersion` pinned to this business version. The embedded
/// `$schema` declaration is dropped; the composed root owns the dialect.
fn inline_metadata_document(business_version: i64, metadata: &CompiledMetadataSchema) -> Value {
    let mut document = metadata.document.clone();
    let object = document
        .as_object_mut()
        .expect("compiled metadata documents are objects");
    object.remove("$schema");
    if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert(
            "schemaVersion".to_string(),
            json!({"type": "integer", "enum": [business_version]}),
        );
    }
    document
}

/// How many contract violations make it into a validation error message
/// before truncation.
const MAX_REPORTED_VIOLATIONS: usize = 10;

/// The baseline revision used for the authoring-time composability check:
/// the protected defaults with no extensions. Any business document that
/// composes with this wrapper composes structurally; external references
/// fail for every revision.
const BASELINE_REVISION: u32 = 1;

/// The authoring-time representation-safety check: the business document
/// must compose offline with the supported metadata wrapper and compile.
/// This does not replace or weaken the independent draft-4 and
/// business-compatibility checks; it rejects documents that depend on
/// references no composed contract can resolve before anything is stored.
pub fn check_offline_composability(business_content: &str) -> Result<(), CompositionError> {
    let definition = ValidatedMetadataDefinition::validate(MetadataDefinition::default())
        .map_err(|e| CompositionError::BusinessUnusable(e.to_string()))?;
    let revision = MetadataRevision::new(BASELINE_REVISION)
        .map_err(|e| CompositionError::BusinessUnusable(e.to_string()))?;
    let baseline = compile_metadata_schema(revision, &definition)
        .map_err(|e| CompositionError::BusinessUnusable(e.to_string()))?;
    let composed = compose_detail_schema(BASELINE_REVISION as i64, business_content, &baseline)?;
    compile_composed(&composed).map(|_| ())
}

/// Compiles a composed contract for offline validation, registering the
/// embedded business resource locally under its identifier.
///
/// Registration is required, not an optimization: in-situ compilation does
/// not apply an embedded `id` to a `$ref` in the same schema object, so a
/// business document whose root is itself a reference would silently
/// resolve against the composed envelope. There is no network resolution
/// path — external references are rejected at composition time, and the
/// `jsonschema` build has no remote retrieval.
pub fn compile_composed(
    composed: &ComposedDocument,
) -> Result<jsonschema::Validator, CompositionError> {
    let business = composed
        .document
        .pointer(&format!("/definitions/{BUSINESS_RESOURCE_KEY}"))
        .cloned()
        .ok_or_else(|| {
            CompositionError::BusinessUnusable(
                "composed document is missing its embedded business resource".to_string(),
            )
        })?;
    let identifier = composed
        .document
        .pointer("/properties/data/$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_suffix('#'))
        .ok_or_else(|| {
            CompositionError::BusinessUnusable(
                "composed document is missing its business identifier reference".to_string(),
            )
        })?;
    let registry = jsonschema::Registry::options()
        .draft(jsonschema::Draft::Draft4)
        .build([(identifier, jsonschema::Resource::from_contents(business))])
        .map_err(|e| {
            CompositionError::BusinessUnusable(format!("business resource is unusable: {e}"))
        })?;
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft4)
        .with_registry(registry)
        .build(&composed.document)
        .map_err(|e| CompositionError::BusinessUnusable(e.to_string()))
}

/// Validates an event `detail` object against its composed contract.
pub fn validate_composed_detail(
    composed: &ComposedDocument,
    detail: &Value,
) -> Result<(), CompositionError> {
    let validator = compile_composed(composed)?;
    let mut violations = validator
        .iter_errors(detail)
        .take(MAX_REPORTED_VIOLATIONS)
        .map(|e| e.to_string());
    match violations.next() {
        None => Ok(()),
        Some(first) => {
            let mut message = first;
            for violation in violations {
                message.push_str("; ");
                message.push_str(&violation);
            }
            Err(CompositionError::DetailViolations(message))
        }
    }
}

/// Walks the embedded business copy collecting in-document `id` scopes and
/// checking every `$ref` resolves offline: fragment-only references stay in
/// their scope, absolute references must name an in-document `id`, and
/// anything else (relative paths, remote URLs, file URIs, unknown schemes)
/// is rejected before compilation with the offending reference.
fn check_offline_references(
    business: &Value,
    root_identifier: &str,
) -> Result<(), CompositionError> {
    let mut ids = vec![root_identifier.to_string()];
    let mut refs = Vec::new();
    scan_schema_values(business, &mut ids, &mut refs);
    for reference in refs {
        if !is_offline_reference(&reference, &ids) {
            return Err(CompositionError::UnresolvableReference(format!(
                "'{reference}' does not resolve inside the composed document"
            )));
        }
    }
    Ok(())
}

fn scan_schema_values(value: &Value, ids: &mut Vec<String>, refs: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                ids.push(id.to_string());
            }
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                refs.push(reference.to_string());
            }
            for child in object.values() {
                scan_schema_values(child, ids, refs);
            }
        }
        Value::Array(items) => {
            for child in items {
                scan_schema_values(child, ids, refs);
            }
        }
        _ => {}
    }
}

fn is_offline_reference(reference: &str, ids: &[String]) -> bool {
    if let Some(stripped) = reference.strip_prefix('#') {
        return stripped.is_empty()
            || stripped.starts_with('/')
            || stripped.starts_with("definitions/");
    }
    let without_fragment = reference.split('#').next().unwrap_or(reference);
    !without_fragment.is_empty() && ids.iter().any(|id| id == without_fragment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::definition::{
        Generator, MetadataDefinition, MetadataNode, ValidatedMetadataDefinition,
    };
    use crate::metadata::schema::compile_metadata_schema;
    use crate::metadata::selector::MetadataRevision;

    const BUSINESS_VERSION: i64 = 3;
    const METADATA_REVISION: u32 = 2;

    fn metadata_schema() -> CompiledMetadataSchema {
        let definition = ValidatedMetadataDefinition::validate(MetadataDefinition {
            children: vec![MetadataNode::Field {
                name: "eventId".to_string(),
                generator: Generator::UuidV4,
            }],
        })
        .unwrap();
        compile_metadata_schema(
            MetadataRevision::new(METADATA_REVISION).unwrap(),
            &definition,
        )
        .unwrap()
    }

    fn compose(business_content: &str) -> ComposedDocument {
        compose_detail_schema(BUSINESS_VERSION, business_content, &metadata_schema()).unwrap()
    }

    fn compose_err(business_content: &str) -> CompositionError {
        compose_detail_schema(BUSINESS_VERSION, business_content, &metadata_schema()).unwrap_err()
    }

    fn validate_detail(composed: &ComposedDocument, detail: &Value) -> Result<(), String> {
        validate_composed_detail(composed, detail).map_err(|e| e.to_string())
    }

    fn valid_detail(data: Value) -> Value {
        json!({
            "metadata": {
                "schemaVersion": BUSINESS_VERSION,
                "idempotencyKey": "key-123",
                "publishedAt": "2026-07-30T10:30:00.123Z",
                "metadataRevision": METADATA_REVISION,
                "eventId": "0f14d0ab-9605-4d62-bc3f-abcdef123456"
            },
            "data": data
        })
    }

    #[test]
    fn primitive_business_root_validates_data_in_its_original_scope() {
        let composed = compose(r#"{"type": "string"}"#);
        assert!(validate_detail(&composed, &valid_detail(json!("hello"))).is_ok());
        assert!(validate_detail(&composed, &valid_detail(json!(42))).is_err());
    }

    #[test]
    fn root_array_business_schema_composes() {
        let composed = compose(r#"{"type": "array", "items": {"type": "integer"}}"#);
        assert!(validate_detail(&composed, &valid_detail(json!([1, 2, 3]))).is_ok());
        assert!(validate_detail(&composed, &valid_detail(json!(["x"]))).is_err());
        assert!(validate_detail(&composed, &valid_detail(json!({"a": 1}))).is_err());
    }

    #[test]
    fn closed_business_objects_stay_closed() {
        let composed = compose(
            r#"{
                "type": "object",
                "properties": {"orderId": {"type": "integer"}},
                "required": ["orderId"],
                "additionalProperties": false
            }"#,
        );
        assert!(validate_detail(&composed, &valid_detail(json!({"orderId": 1}))).is_ok());
        assert!(
            validate_detail(&composed, &valid_detail(json!({"orderId": 1, "extra": 2}))).is_err()
        );
    }

    #[test]
    fn root_local_references_stay_in_the_business_scope() {
        let composed = compose(
            r##"{
                "type": "object",
                "properties": {
                    "item": {"$ref": "#/definitions/item"}
                },
                "required": ["item"],
                "definitions": {
                    "item": {
                        "type": "object",
                        "properties": {"sku": {"type": "string"}},
                        "required": ["sku"]
                    }
                }
            }"##,
        );
        assert!(validate_detail(&composed, &valid_detail(json!({"item": {"sku": "a"}}))).is_ok());
        assert!(validate_detail(&composed, &valid_detail(json!({"item": {}}))).is_err());
    }

    #[test]
    fn a_root_reference_resolves_against_the_business_document_not_the_envelope() {
        // The whole business root is a reference. Naive embedding below
        // `properties.data` would reinterpret `#/definitions/order` against
        // the composed envelope; with scope isolation it must resolve to
        // the business document's own definition and validate data with it.
        let composed = compose(
            r##"{
                "definitions": {
                    "order": {
                        "type": "object",
                        "properties": {"orderId": {"type": "integer"}},
                        "required": ["orderId"],
                        "additionalProperties": false
                    }
                },
                "$ref": "#/definitions/order"
            }"##,
        );
        assert!(validate_detail(&composed, &valid_detail(json!({"orderId": 5}))).is_ok());
        assert!(validate_detail(&composed, &valid_detail(json!({"orderId": "x"}))).is_err());
        assert!(validate_detail(&composed, &valid_detail(json!({}))).is_err());
    }

    #[test]
    fn recursive_definitions_compose() {
        let composed = compose(
            r##"{
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "children": {
                        "type": "array",
                        "items": {"$ref": "#/definitions/node"}
                    }
                },
                "required": ["name"],
                "definitions": {
                    "node": {
                        "type": "object",
                        "properties": {"name": {"type": "string"}},
                        "required": ["name"]
                    }
                }
            }"##,
        );
        let valid = json!({"name": "root", "children": [{"name": "a"}, {"name": "b"}]});
        assert!(validate_detail(&composed, &valid_detail(valid)).is_ok());
        let invalid = json!({"name": "root", "children": [{"nope": 1}]});
        assert!(validate_detail(&composed, &valid_detail(invalid)).is_err());
    }

    #[test]
    fn nested_id_scopes_are_preserved() {
        let composed = compose(
            r#"{
                "type": "object",
                "properties": {"entry": {"$ref": "urn:acme:entry"}},
                "required": ["entry"],
                "definitions": {
                    "entrySchema": {
                        "id": "urn:acme:entry",
                        "type": "object",
                        "properties": {"code": {"type": "string"}},
                        "required": ["code"]
                    }
                }
            }"#,
        );
        assert!(validate_detail(&composed, &valid_detail(json!({"entry": {"code": "x"}}))).is_ok());
        assert!(validate_detail(&composed, &valid_detail(json!({"entry": {}}))).is_err());
    }

    #[test]
    fn an_existing_business_id_is_preserved_for_both_reference_forms() {
        let composed = compose(
            r##"{
                "id": "urn:acme:order",
                "type": "object",
                "properties": {
                    "item": {"$ref": "#/definitions/item"},
                    "self": {"$ref": "urn:acme:order#/definitions/item"}
                },
                "required": ["item"],
                "definitions": {
                    "item": {
                        "type": "object",
                        "properties": {"sku": {"type": "string"}},
                        "required": ["sku"]
                    }
                }
            }"##,
        );
        let valid = json!({"item": {"sku": "a"}, "self": {"sku": "b"}});
        assert!(validate_detail(&composed, &valid_detail(valid)).is_ok());
        assert!(validate_detail(&composed, &valid_detail(json!({"item": {}}))).is_err());
    }

    #[test]
    fn reserved_name_collisions_preserve_standalone_data_validation() {
        // Business data may itself use the reserved envelope names; the
        // composed document must keep the two scopes independent.
        let composed = compose(
            r##"{
                "type": "object",
                "properties": {
                    "metadata": {"type": "string"},
                    "definitions": {"type": "integer"},
                    "flag": {"$ref": "#/definitions/helper"}
                },
                "required": ["metadata"],
                "additionalProperties": false,
                "definitions": {"helper": {"type": "boolean"}}
            }"##,
        );
        let valid = json!({"metadata": "info", "definitions": 7, "flag": true});
        assert!(validate_detail(&composed, &valid_detail(valid)).is_ok());
        let wrong_type = json!({"metadata": 5});
        assert!(validate_detail(&composed, &valid_detail(wrong_type)).is_err());
        let wrong_flag = json!({"metadata": "info", "flag": "not-a-bool"});
        assert!(validate_detail(&composed, &valid_detail(wrong_flag)).is_err());
    }

    #[test]
    fn external_references_are_rejected_before_compilation() {
        for reference in [
            "https://example.com/remote.json",
            "file:///etc/passwd",
            "common.json#/definitions/x",
            "unknown-scheme://x",
        ] {
            let business = json!({
                "type": "object",
                "properties": {"x": {"$ref": reference}}
            });
            let err = compose_err(&business.to_string());
            let CompositionError::UnresolvableReference(message) = &err else {
                panic!("expected UnresolvableReference for {reference}, got {err}");
            };
            assert!(
                message.contains(reference),
                "rejection must name the reference: {message}"
            );
        }
    }

    #[test]
    fn unusable_business_content_is_rejected() {
        assert!(matches!(
            compose_err("not json"),
            CompositionError::BusinessUnusable(_)
        ));
        assert!(matches!(
            compose_err(r#""a string""#),
            CompositionError::BusinessUnusable(_)
        ));
        assert!(matches!(
            compose_err(r#"{"type": "string", "id": 5}"#),
            CompositionError::BusinessUnusable(_)
        ));
    }

    #[test]
    fn the_envelope_detail_shape_is_enforced() {
        let composed = compose(r#"{"type": "object"}"#);
        let mut with_extra_key = valid_detail(json!({}));
        with_extra_key["source"] = json!("acme-orders");
        assert!(validate_detail(&composed, &with_extra_key).is_err());

        let mut missing_data = valid_detail(json!({}));
        missing_data.as_object_mut().unwrap().remove("data");
        assert!(validate_detail(&composed, &missing_data).is_err());
    }

    #[test]
    fn both_numeric_version_stamps_are_pinned_to_the_exact_contract() {
        let composed = compose(r#"{"type": "object"}"#);
        let mut wrong_business_version = valid_detail(json!({}));
        wrong_business_version["metadata"]["schemaVersion"] = json!(BUSINESS_VERSION + 1);
        assert!(
            validate_detail(&composed, &wrong_business_version).is_err(),
            "the composed contract must reject a mismatched business version"
        );

        let mut wrong_revision = valid_detail(json!({}));
        wrong_revision["metadata"]["metadataRevision"] = json!(METADATA_REVISION + 1);
        assert!(
            validate_detail(&composed, &wrong_revision).is_err(),
            "the composed contract must reject a mismatched metadata revision"
        );
    }

    #[test]
    fn missing_or_unknown_metadata_fields_are_rejected() {
        let composed = compose(r#"{"type": "object"}"#);
        let mut missing_extension = valid_detail(json!({}));
        missing_extension["metadata"]
            .as_object_mut()
            .unwrap()
            .remove("eventId");
        assert!(validate_detail(&composed, &missing_extension).is_err());

        let mut unknown_extension = valid_detail(json!({}));
        unknown_extension["metadata"]["extra"] = json!("nope");
        assert!(validate_detail(&composed, &unknown_extension).is_err());
    }

    #[test]
    fn composed_documents_compile_offline_with_local_resource_registration() {
        let fixtures = [
            r#"{"type": "string"}"#,
            r#"{"type": "array", "items": {"type": "integer"}}"#,
            r##"{"definitions": {"order": {"type": "object", "properties": {"orderId": {"type": "integer"}}, "required": ["orderId"]}}, "$ref": "#/definitions/order"}"##,
            r##"{"id": "urn:acme:tree", "definitions": {"node": {"type": "object", "properties": {"child": {"$ref": "#/definitions/node"}}}}, "$ref": "#/definitions/node"}"##,
        ];
        for fixture in fixtures {
            let composed = compose(fixture);
            assert!(
                compile_composed(&composed).is_ok(),
                "composed contract must compile offline: {fixture}"
            );
        }
        // Documents without root-level business references also compile
        // without any registration; root-reference documents require the
        // local resource registration that `compile_composed` performs.
        let without_root_refs = fixtures[0];
        let composed = compose(without_root_refs);
        assert!(
            crate::schema_validation::check_draft4_document(&composed.document_string()).is_ok()
        );
    }

    #[test]
    fn composition_is_deterministic_and_assigns_stable_identifiers() {
        let business = r#"{"type": "object", "properties": {"orderId": {"type": "integer"}}}"#;
        let first = compose(business);
        let second = compose(business);
        assert_eq!(first, second);
        assert_eq!(
            business_identifier(business),
            business_identifier(business),
            "assigned identifiers must be stable for identical content"
        );
        let embedded_id = first.document["definitions"][BUSINESS_RESOURCE_KEY]["id"]
            .as_str()
            .unwrap();
        assert_eq!(embedded_id, business_identifier(business));
        assert!(embedded_id.starts_with("urn:konductor:schema:"));
    }

    #[test]
    fn an_existing_business_id_is_always_preserved() {
        // Guard against the composer overriding an authored `id`.
        let business = r#"{"id": "urn:acme:order", "type": "object"}"#;
        let composed = compose(business);
        assert_eq!(
            composed.document["definitions"][BUSINESS_RESOURCE_KEY]["id"],
            json!("urn:acme:order")
        );
    }

    #[test]
    fn offline_composability_accepts_local_reference_documents() {
        let business = r##"{
            "type": "object",
            "properties": {"item": {"$ref": "#/definitions/item"}},
            "definitions": {"item": {"type": "object", "properties": {"sku": {"type": "string"}}, "required": ["sku"]}}
        }"##;
        assert!(check_offline_composability(business).is_ok());
        let with_root_ref =
            r##"{"definitions": {"order": {"type": "object"}}, "$ref": "#/definitions/order"}"##;
        assert!(check_offline_composability(with_root_ref).is_ok());
    }

    #[test]
    fn offline_composability_rejects_external_references() {
        let business = r#"{"type": "object", "properties": {"x": {"$ref": "https://example.com/remote.json"}}}"#;
        let err = check_offline_composability(business).unwrap_err();
        assert!(matches!(err, CompositionError::UnresolvableReference(_)));
        assert!(err.to_string().contains("example.com"));
    }

    #[test]
    fn offline_composability_rejects_unusable_content() {
        assert!(matches!(
            check_offline_composability("not json"),
            Err(CompositionError::BusinessUnusable(_))
        ));
    }

    #[test]
    fn composed_envelopes_across_metadata_revisions_are_not_gate_compatible() {
        // Design decision 7 (gate-schema-promotion integration): the
        // business compatibility gate compares publisher business documents
        // only — never composed envelopes. This pins why: a metadata
        // revision bump changes the wrapper (the pinned `metadataRevision`
        // enum moves), which the conservative business checker reports as a
        // tightening. If anyone ever routes composed documents through
        // `check_compatible`, metadata updates would falsely reject.
        let definition = ValidatedMetadataDefinition::validate(MetadataDefinition {
            children: vec![MetadataNode::Field {
                name: "eventId".to_string(),
                generator: Generator::UuidV4,
            }],
        })
        .unwrap();
        let business = r#"{"type": "object", "properties": {"orderId": {"type": "integer"}}}"#;
        let revision_one =
            compile_metadata_schema(MetadataRevision::new(1).unwrap(), &definition).unwrap();
        let revision_two =
            compile_metadata_schema(MetadataRevision::new(2).unwrap(), &definition).unwrap();
        let composed_one = compose_detail_schema(1, business, &revision_one).unwrap();
        let composed_two = compose_detail_schema(1, business, &revision_two).unwrap();

        let found = crate::schema_compatibility::check_compatible(
            &composed_one.document,
            &composed_two.document,
        );
        assert!(
            !found.is_accepted(),
            "the metadata wrapper differs across revisions in ways the business gate must never police: {found:?}"
        );
        // The same pair of raw business documents is trivially compatible:
        // identical input, no tightening. The gate's input contract holds.
        let business_doc: Value = serde_json::from_str(business).unwrap();
        assert!(
            crate::schema_compatibility::check_compatible(&business_doc, &business_doc)
                .is_accepted()
        );
    }
}
