//! Validator-backed regressions for the compatibility gate.
//!
//! Every case here exercises two layers: the project's actual draft-4
//! validation configuration (the oracle that decides what publishers and
//! the admission path observe) and the compatibility checker. A checker
//! acceptance is only meaningful when the oracle confirms no previously
//! valid payload becomes invalid; a checker rejection is only a true
//! tightening when such a witness exists — otherwise it is a conservative
//! unsupported-proof rejection.

use serde_json::{json, Value};

use super::{check_compatible, CompatibilityCheck, CompatibilityRule};
use crate::metadata::composition::{
    compose_detail_schema, validate_composed_detail, ComposedDocument,
};
use crate::metadata::definition::{
    Generator, MetadataDefinition, MetadataNode, ValidatedMetadataDefinition,
};
use crate::metadata::schema::{compile_metadata_schema, CompiledMetadataSchema};
use crate::metadata::selector::MetadataRevision;
use crate::schema_validation::{
    check_draft4_document, validate_against_latest, LatestSchema, ValidationError,
};

/// Validates `payload` against `schema` through the same draft-4
/// compilation the admission path uses.
fn accepts(schema: &Value, payload: &Value) -> bool {
    let latest = LatestSchema {
        content: serde_json::to_string(schema).expect("schema serializes"),
        version: 1,
    };
    validate_against_latest(&latest, payload).is_ok()
}

fn metadata_schema() -> CompiledMetadataSchema {
    let definition = ValidatedMetadataDefinition::validate(MetadataDefinition {
        children: vec![MetadataNode::Field {
            name: "eventId".to_string(),
            generator: Generator::UuidV4,
        }],
    })
    .expect("definition validates");
    compile_metadata_schema(
        MetadataRevision::new(2).expect("revision 2 exists"),
        &definition,
    )
    .expect("metadata schema compiles")
}

fn compose(business_content: &str) -> ComposedDocument {
    compose_detail_schema(3, business_content, &metadata_schema()).expect("business composes")
}

fn composed_accepts(business: &Value, data: &Value) -> bool {
    let composed = compose(&serde_json::to_string(business).expect("business serializes"));
    let detail = json!({
        "metadata": {
            "schemaVersion": 3,
            "idempotencyKey": "key-123",
            "publishedAt": "2026-07-30T10:30:00.123Z",
            "metadataRevision": 2,
            "eventId": "0f14d0ab-9605-4d62-bc3f-abcdef123456"
        },
        "data": data
    });
    validate_composed_detail(&composed, &detail).is_ok()
}

/// Asserts the oracle witnesses a genuine breaking change: the predecessor
/// accepts the payload, the candidate rejects it, and the composed
/// EventBridge contract agrees with the raw business validation.
fn assert_breaking_witness(predecessor: &Value, candidate: &Value, witness: &Value) {
    assert!(
        accepts(predecessor, witness),
        "predecessor must accept the witness payload"
    );
    assert!(
        !accepts(candidate, witness),
        "candidate must reject the witness payload"
    );
    assert!(
        composed_accepts(predecessor, witness),
        "composed predecessor contract must accept the witness"
    );
    assert!(
        !composed_accepts(candidate, witness),
        "composed candidate contract must reject the witness"
    );
}

/// Asserts a payload stays valid across an evolution the gate accepts.
fn assert_preserved(predecessor: &Value, candidate: &Value, payload: &Value) {
    assert!(
        accepts(predecessor, payload),
        "predecessor must accept the payload"
    );
    assert!(
        accepts(candidate, payload),
        "candidate must keep accepting the payload"
    );
}

// --- Task 1.1: the two counterexamples found during exploration ---------

#[test]
fn optional_property_constraint_rejects_formerly_valid_payloads() {
    let predecessor = json!({"properties": {"sku": {}}});
    let candidate = json!({"properties": {"sku": {}, "note": {"type": "string"}}});
    assert_breaking_witness(&predecessor, &candidate, &json!({"sku": "A1", "note": 42}));
    let found = check_compatible(&predecessor, &candidate);
    assert!(
        !found.is_accepted(),
        "an optional property constraint that rejects previously allowed \
         values must not pass the gate"
    );
    let CompatibilityCheck::NotEstablished(reasons) = &found else {
        unreachable!()
    };
    assert!(
        reasons.iter().any(
            |i| i.path == "/note" && i.rule == CompatibilityRule::NewPropertyConstraintUnproven
        ),
        "the rejection must name the new property and its unproven rule: {reasons:?}"
    );
}

