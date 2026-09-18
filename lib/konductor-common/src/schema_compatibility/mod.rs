//! Append-time backward-compatibility checking between schema versions.
//!
//! Admission validates against a cached latest schema version, which is only
//! sound if every version accepts everything its predecessor accepted (the
//! stale-acceptance argument in `docs/development/publish-validation.md`).
//! This module enforces that norm when a new version is appended:
//! [`check_compatible`] attempts a bounded syntactic proof that the
//! candidate's accepted instance set contains the predecessor's.
//!
//! The result is explicit: [`CompatibilityCheck::Accepted`] only when the
//! proof established inclusion, otherwise
//! [`CompatibilityCheck::NotEstablished`] with path-bearing reasons that
//! distinguish observed local restrictions from changes no proof rule
//! covers. Anything the checker cannot prove — reference-dependent changes,
//! combinator edits, regex containment, pattern-property interaction,
//! inexact numeric comparison — rejects. Rejection never claims a
//! demonstrated breaking payload; it means this bounded proof could not
//! establish inclusion. Semantic subset checking beyond this proof domain
//! is a deliberate non-goal (tracked as issue #30). Intentional breaking
//! evolution is expressed as a new schema under a new event name, never as
//! a new version of an existing one.

mod rules;
mod structure;

#[cfg(test)]
mod regression_tests;

use std::fmt;

use serde_json::Value;

type SchemaObject = serde_json::Map<String, Value>;

/// The outcome of comparing a candidate document against the predecessor
/// version it would succeed.
#[derive(Debug)]
pub enum CompatibilityCheck {
    /// Every instance the predecessor accepts is provably accepted by the
    /// candidate under the supported comparison rules and work budget.
    Accepted,
    /// Inclusion could not be proven. The reasons mix observed local
    /// restrictions with changes no proof rule covers; neither kind claims
    /// a demonstrated breaking payload.
    NotEstablished(Vec<Incompatibility>),
}

impl CompatibilityCheck {
    /// True only when the proof established inclusion.
    pub fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted)
    }
}

/// One reason compatibility was not established: where it bites and which
/// rule produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incompatibility {
    /// Instance-relative location: property names joined by `/` from the
    /// root (empty at the root, `~0`/`~1` escaped), `[]` appended when
    /// descending into array `items`, and `/*` naming the schema that
    /// governs additional properties. A readable location, not a strict
    /// JSON Pointer.
    pub path: String,
    /// The rule that produced this reason.
    pub rule: CompatibilityRule,
}

/// Whether a rejection reason reports an observed constraint restriction
/// or the absence of an applicable proof rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionKind {
    /// A local constraint visibly tightened (e.g. `minimum` rose).
    LocalRestriction,
    /// The available proof rules cannot establish inclusion for this
    /// change; compatibility is unknown, not disproven.
    UnsupportedProof,
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
    /// A candidate-only property's constraint is not proven to accept every
    /// value the predecessor's additional-property policy allowed for that
    /// name.
    NewPropertyConstraintUnproven,
    /// The document changed while carrying schema-bearing `$ref`s, whose
    /// meaning depends on the referenced targets.
    UnsupportedReferenceChange,
    /// Numeric constraints could not be compared without lossy conversion.
    UnprovenNumericComparison,
    /// The comparison exceeded its depth, node-visit, or step budget.
    ProofBudgetExhausted,
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
            Self::NewPropertyConstraintUnproven => {
                "new property constraint is not proven to accept previously allowed values"
            }
            Self::UnsupportedReferenceChange => "reference-dependent change is unsupported",
            Self::UnprovenNumericComparison => {
                "numeric constraint change cannot be compared exactly"
            }
            Self::ProofBudgetExhausted => "compatibility proof exceeded its work budget",
            Self::UnverifiableChange => "change not provably compatible",
        };
        f.write_str(message)
    }
}

