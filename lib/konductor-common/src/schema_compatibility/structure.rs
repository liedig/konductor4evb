//! Structural compatibility rules: the keyword families that walk or reshape
//! the document tree (`required`, `properties`, `items`, combinator-style
//! deep-equality sweeps, and the catch-all for unhandled keywords).

use std::collections::BTreeSet;

use serde_json::Value;

use super::rules::{additional_properties, AdditionalProperties};
use super::{
    charge_equality, check_schema, child_path, inc, Budget, CompatibilityRule, Incompatibility,
};

type SchemaObject = serde_json::Map<String, Value>;

/// Draft-4 keywords with no validation effect under the application's
/// configuration. `id` is deliberately absent: it re-scopes reference
/// resolution. `format` is deliberately absent: the project's validator
/// build enforces it.
const ANNOTATIONS: [&str; 8] = [
    "$schema",
    "title",
    "description",
    "default",
    "readOnly",
    "media",
    "links",
    "pathStart",
];

pub(super) fn required_set(schema: &SchemaObject) -> BTreeSet<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

pub(super) fn check_required(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    for newly in required_set(cand).difference(&required_set(pred)) {
        out.push(inc(
            &child_path(path, newly),
            CompatibilityRule::NewlyRequiredProperty,
        ));
    }
}

/// Existing named properties recurse. Candidate-only names are judged
/// against what the predecessor allowed for them: optional means absence
/// is allowed, not that the value was unconstrained.
pub(super) fn check_properties(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    depth: usize,
    budget: &mut Budget,
    out: &mut Vec<Incompatibility>,
) {
    let pred_props = pred.get("properties").and_then(Value::as_object);
    if let Some(pred_props) = pred_props {
        let cand_props = cand.get("properties").and_then(Value::as_object);
        for (name, sub) in pred_props {
            if budget.exhausted {
                return;
            }
            let child = child_path(path, name);
            match cand_props.and_then(|props| props.get(name)) {
                None => out.push(inc(&child, CompatibilityRule::RemovedProperty)),
                Some(cand_sub) => check_schema(sub, cand_sub, &child, depth + 1, budget, out),
            }
        }
    }
    let Some(cand_props) = cand.get("properties").and_then(Value::as_object) else {
        return;
    };
    for (name, cand_sub) in cand_props {
        if budget.exhausted {
            return;
        }
        if pred_props.is_some_and(|props| props.contains_key(name)) {
            continue;
        }
        if !budget.charge(1) {
            out.push(inc(path, CompatibilityRule::ProofBudgetExhausted));
            return;
        }
        check_new_property(pred, cand_sub, &child_path(path, name), depth, budget, out);
    }
}

/// Judges a candidate-only property against the predecessor's effective
/// policy for that name: a closed object admitted no such payload, so any
/// constraint is safe; an open object admitted every value, so only an
/// unconstrained subschema preserves them; a schema-valued policy recurses
/// through the same sound subschema rules.
fn check_new_property(
    pred: &SchemaObject,
    cand_sub: &Value,
    path: &str,
    depth: usize,
    budget: &mut Budget,
    out: &mut Vec<Incompatibility>,
) {
    match pred.get("patternProperties") {
        None => {}
        Some(Value::Object(patterns)) if patterns.is_empty() => {}
        // Pattern interaction needs regex containment, which this proof
        // domain deliberately excludes.
        Some(_) => {
            out.push(inc(path, CompatibilityRule::NewPropertyConstraintUnproven));
            return;
        }
    }
    match additional_properties(pred) {
        AdditionalProperties::None => {}
        AdditionalProperties::Any => {
            if !is_unconstrained(cand_sub) {
                out.push(inc(path, CompatibilityRule::NewPropertyConstraintUnproven));
            }
        }
        AdditionalProperties::Schema(policy) => {
            check_schema(policy, cand_sub, path, depth + 1, budget, out);
        }
    }
}