#[test]
fn reference_target_loosening_beneath_negation_rejects() {
    let predecessor = json!({
        "properties": {
            "a": {"type": "integer"},
            "b": {"not": {"$ref": "#/properties/a"}}
        }
    });
    let candidate = json!({
        "properties": {
            "a": {},
            "b": {"not": {"$ref": "#/properties/a"}}
        }
    });
    assert_breaking_witness(&predecessor, &candidate, &json!({"a": 1, "b": "text"}));
    let found = check_compatible(&predecessor, &candidate);
    let CompatibilityCheck::NotEstablished(reasons) = &found else {
        panic!(
            "a changed document containing schema-bearing references must not \
             pass on local loosening rules alone"
        )
    };
    assert!(
        reasons
            .iter()
            .any(|i| i.rule == CompatibilityRule::UnsupportedReferenceChange),
        "the rejection must name the unsupported reference semantics: {reasons:?}"
    );
}

// --- Task 1.2: validation-semantics matrix -------------------------------
//
// These tests record how the existing Rust draft-4 validator (and the
// composed contract built from it) treats the boundaries the checker's
// proof rules reason about. They pin the runtime semantics the checker
// must respect; they do not assert checker output.

#[test]
fn integer_bounds_compare_exactly_beyond_two_pow_53() {
    let at_two_pow_53 = json!({"maximum": 9_007_199_254_740_992_i64});
    assert!(
        !accepts(&at_two_pow_53, &json!(9_007_199_254_740_993_i64)),
        "2^53+1 exceeds maximum 2^53; an f64 comparison would collapse them"
    );
    let above_two_pow_53 = json!({"maximum": 9_007_199_254_740_993_i64});
    assert!(accepts(
        &above_two_pow_53,
        &json!(9_007_199_254_740_993_i64)
    ));
    assert!(!accepts(
        &above_two_pow_53,
        &json!(9_007_199_254_740_994_i64)
    ));
}

#[test]
fn integer_multiples_compare_exactly_up_to_the_largest_safe_divisor() {
    let schema = json!({"multipleOf": 9_007_199_254_740_991_i64});
    assert!(
        accepts(&schema, &json!(18_014_398_509_481_982_i64)),
        "2 * (2^53-1) is an exact multiple"
    );
    assert!(!accepts(&schema, &json!(18_014_398_509_481_983_i64)));
    assert!(!accepts(&schema, &json!(9_007_199_254_740_992_i64)));
}

#[test]
fn integer_multiple_of_divisors_above_two_pow_53_minus_1_are_refused_at_authoring() {
    let content = json!({"multipleOf": 9_007_199_254_740_993_i64}).to_string();
    assert!(
        matches!(
            check_draft4_document(&content),
            Err(ValidationError::SchemaUnusable(_))
        ),
        "the validator cannot decide divisibility exactly above 2^53-1, so \
         authoring must refuse the divisor"
    );
    let stored = LatestSchema {
        content,
        version: 1,
    };
    assert!(
        !matches!(
            validate_against_latest(&stored, &json!(1)),
            Err(ValidationError::SchemaUnusable(_))
        ),
        "a document stored before the gate keeps compiling at publish"
    );
}

#[test]
fn signed_and_unsigned_boundaries_validate() {
    let lower = json!({"minimum": -9_007_199_254_740_993_i64});
    assert!(accepts(&lower, &json!(-9_007_199_254_740_993_i64)));
    assert!(!accepts(&lower, &json!(-9_007_199_254_740_994_i64)));

    let upper = json!({"maximum": 18_446_744_073_709_551_615_u64});
    assert!(accepts(&upper, &json!(u64::MAX)));
}

