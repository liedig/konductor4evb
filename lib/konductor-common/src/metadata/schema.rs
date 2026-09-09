//! Versioned compilation of metadata definitions into exact draft-4
//! validation schemas.
//!
//! Every compiled schema is stamped with [`METADATA_SCHEMA_COMPILER_FORMAT`]
//! and persisted with its revision, so a future compiler change cannot
//! silently reinterpret an old definition: retained formats keep producing
//! their historical documents, and the golden fixtures in this module's
//! tests pin that behavior.

use serde_json::{json, Map, Value};

use crate::error::{ControlPlaneError, ValidationExceptionField, ValidationExceptionReason};
use crate::metadata::definition::{
    GeneratedType, Generator, MetadataNode, ValidatedMetadataDefinition,
};
use crate::metadata::selector::MetadataRevision;

/// Compiler format stamped on every generated metadata schema. Bump this and
/// retain the old format's output when compilation semantics change.
pub const METADATA_SCHEMA_COMPILER_FORMAT: u32 = 1;

const DRAFT4_DECLARATION: &str = "http://json-schema.org/draft-04/schema#";
const UUID_V4_PATTERN: &str =
    "^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";
const RFC3339_MILLIS_PATTERN: &str =
    "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\\.[0-9]{3}Z$";
const UTC_DATE_PATTERN: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}$";

/// A compiled metadata schema for one exact revision: the document plus the
/// compiler format that produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledMetadataSchema {
    pub compiler_format: u32,
    pub revision: MetadataRevision,
    pub document: Value,
}

impl CompiledMetadataSchema {
    /// The serialized document, ready to persist beside its immutable
    /// revision and to hand to any offline draft-4 compiler.
    pub fn document_string(&self) -> String {
        serde_json::to_string(&self.document).unwrap_or_default()
    }

    /// Validates a full metadata block (built-ins plus extension values)
    /// against this exact revision's closed contract, using the same
    /// draft-4 validator family admission and delivery rely on. Readiness
    /// probes platform-generated values through this; it performs no
    /// configuration reads and no network resolution.
    pub fn validate_block(&self, block: &Value) -> Result<(), String> {
        let validator = jsonschema::options()
            .with_draft(jsonschema::Draft::Draft4)
            .build(&self.document)
            .map_err(|e| format!("metadata schema is not compilable: {e}"))?;
        let mut violations = validator
            .iter_errors(block)
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
                Err(message)
            }
        }
    }
}

/// Bounded violation reporting, matching the composition module's cap so
/// error surfaces stay actionable rather than exhaustive.
const MAX_REPORTED_VIOLATIONS: usize = 5;

/// The read-only legacy contract for events predating metadata stamping:
/// the original three built-in fields with their original semantics, closed
/// against any newer field. Revision 0 has no configurable definition.
pub fn legacy_metadata_schema() -> CompiledMetadataSchema {
    CompiledMetadataSchema {
        compiler_format: METADATA_SCHEMA_COMPILER_FORMAT,
        revision: MetadataRevision::LEGACY,
        document: json!({
            "$schema": DRAFT4_DECLARATION,
            "type": "object",
            "properties": {
                "schemaVersion": {"type": "integer", "minimum": 1},
                "idempotencyKey": {"type": "string", "minLength": 1},
                "publishedAt": {"type": "string", "pattern": RFC3339_MILLIS_PATTERN}
            },
            "required": ["idempotencyKey", "publishedAt", "schemaVersion"],
            "additionalProperties": false
        }),
    }
}

/// Compiles the exact draft-4 metadata schema for a positive revision: the
/// four protected built-ins (with `metadataRevision` pinned to this exact
/// revision) plus every configured extension field, all required, closed
/// against unknown fields. Output is deterministic for a given definition.
pub fn compile_metadata_schema(
    revision: MetadataRevision,
    definition: &ValidatedMetadataDefinition,
) -> Result<CompiledMetadataSchema, ControlPlaneError> {
    if revision.is_legacy() {
        return Err(ControlPlaneError::ValidationException {
            message:
                "revision 0 is the read-only legacy selector and has no configurable definition"
                    .to_string(),
            reason: ValidationExceptionReason::FieldValidationFailed,
            field_list: Some(vec![ValidationExceptionField {
                name: "metadataRevision".to_string(),
                message: "must be a positive revision; use the legacy contract for revision 0"
                    .to_string(),
            }]),
        });
    }
    let mut properties = Map::new();
    let mut required: Vec<String> = vec![
        "schemaVersion".to_string(),
        "idempotencyKey".to_string(),
        "publishedAt".to_string(),
        "metadataRevision".to_string(),
    ];
    properties.insert(
        "schemaVersion".to_string(),
        json!({"type": "integer", "minimum": 1}),
    );
    properties.insert(
        "idempotencyKey".to_string(),
        json!({"type": "string", "minLength": 1}),
    );
    properties.insert(
        "publishedAt".to_string(),
        json!({"type": "string", "pattern": RFC3339_MILLIS_PATTERN}),
    );
    properties.insert(
        "metadataRevision".to_string(),
        json!({"type": "integer", "enum": [revision.as_u32()]}),
    );
    compile_children(definition.children(), &mut properties, &mut required)?;
    required.sort();
    Ok(CompiledMetadataSchema {
        compiler_format: METADATA_SCHEMA_COMPILER_FORMAT,
        revision,
        document: json!({
            "$schema": DRAFT4_DECLARATION,
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        }),
    })
}

