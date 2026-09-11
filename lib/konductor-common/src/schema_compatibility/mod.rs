//! Append-time backward-compatibility checking between schema versions.
//!
//! Admission validates against a cached latest schema version, which is only
//! sound if every version accepts everything its predecessor accepted (the
//! stale-acceptance argument in `docs/development/publish-validation.md`).
//! This module enforces that norm when a new version is appended:
//! [`check_compatible`] walks the current latest document and the candidate
//! side by side and reports every tightening it finds.
//!
//! The rules are syntactic and conservative: the candidate's accepted
//! instance set must visibly contain the predecessor's, and anything the
//! checker cannot prove — combinator edits, `$ref` or `definitions` changes,
//! regex containment, tuple-form `items` — rejects. Semantic subset checking
//! is a deliberate non-goal (tracked as issue #30). Intentional breaking
//! evolution is expressed as a new schema under a new event name, never as a
//! new version of an existing one.

mod rules;
mod structure;

use std::fmt;

use serde_json::Value;

/// One contract tightening in a candidate document: where it bites and which
/// rule it violates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incompatibility {
    /// Instance-relative location of the tightening: property names joined
    /// by `/` from the root (empty at the root, `~0`/`~1` escaped), with
    /// `[]` appended when descending into array `items`. A readable
    /// location, not a strict JSON Pointer.
    pub path: String,
    /// The violated compatibility rule.
    pub rule: CompatibilityRule,
}

/// The syntactic compatibility rule a candidate document violated. Variants
/// map one-to-one onto the draft-4 keyword rules, so a rejection message can
/// name both the path and the constraint kind without inspecting values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityRule {
    /// `required` gained an entry.
    NewlyRequiredProperty,
    /// `properties` lost a declared entry.
    RemovedProperty,
    /// `type` shrank (or was added where absent).
    NarrowedType,
    /// `enum` shrank (or was added where absent).
    ShrunkEnum,
    /// `minimum` rose, or its draft-4 `exclusiveMinimum` flag tightened.
    RaisedMinimum,
    /// `maximum` fell, or its draft-4 `exclusiveMaximum` flag tightened.
    LoweredMaximum,
    /// `minLength` rose above the predecessor's (or the implicit 0).
    RaisedMinLength,
    /// `maxLength` fell below the predecessor's (or was added).
    LoweredMaxLength,
    /// `minItems` rose above the predecessor's (or the implicit 0).
    RaisedMinItems,
    /// `maxItems` fell below the predecessor's (or was added).
    LoweredMaxItems,
    /// `minProperties` rose above the predecessor's (or the implicit 0).
    RaisedMinProperties,
    /// `maxProperties` fell below the predecessor's (or was added).
    LoweredMaxProperties,
    /// `multipleOf` was added or no longer divides the predecessor's.
    TightenedMultipleOf,
    /// `pattern` changed (regex containment is undecidable) or was added.
    ChangedPattern,
    /// `additionalProperties` moved towards fewer admitted properties.
    RestrictedAdditionalProperties,
    /// `uniqueItems` was newly asserted.
    AddedUniqueItems,
    /// A combinator (`allOf`/`anyOf`/`oneOf`/`not`) subtree changed.
    ChangedCombinator,
    /// The `$ref` target changed.
    ChangedRef,
    /// A `definitions` subtree changed (the referent of local `$ref`s).
    ChangedDefinitions,
    /// `items` changed in tuple form, or was added where absent.
    ChangedItems,
    /// `additionalItems` changed.
    ChangedAdditionalItems,
    /// `patternProperties` changed (pattern matching is unprovable).
    ChangedPatternProperties,
    /// `dependencies` changed.
    ChangedDependencies,
    /// Any other validation-affecting keyword changed or a subschema is not
    /// an object — unprovable under syntactic rules.
    UnverifiableChange,
}