#[test]
fn fractional_bounds_and_multiples_validate() {
    let multiple = json!({"multipleOf": 0.5});
    assert!(accepts(&multiple, &json!(1.5)));
    assert!(!accepts(&multiple, &json!(1.25)));

    let bound = json!({"minimum": 0.1});
    assert!(accepts(&bound, &json!(0.1)));
    assert!(!accepts(&bound, &json!(0.099)));

    let exclusive = json!({"maximum": 2.5, "exclusiveMaximum": true});
    assert!(!accepts(&exclusive, &json!(2.5)));
    assert!(accepts(&exclusive, &json!(2.4)));
}

#[test]
fn format_is_enforced_under_the_current_validator_configuration() {
    let schema = json!({"type": "string", "format": "date-time"});
    assert!(
        !accepts(&schema, &json!("not-a-date")),
        "the project's draft-4 build enforces format"
    );
    assert!(accepts(&schema, &json!("2026-07-30T10:30:00Z")));

    let email = json!({"type": "string", "format": "email"});
    assert!(
        !accepts(&email, &json!("2026-07-30T10:30:00Z")),
        "a format change alters the accepted set, so the checker must not \
         treat format as a free annotation"
    );
}

#[test]
fn id_scoped_local_references_resolve_in_raw_validation() {
    let schema = json!({
        "definitions": {
            "positive": {"id": "#positive", "type": "integer", "minimum": 1}
        },
        "properties": {"count": {"$ref": "#positive"}}
    });
    assert!(accepts(&schema, &json!({"count": 1})));
    assert!(
        !accepts(&schema, &json!({"count": 0})),
        "id changes reference resolution scope, so the checker cannot treat \
         it as a free annotation"
    );
}

// --- Task 1.3: supported safe evolution ----------------------------------
//
// Each case proves with payloads that the evolution cannot break a
// previously valid document, then asserts the gate accepts.

#[test]
fn identical_documents_with_references_stay_appendable() {
    let schema = json!({
        "type": "object",
        "properties": {"item": {"$ref": "#/definitions/item"}},
        "definitions": {"item": {"type": "object", "properties": {"sku": {"type": "string"}}}}
    });
    assert!(check_compatible(&schema, &schema).is_accepted());
    assert_preserved(&schema, &schema, &json!({"item": {"sku": "A1"}}));
}

#[test]
fn annotation_only_edits_stay_appendable() {
    let predecessor = json!({"type": "string", "title": "old", "description": "first"});
    let candidate = json!({"type": "string", "title": "new", "description": "second"});
    assert!(check_compatible(&predecessor, &candidate).is_accepted());
    assert_preserved(&predecessor, &candidate, &json!("value"));
}

#[test]
fn optional_property_addition_to_a_closed_object_stays_appendable() {
    let predecessor = json!({
        "type": "object",
        "properties": {"sku": {"type": "string"}},
        "additionalProperties": false
    });
    let candidate = json!({
        "type": "object",
        "properties": {"sku": {"type": "string"}, "note": {"type": "string"}},
        "additionalProperties": false
    });
    assert!(
        check_compatible(&predecessor, &candidate).is_accepted(),
        "the predecessor rejected every payload carrying `note`, so \
         constraining it cannot break a previously valid payload"
    );
    assert_preserved(&predecessor, &candidate, &json!({"sku": "A1"}));
    assert!(
        !accepts(&predecessor, &json!({"sku": "A1", "note": "x"})),
        "the closed predecessor rejects the new property outright"
    );
    assert!(accepts(&candidate, &json!({"sku": "A1", "note": "x"})));
}

#[test]
fn required_set_reduction_stays_appendable() {
    let predecessor = json!({"required": ["sku"], "properties": {"sku": {"type": "string"}}});
    let candidate = json!({"properties": {"sku": {"type": "string"}}});
    assert!(check_compatible(&predecessor, &candidate).is_accepted());
    assert_preserved(&predecessor, &candidate, &json!({"sku": "A1"}));
    assert!(!accepts(&predecessor, &json!({})));
    assert!(accepts(&candidate, &json!({})));
}

#[test]
fn type_and_enum_widening_stay_appendable() {
    let pred_type = json!({"type": "string"});
    let cand_type = json!({"type": ["string", "null"]});
    assert!(check_compatible(&pred_type, &cand_type).is_accepted());
    assert_preserved(&pred_type, &cand_type, &json!("x"));
    assert!(!accepts(&pred_type, &Value::Null));
    assert!(accepts(&cand_type, &Value::Null));

    let pred_enum = json!({"enum": ["a", "b"]});
    let cand_enum = json!({"enum": ["a", "b", "c"]});
    assert!(check_compatible(&pred_enum, &cand_enum).is_accepted());
    assert_preserved(&pred_enum, &cand_enum, &json!("a"));
}

