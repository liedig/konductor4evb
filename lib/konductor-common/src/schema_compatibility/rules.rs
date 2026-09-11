//! Per-keyword compatibility rules. Each `check_*` function compares one
//! draft-4 keyword family between the predecessor and candidate subschema at
//! `path` and appends an [`Incompatibility`] per tightening it can prove.
//! Anything a rule cannot prove syntactically rejects.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{inc, CompatibilityRule, Incompatibility};

type SchemaObject = serde_json::Map<String, Value>;

fn type_set(schema: &SchemaObject) -> Option<BTreeSet<&str>> {
    match schema.get("type") {
        None => None,
        Some(Value::String(single)) => Some(BTreeSet::from([single.as_str()])),
        Some(Value::Array(many)) => Some(many.iter().filter_map(Value::as_str).collect()),
        Some(_) => Some(BTreeSet::new()),
    }
}

pub(super) fn check_type(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    let narrowed = match (type_set(pred), type_set(cand)) {
        (None, None) | (Some(_), None) => false,
        (None, Some(_)) => true,
        (Some(p), Some(c)) => !p.is_subset(&c),
    };
    if narrowed {
        out.push(inc(path, CompatibilityRule::NarrowedType));
    }
}

pub(super) fn check_enum(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    let shrunk = match (
        pred.get("enum").and_then(Value::as_array),
        cand.get("enum").and_then(Value::as_array),
    ) {
        (None, None) | (Some(_), None) => false,
        (None, Some(_)) => true,
        (Some(p), Some(c)) => p.iter().any(|value| !c.contains(value)),
    };
    if shrunk {
        out.push(inc(path, CompatibilityRule::ShrunkEnum));
    }
}

fn lower_bound(schema: &SchemaObject) -> Option<(f64, bool)> {
    let minimum = schema.get("minimum")?.as_f64()?;
    let exclusive = schema
        .get("exclusiveMinimum")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some((minimum, exclusive))
}

fn upper_bound(schema: &SchemaObject) -> Option<(f64, bool)> {
    let maximum = schema.get("maximum")?.as_f64()?;
    let exclusive = schema
        .get("exclusiveMaximum")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some((maximum, exclusive))
}

pub(super) fn check_minimum(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    let raised = match (lower_bound(pred), lower_bound(cand)) {
        (None, None) | (Some(_), None) => false,
        (None, Some(_)) => true,
        (Some((p, p_excl)), Some((c, c_excl))) => c > p || (c == p && c_excl && !p_excl),
    };
    if raised {
        out.push(inc(path, CompatibilityRule::RaisedMinimum));
    }
}

pub(super) fn check_maximum(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    let lowered = match (upper_bound(pred), upper_bound(cand)) {
        (None, None) | (Some(_), None) => false,
        (None, Some(_)) => true,
        (Some((p, p_excl)), Some((c, c_excl))) => c < p || (c == p && c_excl && !p_excl),
    };
    if lowered {
        out.push(inc(path, CompatibilityRule::LoweredMaximum));
    }
}

pub(super) fn check_min(
    pred: &SchemaObject,
    cand: &SchemaObject,
    keyword: &str,
    rule: CompatibilityRule,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    let value = |schema: &SchemaObject| schema.get(keyword).and_then(Value::as_u64).unwrap_or(0);
    if value(cand) > value(pred) {
        out.push(inc(path, rule));
    }
}

pub(super) fn check_max(
    pred: &SchemaObject,
    cand: &SchemaObject,
    keyword: &str,
    rule: CompatibilityRule,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    let value = |schema: &SchemaObject| {
        schema
            .get(keyword)
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX)
    };
    if value(cand) < value(pred) {
        out.push(inc(path, rule));
    }
}

pub(super) fn check_multiple_of(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    let tightened = match (
        pred.get("multipleOf").and_then(Value::as_f64),
        cand.get("multipleOf").and_then(Value::as_f64),
    ) {
        (None, None) | (Some(_), None) => false,
        (None, Some(_)) => true,
        (Some(p), Some(c)) => c <= 0.0 || (p / c).fract() != 0.0,
    };
    if tightened {
        out.push(inc(path, CompatibilityRule::TightenedMultipleOf));
    }
}

pub(super) fn check_pattern(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    let changed = match (
        pred.get("pattern").and_then(Value::as_str),
        cand.get("pattern").and_then(Value::as_str),
    ) {
        (None, None) | (Some(_), None) => false,
        (None, Some(_)) => true,
        (Some(p), Some(c)) => p != c,
    };
    if changed {
        out.push(inc(path, CompatibilityRule::ChangedPattern));
    }
}

/// Draft-4 `additionalProperties` admits everything (absent/`true`), nothing
/// (`false`), or whatever a subschema admits (object).
enum AdditionalProperties<'a> {
    Any,
    None,
    Schema(&'a Value),
}

fn additional_properties(schema: &SchemaObject) -> AdditionalProperties<'_> {
    match schema.get("additionalProperties") {
        None | Some(Value::Bool(true)) => AdditionalProperties::Any,
        Some(Value::Bool(false)) => AdditionalProperties::None,
        Some(subschema) => AdditionalProperties::Schema(subschema),
    }
}

pub(super) fn check_additional_properties(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    use AdditionalProperties::{Any, None as NoAdditional, Schema};
    let restricted = match (additional_properties(pred), additional_properties(cand)) {
        (Any, Any) | (NoAdditional, _) | (Schema(_), Any) => false,
        (Schema(p), Schema(c)) => p != c,
        (Any, NoAdditional | Schema(_)) | (Schema(_), NoAdditional) => true,
    };
    if restricted {
        out.push(inc(path, CompatibilityRule::RestrictedAdditionalProperties));
    }
}

pub(super) fn check_unique_items(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    let asserted = |schema: &SchemaObject| schema.get("uniqueItems") == Some(&Value::Bool(true));
    if !asserted(pred) && asserted(cand) {
        out.push(inc(path, CompatibilityRule::AddedUniqueItems));
    }
}