impl CompatibilityRule {
    /// Whether the reason reports an observed local restriction or the
    /// absence of an applicable proof rule.
    pub fn kind(&self) -> RejectionKind {
        match self {
            Self::NewlyRequiredProperty
            | Self::NarrowedType
            | Self::ShrunkEnum
            | Self::RaisedMinimum
            | Self::LoweredMaximum
            | Self::RaisedMinLength
            | Self::LoweredMaxLength
            | Self::RaisedMinItems
            | Self::LoweredMaxItems
            | Self::RaisedMinProperties
            | Self::LoweredMaxProperties
            | Self::TightenedMultipleOf
            | Self::RestrictedAdditionalProperties
            | Self::AddedUniqueItems => RejectionKind::LocalRestriction,
            Self::RemovedProperty
            | Self::ChangedPattern
            | Self::ChangedCombinator
            | Self::ChangedRef
            | Self::ChangedDefinitions
            | Self::ChangedItems
            | Self::ChangedAdditionalItems
            | Self::ChangedPatternProperties
            | Self::ChangedDependencies
            | Self::NewPropertyConstraintUnproven
            | Self::UnsupportedReferenceChange
            | Self::UnprovenNumericComparison
            | Self::ProofBudgetExhausted
            | Self::UnverifiableChange => RejectionKind::UnsupportedProof,
        }
    }
}

/// Depth limit for one pairwise proof: schema nesting deeper than this
/// fails closed rather than risking unbounded recursion.
const MAX_DEPTH: usize = 64;
/// Node-visit limit for one pairwise proof, shared by the reference scan
/// and the comparison traversal.
const MAX_NODES: usize = 10_000;
/// Step limit for collection work (enum membership, subtree equality)
/// within one pairwise proof.
const MAX_STEPS: u64 = 100_000;

/// The deterministic work budget shared by one pairwise proof. Exhaustion
/// is sticky: once tripped, every later charge fails, so no truncated proof
/// can pass as acceptance.
struct Budget {
    nodes_left: usize,
    steps_left: u64,
    exhausted: bool,
}

impl Budget {
    fn new() -> Self {
        Self {
            nodes_left: MAX_NODES,
            steps_left: MAX_STEPS,
            exhausted: false,
        }
    }

    /// Charges one schema-node visit at `depth`; false once the depth or
    /// node budget cannot cover it.
    fn visit(&mut self, depth: usize) -> bool {
        if self.exhausted || depth >= MAX_DEPTH || self.nodes_left == 0 {
            self.exhausted = true;
            return false;
        }
        self.nodes_left -= 1;
        true
    }

    /// Charges `steps` of collection work; false once the step budget
    /// cannot cover the full comparison.
    fn charge(&mut self, steps: u64) -> bool {
        if self.exhausted || self.steps_left < steps {
            self.exhausted = true;
            return false;
        }
        self.steps_left -= steps;
        true
    }
}

/// Attempts a bounded syntactic proof that `candidate` accepts every
/// instance `predecessor` accepts under the application's draft-4
/// validation semantics.
///
/// Identical documents are accepted outright — including documents with
/// references, whose meaning is fixed while the document is unchanged; the
/// equality scan is bounded by document size, which parsing already paid.
/// Any other pair containing a schema-bearing `$ref` is rejected as
/// unsupported: a reference's meaning depends on its target, so local
/// keyword reasoning cannot prove inclusion (a loosened target tightens
/// under `not`).
pub fn check_compatible(predecessor: &Value, candidate: &Value) -> CompatibilityCheck {
    if predecessor == candidate {
        return CompatibilityCheck::Accepted;
    }
    let mut budget = Budget::new();
    let mut reasons = Vec::new();

    let referenced = contains_schema_reference(predecessor, 0, &mut budget)
        || contains_schema_reference(candidate, 0, &mut budget);
    if budget.exhausted {
        reasons.push(inc("", CompatibilityRule::ProofBudgetExhausted));
    } else if referenced {
        reasons.push(inc("", CompatibilityRule::UnsupportedReferenceChange));
    } else {
        check_schema(predecessor, candidate, "", 0, &mut budget, &mut reasons);
    }

    if reasons.is_empty() {
        CompatibilityCheck::Accepted
    } else {
        CompatibilityCheck::NotEstablished(reasons)
    }
}

/// True when a schema-bearing `$ref` occurs anywhere in the document.
/// Only known schema positions are traversed, so literal `$ref` keys
/// inside instance data (`enum`, `default`) never count as references.
fn contains_schema_reference(value: &Value, depth: usize, budget: &mut Budget) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if !budget.visit(depth) {
        return false;
    }
    if object.contains_key("$ref") {
        return true;
    }
    child_schemas(object)
        .into_iter()
        .any(|child| contains_schema_reference(child, depth + 1, budget))
}