/// True when a subschema accepts every JSON value: `true` (tolerated for
/// generators that emit newer boolean schemas), or an object carrying only
/// annotation keywords.
fn is_unconstrained(schema: &Value) -> bool {
    match schema {
        Value::Bool(unconstrained) => *unconstrained,
        Value::Object(object) => object.keys().all(|k| ANNOTATIONS.contains(&k.as_str())),
        _ => false,
    }
}

/// Object-form `items` recurses; tuple-form stays deep-equal. Adding an
/// items schema compares against the empty schema so the reasons name the
/// actual restriction on array elements.
pub(super) fn check_items(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    depth: usize,
    budget: &mut Budget,
    out: &mut Vec<Incompatibility>,
) {
    match (pred.get("items"), cand.get("items")) {
        (None, None) | (Some(_), None) => {}
        (None, Some(c)) if c.is_object() => {
            check_schema(
                &Value::Object(SchemaObject::new()),
                c,
                &format!("{path}[]"),
                depth + 1,
                budget,
                out,
            );
        }
        (None, Some(_)) => out.push(inc(path, CompatibilityRule::ChangedItems)),
        (Some(p), Some(c)) if p.is_object() && c.is_object() => {
            check_schema(p, c, &format!("{path}[]"), depth + 1, budget, out);
        }
        (Some(p), Some(c)) => {
            if charge_equality(p, c, path, budget, out) && p != c {
                out.push(inc(path, CompatibilityRule::ChangedItems));
            }
        }
    }
}

pub(super) fn check_deep_equal(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    budget: &mut Budget,
    out: &mut Vec<Incompatibility>,
) {
    const RULES: [(&str, CompatibilityRule); 9] = [
        ("allOf", CompatibilityRule::ChangedCombinator),
        ("anyOf", CompatibilityRule::ChangedCombinator),
        ("oneOf", CompatibilityRule::ChangedCombinator),
        ("not", CompatibilityRule::ChangedCombinator),
        ("$ref", CompatibilityRule::ChangedRef),
        ("definitions", CompatibilityRule::ChangedDefinitions),
        ("dependencies", CompatibilityRule::ChangedDependencies),
        (
            "patternProperties",
            CompatibilityRule::ChangedPatternProperties,
        ),
        ("additionalItems", CompatibilityRule::ChangedAdditionalItems),
    ];
    for (keyword, rule) in RULES {
        let (p, c) = (pred.get(keyword), cand.get(keyword));
        if p == c {
            continue;
        }
        if let (Some(a), Some(b)) = (p, c) {
            if !charge_equality(a, b, path, budget, out) {
                return;
            }
        }
        out.push(inc(path, rule));
    }
}

pub(super) fn check_unhandled_keywords(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    budget: &mut Budget,
    out: &mut Vec<Incompatibility>,
) {
    const COVERED: [&str; 36] = [
        // Keywords with dedicated compatibility rules above.
        "required",
        "properties",
        "type",
        "enum",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "minLength",
        "maxLength",
        "minItems",
        "maxItems",
        "minProperties",
        "maxProperties",
        "multipleOf",
        "pattern",
        "additionalProperties",
        "uniqueItems",
        "items",
        "additionalItems",
        "allOf",
        "anyOf",
        "oneOf",
        "not",
        "$ref",
        "definitions",
        "dependencies",
        "patternProperties",
        // Draft-4 annotations with no validation effect (`ANNOTATIONS`).
        "$schema",
        "title",
        "description",
        "default",
        "readOnly",
        "media",
        "links",
        "pathStart",
    ];
    let keywords: BTreeSet<&str> = pred.keys().chain(cand.keys()).map(String::as_str).collect();
    for keyword in keywords {
        if COVERED.contains(&keyword) {
            continue;
        }
        let (p, c) = (pred.get(keyword), cand.get(keyword));
        if p == c {
            continue;
        }
        if let (Some(a), Some(b)) = (p, c) {
            if !charge_equality(a, b, path, budget, out) {
                return;
            }
        }
        out.push(inc(path, CompatibilityRule::UnverifiableChange));
    }
}