impl fmt::Display for CompatibilityRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NewlyRequiredProperty => "property newly required",
            Self::RemovedProperty => "declared property removed",
            Self::NarrowedType => "type narrowed",
            Self::ShrunkEnum => "enum restricted",
            Self::RaisedMinimum => "minimum raised or made exclusive",
            Self::LoweredMaximum => "maximum lowered or made exclusive",
            Self::RaisedMinLength => "minLength raised",
            Self::LoweredMaxLength => "maxLength lowered",
            Self::RaisedMinItems => "minItems raised",
            Self::LoweredMaxItems => "maxItems lowered",
            Self::RaisedMinProperties => "minProperties raised",
            Self::LoweredMaxProperties => "maxProperties lowered",
            Self::TightenedMultipleOf => "multipleOf tightened",
            Self::ChangedPattern => "pattern changed",
            Self::RestrictedAdditionalProperties => "additionalProperties restricted",
            Self::AddedUniqueItems => "uniqueItems newly asserted",
            Self::ChangedCombinator => "combinator (allOf/anyOf/oneOf/not) changed",
            Self::ChangedRef => "$ref target changed",
            Self::ChangedDefinitions => "definitions changed",
            Self::ChangedItems => "items changed",
            Self::ChangedAdditionalItems => "additionalItems changed",
            Self::ChangedPatternProperties => "patternProperties changed",
            Self::ChangedDependencies => "dependencies changed",
            Self::UnverifiableChange => "change not provably compatible",
        };
        f.write_str(message)
    }
}

/// Checks that `candidate` accepts every instance `predecessor` accepts, as
/// far as syntactic draft-4 rules can prove it. An empty result means the
/// append is compatible; any entry is a tightening that must be rejected.
pub fn check_compatible(predecessor: &Value, candidate: &Value) -> Vec<Incompatibility> {
    let mut incompatibilities = Vec::new();
    check_schema(predecessor, candidate, "", &mut incompatibilities);
    incompatibilities
}

fn check_schema(
    predecessor: &Value,
    candidate: &Value,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    let (Some(pred), Some(cand)) = (predecessor.as_object(), candidate.as_object()) else {
        if predecessor != candidate {
            out.push(inc(path, CompatibilityRule::UnverifiableChange));
        }
        return;
    };

    structure::check_required(pred, cand, path, out);
    structure::check_properties(pred, cand, path, out);
    rules::check_type(pred, cand, path, out);
    rules::check_enum(pred, cand, path, out);
    rules::check_minimum(pred, cand, path, out);
    rules::check_maximum(pred, cand, path, out);
    rules::check_min(
        pred,
        cand,
        "minLength",
        CompatibilityRule::RaisedMinLength,
        path,
        out,
    );
    rules::check_max(
        pred,
        cand,
        "maxLength",
        CompatibilityRule::LoweredMaxLength,
        path,
        out,
    );
    rules::check_min(
        pred,
        cand,
        "minItems",
        CompatibilityRule::RaisedMinItems,
        path,
        out,
    );
    rules::check_max(
        pred,
        cand,
        "maxItems",
        CompatibilityRule::LoweredMaxItems,
        path,
        out,
    );
    rules::check_min(
        pred,
        cand,
        "minProperties",
        CompatibilityRule::RaisedMinProperties,
        path,
        out,
    );
    rules::check_max(
        pred,
        cand,
        "maxProperties",
        CompatibilityRule::LoweredMaxProperties,
        path,
        out,
    );
    rules::check_multiple_of(pred, cand, path, out);
    rules::check_pattern(pred, cand, path, out);
    rules::check_additional_properties(pred, cand, path, out);
    rules::check_unique_items(pred, cand, path, out);
    structure::check_items(pred, cand, path, out);
    structure::check_deep_equal(pred, cand, path, out);
    structure::check_unhandled_keywords(pred, cand, path, out);
}

fn inc(path: &str, rule: CompatibilityRule) -> Incompatibility {
    Incompatibility {
        path: path.to_string(),
        rule,
    }
}