/// The keyword values of a draft-4 schema object that are themselves
/// schemas: named/definition/pattern subschemas, combinator branches,
/// `items` (list or schema form), schema-valued `additional*`, `not`, and
/// schema-form `dependencies`. Unknown keywords are not traversed; the
/// comparison's unhandled-keyword rule fails closed on their changes.
fn child_schemas(object: &SchemaObject) -> Vec<&Value> {
    let mut children = Vec::new();
    for keyword in ["properties", "patternProperties", "definitions"] {
        if let Some(map) = object.get(keyword).and_then(Value::as_object) {
            children.extend(map.values());
        }
    }
    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(list) = object.get(keyword).and_then(Value::as_array) {
            children.extend(list.iter());
        }
    }
    for keyword in ["additionalProperties", "additionalItems", "not"] {
        if let Some(sub) = object.get(keyword).filter(|v| v.is_object()) {
            children.push(sub);
        }
    }
    match object.get("items") {
        Some(Value::Array(list)) => children.extend(list.iter()),
        Some(sub @ Value::Object(_)) => children.push(sub),
        _ => {}
    }
    if let Some(dependencies) = object.get("dependencies").and_then(Value::as_object) {
        children.extend(dependencies.values().filter(|v| v.is_object()));
    }
    children
}

fn check_schema(
    predecessor: &Value,
    candidate: &Value,
    path: &str,
    depth: usize,
    budget: &mut Budget,
    out: &mut Vec<Incompatibility>,
) {
    if budget.exhausted {
        return;
    }
    // Identical subschemas admit every instance the predecessor admitted;
    // re-proving them would only spend budget on repeated structure.
    if predecessor == candidate {
        return;
    }
    if !budget.visit(depth) {
        out.push(inc(path, CompatibilityRule::ProofBudgetExhausted));
        return;
    }
    let (Some(pred), Some(cand)) = (predecessor.as_object(), candidate.as_object()) else {
        if charge_equality(predecessor, candidate, path, budget, out) && predecessor != candidate {
            out.push(inc(path, CompatibilityRule::UnverifiableChange));
        }
        return;
    };

    structure::check_required(pred, cand, path, out);
    structure::check_properties(pred, cand, path, depth, budget, out);
    if budget.exhausted {
        return;
    }
    rules::check_type(pred, cand, path, out);
    rules::check_enum(pred, cand, path, budget, out);
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
    rules::check_additional_properties(pred, cand, path, depth, budget, out);
    rules::check_unique_items(pred, cand, path, out);
    structure::check_items(pred, cand, path, depth, budget, out);
    structure::check_deep_equal(pred, cand, path, budget, out);
    structure::check_unhandled_keywords(pred, cand, path, budget, out);
}

/// Charges the deep-equality comparison of two subtrees against the step
/// budget; records budget exhaustion and returns false when the
/// comparison cannot be afforded, so the caller fails closed.
fn charge_equality(
    a: &Value,
    b: &Value,
    path: &str,
    budget: &mut Budget,
    out: &mut Vec<Incompatibility>,
) -> bool {
    let Some(cost) = subtree_size(a, budget.steps_left)
        .and_then(|x| subtree_size(b, budget.steps_left).map(|y| x + y))
    else {
        budget.exhausted = true;
        out.push(inc(path, CompatibilityRule::ProofBudgetExhausted));
        return false;
    };
    if !budget.charge(cost) {
        out.push(inc(path, CompatibilityRule::ProofBudgetExhausted));
        return false;
    }
    true
}