#[test]
fn small_integer_bound_loosening_stays_appendable() {
    let predecessor = json!({"minimum": 5});
    let candidate = json!({"minimum": 4});
    assert!(check_compatible(&predecessor, &candidate).is_accepted());
    assert_preserved(&predecessor, &candidate, &json!(5));
    assert!(!accepts(&predecessor, &json!(4)));
    assert!(accepts(&candidate, &json!(4)));
}

// --- Task 3.4: bounded counterexample-detection corpus -------------------
//
// For every ordered pair the checker accepts, every sampled payload the
// predecessor validator accepts must stay accepted by the candidate. This
// is a detector for unsound acceptance rules, never a proof of soundness:
// each acceptance rule carries its own justification in rules.rs and
// structure.rs, and the corpus spans their interactions.

#[test]
fn accepted_corpus_pairs_preserve_every_sampled_payload() {
    let schemas: Vec<Value> = vec![
        json!({}),
        json!({"type": "object"}),
        json!({"type": "object", "properties": {"sku": {}}}),
        json!({"type": "object", "properties": {"sku": {"type": "string"}}}),
        json!({
            "type": "object",
            "properties": {"sku": {"type": "string"}},
            "additionalProperties": false
        }),
        json!({
            "type": "object",
            "properties": {"sku": {"type": "string"}, "note": {"type": "string"}},
            "additionalProperties": false
        }),
        json!({"type": "object", "properties": {"sku": {}}, "additionalProperties": {"type": "number"}}),
        json!({"type": "object", "properties": {"sku": {}}, "additionalProperties": {}}),
        json!({"required": ["sku"], "properties": {"sku": {"type": "string"}}}),
        json!({"properties": {"sku": {"type": "string"}}}),
        json!({"properties": {"sku": {}, "note": {"type": "string"}}}),
        json!({"type": "string"}),
        json!({"type": ["string", "null"]}),
        json!({"type": "string", "minLength": 2, "maxLength": 8}),
        json!({"type": "string", "minLength": 0, "maxLength": 16}),
        json!({"enum": ["a", "b"]}),
        json!({"enum": ["a", "b", "c"]}),
        json!({"type": "integer", "minimum": 5, "maximum": 10}),
        json!({"type": "integer", "minimum": 4, "maximum": 11}),
        json!({"multipleOf": 6}),
        json!({"multipleOf": 2}),
        json!({"type": "array", "items": {"type": "integer"}, "minItems": 1}),
        json!({"type": "array", "items": {"type": ["integer", "string"]}}),
        json!({"title": "annotated", "description": "free text", "default": 1}),
        json!({
            "properties": {
                "a": {"type": "integer"},
                "b": {"not": {"$ref": "#/properties/a"}}
            }
        }),
    ];
    let payloads: Vec<Value> = vec![
        json!(null),
        json!(true),
        json!(0),
        json!(1),
        json!(-3),
        json!(5),
        json!(6),
        json!(12),
        json!(2.5),
        json!(""),
        json!("a"),
        json!("c"),
        json!("A1"),
        json!([1, 2, 3]),
        json!([1, "x"]),
        json!([]),
        json!({}),
        json!({"sku": "A1"}),
        json!({"sku": "A1", "note": "x"}),
        json!({"sku": "A1", "note": 42}),
        json!({"sku": 7}),
        json!({"a": 1, "b": "text"}),
        json!({"extra": 1}),
    ];

    for predecessor in &schemas {
        for candidate in &schemas {
            if !check_compatible(predecessor, candidate).is_accepted() {
                continue;
            }
            for payload in &payloads {
                if accepts(predecessor, payload) {
                    assert!(
                        accepts(candidate, payload),
                        "accepted pair broke a previously valid payload\n\
                         predecessor: {predecessor}\ncandidate: {candidate}\npayload: {payload}"
                    );
                }
            }
        }
    }
}