fn child_path(path: &str, property: &str) -> String {
    let escaped = property.replace('~', "~0").replace('/', "~1");
    format!("{path}/{escaped}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rules(found: &[Incompatibility]) -> Vec<CompatibilityRule> {
        found.iter().map(|i| i.rule).collect()
    }

    #[test]
    fn identical_documents_are_compatible() {
        let schema = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        assert!(check_compatible(&schema, &schema).is_empty());
    }

    #[test]
    fn newly_required_property_rejects_at_its_instance_path() {
        let pred = json!({"properties": {"sku": {"type": "string"}}});
        let cand = json!({"properties": {"sku": {"type": "string"}}, "required": ["sku"]});
        let found = check_compatible(&pred, &cand);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, CompatibilityRule::NewlyRequiredProperty);
        assert_eq!(found[0].path, "/sku");
    }

    #[test]
    fn dropped_required_entry_passes() {
        let pred = json!({"required": ["sku"], "properties": {"sku": {}}});
        let cand = json!({"properties": {"sku": {}}});
        assert!(check_compatible(&pred, &cand).is_empty());
    }

    #[test]
    fn removed_declared_property_rejects() {
        let pred = json!({"properties": {"sku": {"type": "string"}, "qty": {"type": "integer"}}});
        let cand = json!({"properties": {"qty": {"type": "integer"}}});
        let found = check_compatible(&pred, &cand);
        assert_eq!(rules(&found), vec![CompatibilityRule::RemovedProperty]);
        assert_eq!(found[0].path, "/sku");
    }

    #[test]
    fn new_optional_property_passes() {
        let pred = json!({"properties": {"sku": {}}});
        let cand = json!({"properties": {"sku": {}, "note": {"type": "string"}}});
        assert!(check_compatible(&pred, &cand).is_empty());
    }

    #[test]
    fn nested_properties_recurse() {
        let pred = json!({"properties": {"order": {"properties": {"total": {"minimum": 0}}}}});
        let cand = json!({"properties": {"order": {"properties": {"total": {"minimum": 5}}}}});
        let found = check_compatible(&pred, &cand);
        assert_eq!(rules(&found), vec![CompatibilityRule::RaisedMinimum]);
        assert_eq!(found[0].path, "/order/total");
    }

    #[test]
    fn property_names_with_path_characters_are_escaped() {
        let pred = json!({"properties": {"a/b": {}}});
        let cand = json!({"required": ["a/b"], "properties": {"a/b": {}}});
        let found = check_compatible(&pred, &cand);
        assert_eq!(found[0].path, "/a~1b");
    }

    #[test]
    fn type_set_widening_passes_and_narrowing_rejects() {
        let pred = json!({"type": "string"});
        let wider = json!({"type": ["string", "null"]});
        assert!(check_compatible(&pred, &wider).is_empty());

        let narrower = json!({"type": "integer"});
        let found = check_compatible(&pred, &narrower);
        assert_eq!(rules(&found), vec![CompatibilityRule::NarrowedType]);

        let untyped = json!({"title": "no constraint"});
        assert!(check_compatible(&pred, &untyped).is_empty());

        let found = check_compatible(&untyped, &pred);
        assert_eq!(rules(&found), vec![CompatibilityRule::NarrowedType]);
    }

    #[test]
    fn enum_superset_passes_and_shrinkage_rejects() {
        let pred = json!({"enum": ["a", "b"]});
        let wider = json!({"enum": ["a", "b", "c"]});
        assert!(check_compatible(&pred, &wider).is_empty());

        let without_enum = json!({"title": "no enum constraint"});
        assert!(check_compatible(&pred, &without_enum).is_empty());

        let shrunk = json!({"enum": ["a"]});
        assert_eq!(
            rules(&check_compatible(&pred, &shrunk)),
            vec![CompatibilityRule::ShrunkEnum]
        );
        assert_eq!(
            rules(&check_compatible(&without_enum, &pred)),
            vec![CompatibilityRule::ShrunkEnum]
        );
    }

    #[test]
    fn enum_values_compare_deeply() {
        let pred = json!({"enum": [{"a": 1}, [1, 2]]});
        let same = json!({"enum": [[1, 2], {"a": 1}]});
        assert!(check_compatible(&pred, &same).is_empty());

        let different = json!({"enum": [{"a": 1}, [1, 2, 3]]});
        assert_eq!(
            rules(&check_compatible(&pred, &different)),
            vec![CompatibilityRule::ShrunkEnum]
        );
    }

    #[test]
    fn minimum_containment_uses_draft4_exclusive_boolean() {
        let pred = json!({"minimum": 5});
        assert!(check_compatible(&pred, &json!({"minimum": 4})).is_empty());
        assert!(check_compatible(&pred, &json!({})).is_empty());
        assert_eq!(
            rules(&check_compatible(&pred, &json!({"minimum": 6}))),
            vec![CompatibilityRule::RaisedMinimum]
        );
        assert_eq!(
            rules(&check_compatible(&json!({}), &pred)),
            vec![CompatibilityRule::RaisedMinimum]
        );

        let same_value_now_exclusive = json!({"minimum": 5, "exclusiveMinimum": true});
        assert_eq!(
            rules(&check_compatible(&pred, &same_value_now_exclusive)),
            vec![CompatibilityRule::RaisedMinimum]
        );
        assert!(check_compatible(&same_value_now_exclusive, &pred).is_empty());
    }

    #[test]
    fn maximum_containment_uses_draft4_exclusive_boolean() {
        let pred = json!({"maximum": 10});
        assert!(check_compatible(&pred, &json!({"maximum": 11})).is_empty());
        assert_eq!(
            rules(&check_compatible(&pred, &json!({"maximum": 9}))),
            vec![CompatibilityRule::LoweredMaximum]
        );

        let exclusive = json!({"maximum": 10, "exclusiveMaximum": true});
        assert_eq!(
            rules(&check_compatible(&pred, &exclusive)),
            vec![CompatibilityRule::LoweredMaximum]
        );
        assert!(check_compatible(&exclusive, &pred).is_empty());
    }

    #[test]
    fn length_and_item_bounds_check_containment() {
        let pred = json!({"minLength": 2, "maxLength": 8, "minItems": 1, "maxItems": 4});
        let looser = json!({"minLength": 0, "maxLength": 16, "maxItems": 10});
        assert!(check_compatible(&pred, &looser).is_empty());

        let raised_min_length =
            json!({"minLength": 3, "maxLength": 8, "minItems": 1, "maxItems": 4});
        assert_eq!(
            rules(&check_compatible(&pred, &raised_min_length)),
            vec![CompatibilityRule::RaisedMinLength]
        );
        let lowered_max_items =
            json!({"minLength": 2, "maxLength": 8, "minItems": 1, "maxItems": 3});
        assert_eq!(
            rules(&check_compatible(&pred, &lowered_max_items)),
            vec![CompatibilityRule::LoweredMaxItems]
        );

        let unconstrained = json!({"type": "string"});
        let bounded = json!({"maxLength": 5});
        assert_eq!(
            rules(&check_compatible(&unconstrained, &bounded)),
            vec![CompatibilityRule::LoweredMaxLength]
        );
    }

    #[test]
    fn min_length_zero_added_is_a_noop() {
        let pred = json!({"type": "array"});
        let cand = json!({"type": "array", "minItems": 0});
        assert!(check_compatible(&pred, &cand).is_empty());
    }

    #[test]
    fn multiple_of_must_divide_the_predecessor() {
        let pred = json!({"multipleOf": 6});
        assert!(check_compatible(&pred, &json!({"multipleOf": 2})).is_empty());
        assert!(check_compatible(&pred, &json!({})).is_empty());
        assert_eq!(
            rules(&check_compatible(&pred, &json!({"multipleOf": 4}))),
            vec![CompatibilityRule::TightenedMultipleOf]
        );
        assert_eq!(
            rules(&check_compatible(&json!({}), &pred)),
            vec![CompatibilityRule::TightenedMultipleOf]
        );
    }

    #[test]
    fn pattern_is_identical_or_removed_only() {
        let pred = json!({"pattern": "^[a-z]+$"});
        assert!(check_compatible(&pred, &json!({"pattern": "^[a-z]+$"})).is_empty());
        assert!(check_compatible(&pred, &json!({})).is_empty());

        let widened = json!({"pattern": "^[a-z0-9]+$"});
        assert_eq!(
            rules(&check_compatible(&pred, &widened)),
            vec![CompatibilityRule::ChangedPattern]
        );
        assert_eq!(
            rules(&check_compatible(&json!({}), &pred)),
            vec![CompatibilityRule::ChangedPattern]
        );
    }

    #[test]
    fn additional_properties_true_to_false_rejects() {
        let pred = json!({"properties": {"a": {}}, "additionalProperties": true});
        let cand = json!({"properties": {"a": {}}, "additionalProperties": false});
        assert_eq!(
            rules(&check_compatible(&pred, &cand)),
            vec![CompatibilityRule::RestrictedAdditionalProperties]
        );
        assert!(check_compatible(&cand, &pred).is_empty());

        let absent = json!({"properties": {"a": {}}});
        assert_eq!(
            rules(&check_compatible(&absent, &cand)),
            vec![CompatibilityRule::RestrictedAdditionalProperties]
        );

        let with_schema =
            json!({"properties": {"a": {}}, "additionalProperties": {"type": "string"}});
        assert_eq!(
            rules(&check_compatible(&absent, &with_schema)),
            vec![CompatibilityRule::RestrictedAdditionalProperties]
        );
        assert!(check_compatible(&with_schema, &absent).is_empty());
        assert!(check_compatible(&with_schema, &with_schema).is_empty());
        let other_schema =
            json!({"properties": {"a": {}}, "additionalProperties": {"type": "number"}});
        assert_eq!(
            rules(&check_compatible(&with_schema, &other_schema)),
            vec![CompatibilityRule::RestrictedAdditionalProperties]
        );
    }

    #[test]
    fn combinators_must_be_deep_equal() {
        let pred = json!({"allOf": [{"type": "string"}]});
        assert!(check_compatible(&pred, &pred).is_empty());
        assert_eq!(
            rules(&check_compatible(
                &pred,
                &json!({"allOf": [{"type": "integer"}]})
            )),
            vec![CompatibilityRule::ChangedCombinator]
        );
        assert_eq!(
            rules(&check_compatible(&pred, &json!({}))),
            vec![CompatibilityRule::ChangedCombinator]
        );
        assert_eq!(
            rules(&check_compatible(
                &json!({}),
                &json!({"not": {"type": "null"}})
            )),
            vec![CompatibilityRule::ChangedCombinator]
        );
    }

    #[test]
    fn ref_and_definitions_must_be_deep_equal() {
        let pred = json!({
            "$ref": "#/definitions/money",
            "definitions": {"money": {"type": "number", "minimum": 0}}
        });
        assert!(check_compatible(&pred, &pred).is_empty());

        let retargeted = json!({
            "$ref": "#/definitions/amount",
            "definitions": {"amount": {"type": "number", "minimum": 0}}
        });
        assert_eq!(
            rules(&check_compatible(&pred, &retargeted)),
            vec![
                CompatibilityRule::ChangedRef,
                CompatibilityRule::ChangedDefinitions
            ]
        );

        let tightened_definition = json!({
            "$ref": "#/definitions/money",
            "definitions": {"money": {"type": "number", "minimum": 5}}
        });
        assert_eq!(
            rules(&check_compatible(&pred, &tightened_definition)),
            vec![CompatibilityRule::ChangedDefinitions]
        );
    }

    #[test]
    fn object_items_recurse_and_tuple_items_reject_changes() {
        let pred = json!({"items": {"properties": {"sku": {}}}});
        let tightened_item = json!({"items": {"required": ["sku"], "properties": {"sku": {}}}});
        let found = check_compatible(&pred, &tightened_item);
        assert_eq!(
            rules(&found),
            vec![CompatibilityRule::NewlyRequiredProperty]
        );
        assert_eq!(found[0].path, "[]/sku");

        let tuple_pred = json!({"items": [{"type": "string"}, {"type": "integer"}]});
        assert!(check_compatible(&tuple_pred, &tuple_pred).is_empty());
        let tuple_cand = json!({"items": [{"type": "string"}]});
        assert_eq!(
            rules(&check_compatible(&tuple_pred, &tuple_cand)),
            vec![CompatibilityRule::ChangedItems]
        );
        assert_eq!(
            rules(&check_compatible(&json!({}), &pred)),
            vec![CompatibilityRule::ChangedItems]
        );
        assert!(check_compatible(&pred, &json!({})).is_empty());
    }

    #[test]
    fn unique_items_newly_asserted_rejects() {
        let pred = json!({"type": "array"});
        let cand = json!({"type": "array", "uniqueItems": true});
        assert_eq!(
            rules(&check_compatible(&pred, &cand)),
            vec![CompatibilityRule::AddedUniqueItems]
        );
        assert!(check_compatible(&cand, &pred).is_empty());
    }

    #[test]
    fn unhandled_keyword_changes_reject_but_annotations_do_not() {
        let pred = json!({"title": "old", "description": "first", "default": 1, "custom": true});
        let cand = json!({"title": "new", "description": "second", "default": 2, "custom": false});
        assert_eq!(
            rules(&check_compatible(&pred, &cand)),
            vec![CompatibilityRule::UnverifiableChange]
        );
        let annotations_only =
            json!({"title": "new", "description": "second", "default": 2, "custom": true});
        assert!(check_compatible(&pred, &annotations_only).is_empty());
    }

    #[test]
    fn non_object_subschemas_must_be_identical() {
        let pred = json!({"properties": {"flag": true}});
        let cand = json!({"properties": {"flag": false}});
        let found = check_compatible(&pred, &cand);
        assert_eq!(rules(&found), vec![CompatibilityRule::UnverifiableChange]);
        assert_eq!(found[0].path, "/flag");
    }
}