/// Counts value nodes, giving up past `limit` so the count itself stays
/// bounded by the remaining step budget.
fn subtree_size(value: &Value, limit: u64) -> Option<u64> {
    let mut stack = vec![value];
    let mut count = 0u64;
    while let Some(current) = stack.pop() {
        count += 1;
        if count > limit {
            return None;
        }
        match current {
            Value::Array(items) => stack.extend(items),
            Value::Object(map) => stack.extend(map.values()),
            _ => {}
        }
    }
    Some(count)
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

    fn rejections(found: &CompatibilityCheck) -> &[Incompatibility] {
        match found {
            CompatibilityCheck::NotEstablished(reasons) => reasons,
            CompatibilityCheck::Accepted => panic!("expected a rejected comparison"),
        }
    }

    fn rules(found: &CompatibilityCheck) -> Vec<CompatibilityRule> {
        match found {
            CompatibilityCheck::Accepted => Vec::new(),
            CompatibilityCheck::NotEstablished(reasons) => reasons.iter().map(|i| i.rule).collect(),
        }
    }

    #[test]
    fn identical_documents_are_compatible() {
        let schema = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        assert!(check_compatible(&schema, &schema).is_accepted());
    }

    #[test]
    fn newly_required_property_rejects_at_its_instance_path() {
        let pred = json!({"properties": {"sku": {"type": "string"}}});
        let cand = json!({"properties": {"sku": {"type": "string"}}, "required": ["sku"]});
        let found = check_compatible(&pred, &cand);
        let reasons = rejections(&found);
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].rule, CompatibilityRule::NewlyRequiredProperty);
        assert_eq!(reasons[0].path, "/sku");
    }

    #[test]
    fn dropped_required_entry_passes() {
        let pred = json!({"required": ["sku"], "properties": {"sku": {}}});
        let cand = json!({"properties": {"sku": {}}});
        assert!(check_compatible(&pred, &cand).is_accepted());
    }

    #[test]
    fn removed_declared_property_rejects() {
        let pred = json!({"properties": {"sku": {"type": "string"}, "qty": {"type": "integer"}}});
        let cand = json!({"properties": {"qty": {"type": "integer"}}});
        let found = check_compatible(&pred, &cand);
        assert_eq!(rules(&found), vec![CompatibilityRule::RemovedProperty]);
        assert_eq!(rejections(&found)[0].path, "/sku");
    }

    #[test]
    fn new_optional_property_in_an_open_object_rejects() {
        let pred = json!({"properties": {"sku": {}}});
        let cand = json!({"properties": {"sku": {}, "note": {"type": "string"}}});
        let found = check_compatible(&pred, &cand);
        let reasons = rejections(&found);
        assert_eq!(reasons.len(), 1);
        assert_eq!(
            reasons[0].rule,
            CompatibilityRule::NewPropertyConstraintUnproven
        );
        assert_eq!(reasons[0].path, "/note");
    }

    #[test]
    fn new_unconstrained_property_in_an_open_object_passes() {
        let pred = json!({"properties": {"sku": {}}});
        let bare = json!({"properties": {"sku": {}, "note": {}}});
        assert!(check_compatible(&pred, &bare).is_accepted());
        let annotated = json!({"properties": {"sku": {}, "note": {"title": "free text"}}});
        assert!(check_compatible(&pred, &annotated).is_accepted());
    }

    #[test]
    fn new_optional_property_in_a_closed_object_passes() {
        let pred = json!({"properties": {"sku": {}}, "additionalProperties": false});
        let cand = json!({
            "properties": {"sku": {}, "note": {"type": "string"}},
            "additionalProperties": false
        });
        assert!(check_compatible(&pred, &cand).is_accepted());
    }

    #[test]
    fn new_property_against_schema_valued_additional_properties_recurses() {
        let pred = json!({"properties": {"sku": {}}, "additionalProperties": {"type": "number"}});
        let constrained = json!({
            "properties": {"sku": {}, "note": {"type": "string"}},
            "additionalProperties": {"type": "number"}
        });
        let found = check_compatible(&pred, &constrained);
        assert_eq!(rules(&found), vec![CompatibilityRule::NarrowedType]);
        assert_eq!(rejections(&found)[0].path, "/note");

        let matching = json!({
            "properties": {"sku": {}, "note": {"type": "number"}},
            "additionalProperties": {"type": "number"}
        });
        assert!(check_compatible(&pred, &matching).is_accepted());
    }

    #[test]
    fn new_property_with_pattern_properties_is_unproven() {
        let pred = json!({"properties": {"sku": {}}, "patternProperties": {"^x-": {}}});
        let cand = json!({
            "properties": {"sku": {}, "note": {}},
            "patternProperties": {"^x-": {}}
        });
        let found = check_compatible(&pred, &cand);
        assert_eq!(
            rules(&found),
            vec![CompatibilityRule::NewPropertyConstraintUnproven]
        );
        assert_eq!(rejections(&found)[0].path, "/note");
    }

    #[test]
    fn nested_properties_recurse() {
        let pred = json!({"properties": {"order": {"properties": {"total": {"minimum": 0}}}}});
        let cand = json!({"properties": {"order": {"properties": {"total": {"minimum": 5}}}}});
        let found = check_compatible(&pred, &cand);
        assert_eq!(rules(&found), vec![CompatibilityRule::RaisedMinimum]);
        assert_eq!(rejections(&found)[0].path, "/order/total");
    }

    #[test]
    fn property_names_with_path_characters_are_escaped() {
        let pred = json!({"properties": {"a/b": {}}});
        let cand = json!({"required": ["a/b"], "properties": {"a/b": {}}});
        let found = check_compatible(&pred, &cand);
        assert_eq!(rejections(&found)[0].path, "/a~1b");
    }

    #[test]
    fn type_set_widening_passes_and_narrowing_rejects() {
        let pred = json!({"type": "string"});
        let wider = json!({"type": ["string", "null"]});
        assert!(check_compatible(&pred, &wider).is_accepted());

        let narrower = json!({"type": "integer"});
        let found = check_compatible(&pred, &narrower);
        assert_eq!(rules(&found), vec![CompatibilityRule::NarrowedType]);

        let untyped = json!({"title": "no constraint"});
        assert!(check_compatible(&pred, &untyped).is_accepted());

        let found = check_compatible(&untyped, &pred);
        assert_eq!(rules(&found), vec![CompatibilityRule::NarrowedType]);
    }

    #[test]
    fn enum_superset_passes_and_shrinkage_rejects() {
        let pred = json!({"enum": ["a", "b"]});
        let wider = json!({"enum": ["a", "b", "c"]});
        assert!(check_compatible(&pred, &wider).is_accepted());

        let without_enum = json!({"title": "no enum constraint"});
        assert!(check_compatible(&pred, &without_enum).is_accepted());

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
        assert!(check_compatible(&pred, &same).is_accepted());

        let different = json!({"enum": [{"a": 1}, [1, 2, 3]]});
        assert_eq!(
            rules(&check_compatible(&pred, &different)),
            vec![CompatibilityRule::ShrunkEnum]
        );
    }

    #[test]
    fn minimum_containment_uses_draft4_exclusive_boolean() {
        let pred = json!({"minimum": 5});
        assert!(check_compatible(&pred, &json!({"minimum": 4})).is_accepted());
        assert!(check_compatible(&pred, &json!({})).is_accepted());
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
        assert!(check_compatible(&same_value_now_exclusive, &pred).is_accepted());
    }

    #[test]
    fn maximum_containment_uses_draft4_exclusive_boolean() {
        let pred = json!({"maximum": 10});
        assert!(check_compatible(&pred, &json!({"maximum": 11})).is_accepted());
        assert_eq!(
            rules(&check_compatible(&pred, &json!({"maximum": 9}))),
            vec![CompatibilityRule::LoweredMaximum]
        );

        let exclusive = json!({"maximum": 10, "exclusiveMaximum": true});
        assert_eq!(
            rules(&check_compatible(&pred, &exclusive)),
            vec![CompatibilityRule::LoweredMaximum]
        );
        assert!(check_compatible(&exclusive, &pred).is_accepted());
    }

    #[test]
    fn numeric_bounds_compare_exactly_beyond_two_pow_53() {
        let pred = json!({"maximum": 9_007_199_254_740_993_i64});
        assert!(
            check_compatible(&pred, &json!({"maximum": 9_007_199_254_740_994_i64})).is_accepted()
        );
        // 2^53+1 collapses to 2^53 as f64; the lowered maximum must still
        // be caught.
        assert_eq!(
            rules(&check_compatible(
                &pred,
                &json!({"maximum": 9_007_199_254_740_992_i64})
            )),
            vec![CompatibilityRule::LoweredMaximum]
        );
    }

    #[test]
    fn signed_and_unsigned_bounds_compare_exactly() {
        let pred = json!({"minimum": -9_007_199_254_740_993_i64});
        assert!(
            check_compatible(&pred, &json!({"minimum": -9_007_199_254_740_994_i64})).is_accepted()
        );
        assert_eq!(
            rules(&check_compatible(&pred, &json!({"minimum": 0}))),
            vec![CompatibilityRule::RaisedMinimum]
        );

        let upper = json!({"maximum": 18_446_744_073_709_551_615_u64});
        assert_eq!(
            rules(&check_compatible(&upper, &json!({"maximum": -1}))),
            vec![CompatibilityRule::LoweredMaximum]
        );
    }

    #[test]
    fn distinct_fractional_bounds_are_unproven() {
        let pred = json!({"minimum": 5.5});
        // A real loosening, but comparing distinct fractional values is
        // outside the exact proof domain, so it fails closed.
        assert_eq!(
            rules(&check_compatible(&pred, &json!({"minimum": 5.4}))),
            vec![CompatibilityRule::UnprovenNumericComparison]
        );
        assert!(check_compatible(&pred, &pred).is_accepted());
        assert!(check_compatible(&pred, &json!({})).is_accepted());
        assert_eq!(
            rules(&check_compatible(&json!({}), &pred)),
            vec![CompatibilityRule::RaisedMinimum]
        );
    }

    #[test]
    fn length_and_item_bounds_check_containment() {
        let pred = json!({"minLength": 2, "maxLength": 8, "minItems": 1, "maxItems": 4});
        let looser = json!({"minLength": 0, "maxLength": 16, "maxItems": 10});
        assert!(check_compatible(&pred, &looser).is_accepted());

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
        assert!(check_compatible(&pred, &cand).is_accepted());
    }

    #[test]
    fn multiple_of_must_divide_the_predecessor() {
        let pred = json!({"multipleOf": 6});
        assert!(check_compatible(&pred, &json!({"multipleOf": 2})).is_accepted());
        assert!(check_compatible(&pred, &json!({})).is_accepted());
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
    fn multiple_of_compares_exactly_beyond_two_pow_53() {
        // 2^55+1 rounds to 2^55 as f64, where the old rounded-division
        // check saw divisibility by 2.
        let pred = json!({"multipleOf": 36_028_797_018_963_969_i64});
        assert_eq!(
            rules(&check_compatible(&pred, &json!({"multipleOf": 2}))),
            vec![CompatibilityRule::TightenedMultipleOf]
        );
        let double = json!({"multipleOf": 72_057_594_037_927_938_i64});
        assert!(check_compatible(&double, &pred).is_accepted());
    }

    #[test]
    fn fractional_multiple_of_is_unproven() {
        let pred = json!({"multipleOf": 0.5});
        assert_eq!(
            rules(&check_compatible(&pred, &json!({"multipleOf": 0.25}))),
            vec![CompatibilityRule::UnprovenNumericComparison]
        );
        assert!(check_compatible(&pred, &json!({})).is_accepted());
        assert_eq!(
            rules(&check_compatible(&json!({}), &pred)),
            vec![CompatibilityRule::TightenedMultipleOf]
        );
    }

    #[test]
    fn pattern_is_identical_or_removed_only() {
        let pred = json!({"pattern": "^[a-z]+$"});
        assert!(check_compatible(&pred, &json!({"pattern": "^[a-z]+$"})).is_accepted());
        assert!(check_compatible(&pred, &json!({})).is_accepted());

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
        assert!(check_compatible(&cand, &pred).is_accepted());

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
        assert!(check_compatible(&with_schema, &absent).is_accepted());
        assert!(check_compatible(&with_schema, &with_schema).is_accepted());
    }

    #[test]
    fn additional_properties_schema_changes_recurse() {
        let with_schema =
            json!({"properties": {"a": {}}, "additionalProperties": {"type": "string"}});
        let other_schema =
            json!({"properties": {"a": {}}, "additionalProperties": {"type": "number"}});
        let found = check_compatible(&with_schema, &other_schema);
        assert_eq!(rules(&found), vec![CompatibilityRule::NarrowedType]);
        assert_eq!(rejections(&found)[0].path, "/*");

        let loosened = json!({"properties": {"a": {}}, "additionalProperties": {}});
        assert!(check_compatible(&with_schema, &loosened).is_accepted());
    }

    #[test]
    fn combinators_must_be_deep_equal() {
        let pred = json!({"allOf": [{"type": "string"}]});
        assert!(check_compatible(&pred, &pred).is_accepted());
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
    fn changed_documents_with_references_are_unsupported() {
        let pred = json!({
            "$ref": "#/definitions/money",
            "definitions": {"money": {"type": "number", "minimum": 0}}
        });
        assert!(check_compatible(&pred, &pred).is_accepted());

        // Even a pure loosening of the referenced target rejects: the
        // reference guard runs before any local rule.
        let loosened = json!({
            "$ref": "#/definitions/money",
            "definitions": {"money": {"type": "number"}}
        });
        let found = check_compatible(&pred, &loosened);
        let reasons = rejections(&found);
        assert_eq!(reasons.len(), 1);
        assert_eq!(
            reasons[0].rule,
            CompatibilityRule::UnsupportedReferenceChange
        );
        assert_eq!(reasons[0].path, "");
    }

    #[test]
    fn literal_ref_keys_in_instance_data_do_not_trigger_the_reference_guard() {
        let pred = json!({"default": {"$ref": "#/somewhere"}, "title": "a"});
        let cand = json!({"default": {"$ref": "#/elsewhere"}, "title": "b"});
        assert!(check_compatible(&pred, &cand).is_accepted());

        let pred_enum = json!({"enum": [{"$ref": "a"}]});
        let cand_enum = json!({"enum": [{"$ref": "a"}, {"$ref": "b"}]});
        assert!(check_compatible(&pred_enum, &cand_enum).is_accepted());
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
        assert_eq!(rejections(&found)[0].path, "[]/sku");

        let tuple_pred = json!({"items": [{"type": "string"}, {"type": "integer"}]});
        assert!(check_compatible(&tuple_pred, &tuple_pred).is_accepted());
        let tuple_cand = json!({"items": [{"type": "string"}]});
        assert_eq!(
            rules(&check_compatible(&tuple_pred, &tuple_cand)),
            vec![CompatibilityRule::ChangedItems]
        );

        // Adding items compares against the empty schema: an
        // all-loosening items schema restricts nothing, a constraining
        // one names the rule.
        assert!(check_compatible(&json!({}), &pred).is_accepted());
        let constraining = json!({"items": {"type": "integer"}});
        let found = check_compatible(&json!({}), &constraining);
        assert_eq!(rules(&found), vec![CompatibilityRule::NarrowedType]);
        assert_eq!(rejections(&found)[0].path, "[]");

        assert!(check_compatible(&pred, &json!({})).is_accepted());
    }

    #[test]
    fn unique_items_newly_asserted_rejects() {
        let pred = json!({"type": "array"});
        let cand = json!({"type": "array", "uniqueItems": true});
        assert_eq!(
            rules(&check_compatible(&pred, &cand)),
            vec![CompatibilityRule::AddedUniqueItems]
        );
        assert!(check_compatible(&cand, &pred).is_accepted());
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
        assert!(check_compatible(&pred, &annotations_only).is_accepted());
    }

    #[test]
    fn id_and_format_changes_are_not_free_annotations() {
        // id re-scopes reference resolution and the validator enforces
        // format, so neither may drift unchecked inside an otherwise
        // unchanged reference-free document.
        let pred = json!({"id": "#a", "format": "date-time", "type": "string"});
        let cand = json!({"id": "#b", "format": "date-time", "type": "string"});
        assert_eq!(
            rules(&check_compatible(&pred, &cand)),
            vec![CompatibilityRule::UnverifiableChange]
        );
        let cand_format = json!({"id": "#a", "format": "email", "type": "string"});
        assert_eq!(
            rules(&check_compatible(&pred, &cand_format)),
            vec![CompatibilityRule::UnverifiableChange]
        );
    }

    #[test]
    fn non_object_subschemas_must_be_identical() {
        let pred = json!({"properties": {"flag": true}});
        let cand = json!({"properties": {"flag": false}});
        let found = check_compatible(&pred, &cand);
        assert_eq!(rules(&found), vec![CompatibilityRule::UnverifiableChange]);
        assert_eq!(rejections(&found)[0].path, "/flag");
    }

    fn nested(depth: usize, leaf: Value) -> Value {
        let mut schema = leaf;
        for _ in 0..depth {
            schema = json!({"properties": {"a": schema}});
        }
        schema
    }

    #[test]
    fn deeply_nested_documents_exhaust_the_budget_and_fail_closed() {
        let pred = nested(70, json!({}));
        let cand = nested(70, json!({"minLength": 1}));
        let found = check_compatible(&pred, &cand);
        assert!(
            rejections(&found)
                .iter()
                .any(|i| i.rule == CompatibilityRule::ProofBudgetExhausted),
            "depth exhaustion must fail closed: {found:?}"
        );
    }

    #[test]
    fn repeated_identical_subschemas_do_not_consume_the_budget() {
        let mut shared = serde_json::Map::new();
        for i in 0..200_u64 {
            shared.insert(format!("v{i}"), json!(format!("value-{i}")));
        }
        let shared = json!({"enum": Value::Array(shared.into_values().collect::<Vec<_>>())});
        let build = |leaf: Value| {
            let mut properties = serde_json::Map::new();
            properties.insert("changed".to_string(), leaf);
            for i in 0..500 {
                properties.insert(format!("p{i}"), shared.clone());
            }
            json!({"properties": properties})
        };
        let pred = build(json!({"minimum": 5}));
        let cand = build(json!({"minimum": 4}));
        assert!(
            check_compatible(&pred, &cand).is_accepted(),
            "500 identical 200-value enum subschemas must not exhaust the budget"
        );
    }

    #[test]
    fn wide_documents_exhaust_the_budget_and_fail_closed() {
        // Each property loosens by a distinct annotation, so no pair takes
        // the identical-subschema short-circuit and every visit charges.
        let wide = |offset: u64, leaf: Value| {
            let mut properties = serde_json::Map::new();
            properties.insert("last".to_string(), leaf);
            for i in 0..6_000_u64 {
                properties.insert(format!("p{i}"), json!({"default": i + offset}));
            }
            json!({"properties": properties})
        };
        let pred = wide(0, json!({}));
        let cand = wide(10_000, json!({"minLength": 1}));
        let found = check_compatible(&pred, &cand);
        assert!(
            rejections(&found)
                .iter()
                .any(|i| i.rule == CompatibilityRule::ProofBudgetExhausted),
            "node-visit exhaustion must fail closed: {found:?}"
        );
    }

    #[test]
    fn large_enum_comparisons_exhaust_the_step_budget() {
        let pred_values: Vec<Value> = (0..400).map(|i| json!(format!("v{i}"))).collect();
        let mut cand_values = pred_values.clone();
        cand_values.push(json!("v400"));
        let pred = json!({"enum": pred_values});
        let cand = json!({"enum": cand_values});
        // 400 * 401 membership comparisons exceed the step budget: a
        // widening this large fails closed rather than truncating the
        // proof.
        assert_eq!(
            rules(&check_compatible(&pred, &cand)),
            vec![CompatibilityRule::ProofBudgetExhausted]
        );

        let pred_values: Vec<Value> = (0..300).map(|i| json!(format!("v{i}"))).collect();
        let mut cand_values = pred_values.clone();
        cand_values.push(json!("v300"));
        let pred = json!({"enum": pred_values});
        let cand = json!({"enum": cand_values});
        assert!(check_compatible(&pred, &cand).is_accepted());
    }

    #[test]
    fn rejection_kinds_distinguish_restrictions_from_unsupported_proofs() {
        assert_eq!(
            CompatibilityRule::RaisedMinimum.kind(),
            RejectionKind::LocalRestriction
        );
        assert_eq!(
            CompatibilityRule::NewlyRequiredProperty.kind(),
            RejectionKind::LocalRestriction
        );
        assert_eq!(
            CompatibilityRule::RemovedProperty.kind(),
            RejectionKind::UnsupportedProof
        );
        assert_eq!(
            CompatibilityRule::NewPropertyConstraintUnproven.kind(),
            RejectionKind::UnsupportedProof
        );
        assert_eq!(
            CompatibilityRule::UnsupportedReferenceChange.kind(),
            RejectionKind::UnsupportedProof
        );
        assert_eq!(
            CompatibilityRule::UnprovenNumericComparison.kind(),
            RejectionKind::UnsupportedProof
        );
        assert_eq!(
            CompatibilityRule::ProofBudgetExhausted.kind(),
            RejectionKind::UnsupportedProof
        );
    }
}