fn compile_children(
    children: &[MetadataNode],
    properties: &mut Map<String, Value>,
    required: &mut Vec<String>,
) -> Result<(), ControlPlaneError> {
    for node in children {
        match node {
            MetadataNode::Field { name, generator } => {
                properties.insert(name.clone(), generated_property(*generator));
                required.push(name.clone());
            }
            MetadataNode::Object { name, children } => {
                let mut nested_properties = Map::new();
                let mut nested_required = Vec::new();
                compile_children(children, &mut nested_properties, &mut nested_required)?;
                nested_required.sort();
                properties.insert(
                    name.clone(),
                    json!({
                        "type": "object",
                        "properties": nested_properties,
                        "required": nested_required,
                        "additionalProperties": false
                    }),
                );
                required.push(name.clone());
            }
        }
    }
    Ok(())
}

fn generated_property(generator: Generator) -> Value {
    match generator.generated_type() {
        GeneratedType::UuidV4 => json!({"type": "string", "pattern": UUID_V4_PATTERN}),
        GeneratedType::UtcTimestamp => {
            json!({"type": "string", "pattern": RFC3339_MILLIS_PATTERN})
        }
        GeneratedType::UtcDate => json!({"type": "string", "pattern": UTC_DATE_PATTERN}),
        GeneratedType::NonEmptyString => json!({"type": "string", "minLength": 1}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::definition::MetadataDefinition;
    use crate::schema_validation::{validate_against_latest, LatestSchema};

    fn field(name: &str, generator: Generator) -> MetadataNode {
        MetadataNode::Field {
            name: name.to_string(),
            generator,
        }
    }

    fn object(name: &str, children: Vec<MetadataNode>) -> MetadataNode {
        MetadataNode::Object {
            name: name.to_string(),
            children,
        }
    }

    fn definition(children: Vec<MetadataNode>) -> ValidatedMetadataDefinition {
        ValidatedMetadataDefinition::validate(MetadataDefinition { children }).unwrap()
    }

    fn compile(revision: u32, children: Vec<MetadataNode>) -> CompiledMetadataSchema {
        compile_metadata_schema(
            MetadataRevision::new(revision).unwrap(),
            &definition(children),
        )
        .unwrap()
    }

    fn validate(schema: &CompiledMetadataSchema, metadata: Value) -> Result<(), String> {
        let latest = LatestSchema {
            content: schema.document_string(),
            version: 1,
        };
        validate_against_latest(&latest, &metadata)
            .map_err(|e| format!("expected validation to pass, got: {e}"))
    }

    fn baseline_metadata(revision: u32) -> Value {
        json!({
            "schemaVersion": 3,
            "idempotencyKey": "key-123",
            "publishedAt": "2026-07-30T10:30:00.123Z",
            "metadataRevision": revision
        })
    }

    #[test]
    fn legacy_schema_preserves_the_original_three_field_contract() {
        let legacy = legacy_metadata_schema();
        assert!(legacy.revision.is_legacy());
        assert_eq!(legacy.compiler_format, METADATA_SCHEMA_COMPILER_FORMAT);
        let properties = legacy.document["properties"].as_object().unwrap();
        assert_eq!(properties.len(), 3);
        assert!(validate(
            &legacy,
            json!({
                "schemaVersion": 3,
                "idempotencyKey": "key-123",
                "publishedAt": "2026-07-30T10:30:00.123Z"
            })
        )
        .is_ok());
    }

    #[test]
    fn legacy_schema_rejects_newer_metadata_fields() {
        let legacy = legacy_metadata_schema();
        assert!(validate(
            &legacy,
            json!({
                "schemaVersion": 3,
                "idempotencyKey": "key-123",
                "publishedAt": "2026-07-30T10:30:00.123Z",
                "metadataRevision": 1
            })
        )
        .is_err());
        assert!(validate(
            &legacy,
            json!({
                "schemaVersion": 3,
                "idempotencyKey": "key-123",
                "publishedAt": "2026-07-30T10:30:00.123Z",
                "eventId": "0f14d0ab-9605-4d62-bc3f-abcdef123456"
            })
        )
        .is_err());
    }

    #[test]
    fn legacy_revision_cannot_take_a_configurable_definition() {
        let err =
            compile_metadata_schema(MetadataRevision::LEGACY, &definition(vec![])).unwrap_err();
        assert!(err.to_string().contains("legacy selector"));
    }

    #[test]
    fn baseline_revision_one_contains_only_the_protected_built_ins() {
        let baseline = compile(1, vec![]);
        let properties = baseline.document["properties"].as_object().unwrap();
        assert_eq!(properties.len(), 4);
        assert!(properties.contains_key("metadataRevision"));
        assert!(validate(&baseline, baseline_metadata(1)).is_ok());
    }

    #[test]
    fn exact_contracts_require_configured_fields_and_reject_unknown_ones() {
        let schema = compile(2, vec![field("eventId", Generator::UuidV4)]);
        let mut metadata = baseline_metadata(2);
        metadata["eventId"] = json!("0f14d0ab-9605-4d62-bc3f-abcdef123456");
        assert!(validate(&schema, metadata.clone()).is_ok());

        // Missing the configured field fails.
        let mut missing = metadata.clone();
        missing.as_object_mut().unwrap().remove("eventId");
        assert!(validate(&schema, missing).is_err());

        // An unconfigured extra field fails.
        let mut extra = metadata.clone();
        extra["extra"] = json!("nope");
        assert!(validate(&schema, extra).is_err());
    }

    #[test]
    fn metadata_revision_is_pinned_to_the_exact_revision() {
        let m2 = compile(2, vec![]);
        let mut stamped_wrong = baseline_metadata(3);
        stamped_wrong["metadataRevision"] = json!(3);
        assert!(
            validate(&m2, stamped_wrong).is_err(),
            "an M2 contract must reject metadataRevision 3"
        );
        assert!(validate(&m2, baseline_metadata(2)).is_ok());
    }

    #[test]
    fn original_defaults_keep_their_semantics() {
        let baseline = compile(1, vec![]);
        let mut zero_version = baseline_metadata(1);
        zero_version["schemaVersion"] = json!(0);
        assert!(validate(&baseline, zero_version).is_err());

        let mut empty_key = baseline_metadata(1);
        empty_key["idempotencyKey"] = json!("");
        assert!(validate(&baseline, empty_key).is_err());

        let mut bad_timestamp = baseline_metadata(1);
        bad_timestamp["publishedAt"] = json!("2026-07-30T10:30:00Z");
        assert!(validate(&baseline, bad_timestamp).is_err());
    }

    #[test]
    fn generator_constraints_are_derived_into_the_document() {
        let schema = compile(
            3,
            vec![
                field("eventId", Generator::UuidV4),
                field("emittedAt", Generator::UtcTimestamp),
                field("emitDate", Generator::UtcDate),
                field("region", Generator::AwsRegion),
                object("correlation", vec![field("traceDate", Generator::UtcDate)]),
            ],
        );
        let properties = schema.document["properties"].as_object().unwrap();
        assert!(properties["eventId"]["pattern"]
            .as_str()
            .unwrap()
            .contains("4[0-9a-f]{3}"));
        assert!(properties["emittedAt"]["pattern"].is_string());
        assert!(properties["emitDate"]["pattern"].is_string());
        assert_eq!(properties["region"]["minLength"], json!(1));
        let nested = properties["correlation"]["properties"].as_object().unwrap();
        assert!(nested["traceDate"]["pattern"].is_string());

        let mut metadata = baseline_metadata(3);
        metadata["eventId"] = json!("0f14d0ab-9605-4d62-bc3f-abcdef123456");
        metadata["emittedAt"] = json!("2026-07-30T10:30:00.123Z");
        metadata["emitDate"] = json!("2026-07-30");
        metadata["region"] = json!("us-east-1");
        metadata["correlation"] = json!({"traceDate": "2026-07-30"});
        assert!(validate(&schema, metadata.clone()).is_ok());

        let mut nested_violation = metadata.clone();
        nested_violation["correlation"] = json!({"traceDate": "not-a-date"});
        assert!(validate(&schema, nested_violation).is_err());

        let mut missing_nested_field = metadata;
        missing_nested_field["correlation"] = json!({});
        assert!(validate(&schema, missing_nested_field).is_err());
    }

    #[test]
    fn compilation_is_deterministic_for_the_same_inputs() {
        let first = compile(4, vec![field("eventId", Generator::UuidV4)]);
        let second = compile(4, vec![field("eventId", Generator::UuidV4)]);
        assert_eq!(first, second);
        assert_eq!(first.document_string(), second.document_string());
    }

    #[test]
    fn compiled_documents_are_offline_draft4_compilable() {
        for schema in [
            legacy_metadata_schema(),
            compile(1, vec![]),
            compile(
                2,
                vec![object(
                    "correlation",
                    vec![field("traceId", Generator::UuidV4)],
                )],
            ),
        ] {
            assert!(
                crate::schema_validation::check_draft4_document(&schema.document_string()).is_ok()
            );
        }
    }

    #[test]
    fn golden_fixture_for_the_legacy_contract() {
        let golden: Value = serde_json::from_str(
            r#"{
              "$schema": "http://json-schema.org/draft-04/schema#",
              "type": "object",
              "properties": {
                "schemaVersion": {"type": "integer", "minimum": 1},
                "idempotencyKey": {"type": "string", "minLength": 1},
                "publishedAt": {"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\\.[0-9]{3}Z$"}
              },
              "required": ["idempotencyKey", "publishedAt", "schemaVersion"],
              "additionalProperties": false
            }"#,
        )
        .unwrap();
        assert_eq!(legacy_metadata_schema().document, golden);
    }

    #[test]
    fn golden_fixture_for_baseline_revision_one() {
        let golden: Value = serde_json::from_str(
            r#"{
              "$schema": "http://json-schema.org/draft-04/schema#",
              "type": "object",
              "properties": {
                "idempotencyKey": {"type": "string", "minLength": 1},
                "metadataRevision": {"type": "integer", "enum": [1]},
                "publishedAt": {"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\\.[0-9]{3}Z$"},
                "schemaVersion": {"type": "integer", "minimum": 1}
              },
              "required": ["idempotencyKey", "metadataRevision", "publishedAt", "schemaVersion"],
              "additionalProperties": false
            }"#,
        )
        .unwrap();
        let compiled = compile_metadata_schema(
            MetadataRevision::new(1).unwrap(),
            &ValidatedMetadataDefinition::validate(MetadataDefinition::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(compiled.document, golden);
        assert_eq!(compiled.compiler_format, METADATA_SCHEMA_COMPILER_FORMAT);
    }

    #[test]
    fn golden_fixture_for_an_extension_revision() {
        let golden: Value = serde_json::from_str(
            r#"{
              "$schema": "http://json-schema.org/draft-04/schema#",
              "type": "object",
              "properties": {
                "eventId": {"type": "string", "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"},
                "idempotencyKey": {"type": "string", "minLength": 1},
                "metadataRevision": {"type": "integer", "enum": [2]},
                "publishedAt": {"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\\.[0-9]{3}Z$"},
                "schemaVersion": {"type": "integer", "minimum": 1}
              },
              "required": ["eventId", "idempotencyKey", "metadataRevision", "publishedAt", "schemaVersion"],
              "additionalProperties": false
            }"#,
        )
        .unwrap();
        let compiled = compile(2, vec![field("eventId", Generator::UuidV4)]);
        assert_eq!(compiled.document, golden);
    }

    #[test]
    fn validate_block_accepts_exact_platform_output_and_rejects_drift() {
        let compiled = compile(2, vec![field("eventId", Generator::UuidV4)]);
        let exact = json!({
            "schemaVersion": 1,
            "idempotencyKey": "00000000-0000-4000-8000-000000000000",
            "publishedAt": "2026-01-01T00:00:00.000Z",
            "metadataRevision": 2,
            "eventId": "48151623-42cd-4c8a-9c7b-3f2a1d0e5f6a"
        });
        assert!(compiled.validate_block(&exact).is_ok());

        let unknown_field = json!({
            "schemaVersion": 1,
            "idempotencyKey": "00000000-0000-4000-8000-000000000000",
            "publishedAt": "2026-01-01T00:00:00.000Z",
            "metadataRevision": 2,
            "eventId": "48151623-42cd-4c8a-9c7b-3f2a1d0e5f6a",
            "injected": "producer-supplied values have no channel"
        });
        assert!(compiled.validate_block(&unknown_field).is_err());

        let wrong_revision = json!({
            "schemaVersion": 1,
            "idempotencyKey": "00000000-0000-4000-8000-000000000000",
            "publishedAt": "2026-01-01T00:00:00.000Z",
            "metadataRevision": 3,
            "eventId": "48151623-42cd-4c8a-9c7b-3f2a1d0e5f6a"
        });
        assert!(compiled.validate_block(&wrong_revision).is_err());
    }
}
