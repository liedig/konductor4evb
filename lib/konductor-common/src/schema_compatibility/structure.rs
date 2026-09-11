//! Structural compatibility rules: the keyword families that walk or reshape
//! the document tree (`required`, `properties`, `items`, combinator-style
//! deep-equality sweeps, and the catch-all for unhandled keywords).

use std::collections::BTreeSet;

use serde_json::Value;

use super::{check_schema, child_path, inc, CompatibilityRule, Incompatibility};

type SchemaObject = serde_json::Map<String, Value>;

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

pub(super) fn check_properties(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    let Some(pred_props) = pred.get("properties").and_then(Value::as_object) else {
        return;
    };
    let cand_props = cand.get("properties").and_then(Value::as_object);
    for (name, sub) in pred_props {
        let child = child_path(path, name);
        match cand_props.and_then(|props| props.get(name)) {
            None => out.push(inc(&child, CompatibilityRule::RemovedProperty)),
            Some(cand_sub) => check_schema(sub, cand_sub, &child, out),
        }
    }
}

pub(super) fn check_items(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    match (pred.get("items"), cand.get("items")) {
        (None, None) | (Some(_), None) => {}
        (None, Some(_)) => out.push(inc(path, CompatibilityRule::ChangedItems)),
        (Some(p), Some(c)) if p.is_object() && c.is_object() => {
            check_schema(p, c, &format!("{path}[]"), out);
        }
        (Some(p), Some(c)) => {
            if p != c {
                out.push(inc(path, CompatibilityRule::ChangedItems));
            }
        }
    }
}

pub(super) fn check_deep_equal(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
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
        if pred.get(keyword) != cand.get(keyword) {
            out.push(inc(path, rule));
        }
    }
}

pub(super) fn check_unhandled_keywords(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    const COVERED: [&str; 38] = [
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
        // Draft-4 annotations and metadata with no validation effect.
        "$schema",
        "id",
        "title",
        "description",
        "default",
        "format",
        "readOnly",
        "media",
        "links",
        "pathStart",
    ];
    let keywords: BTreeSet<&str> = pred.keys().chain(cand.keys()).map(String::as_str).collect();
    for keyword in keywords {
        if !COVERED.contains(&keyword) && pred.get(keyword) != cand.get(keyword) {
            out.push(inc(path, CompatibilityRule::UnverifiableChange));
        }
    }
}
