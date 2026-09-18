//! Per-keyword compatibility rules. Each `check_*` function compares one
//! draft-4 keyword family between the predecessor and candidate subschema at
//! `path` and appends an [`Incompatibility`] per restriction it can prove —
//! or per comparison it cannot prove, which fails closed as unproven rather
//! than approximating.

use std::cmp::Ordering;

use serde_json::Value;

use super::{check_schema, inc, Budget, CompatibilityRule, Incompatibility};

type SchemaObject = serde_json::Map<String, Value>;

fn type_set(schema: &SchemaObject) -> Option<std::collections::BTreeSet<&str>> {
    match schema.get("type") {
        None => None,
        Some(Value::String(single)) => Some(std::collections::BTreeSet::from([single.as_str()])),
        Some(Value::Array(many)) => Some(many.iter().filter_map(Value::as_str).collect()),
        Some(_) => Some(std::collections::BTreeSet::new()),
    }
}

/// The candidate's type set must contain the predecessor's: every
/// previously typed value keeps its type.
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

/// Every predecessor enum value must remain a candidate enum value. Values
/// compare representationally; mathematically equal but differently
/// represented values (e.g. `1` vs `1.0`) fail closed as shrinkage.
pub(super) fn check_enum(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    budget: &mut Budget,
    out: &mut Vec<Incompatibility>,
) {
    let shrunk = match (
        pred.get("enum").and_then(Value::as_array),
        cand.get("enum").and_then(Value::as_array),
    ) {
        (None, None) | (Some(_), None) => false,
        (None, Some(_)) => true,
        (Some(p), Some(c)) => {
            let cost = (p.len() as u64).saturating_mul(c.len() as u64);
            if !budget.charge(cost.max(1)) {
                out.push(inc(path, CompatibilityRule::ProofBudgetExhausted));
                return;
            }
            p.iter().any(|value| !c.contains(value))
        }
    };
    if shrunk {
        out.push(inc(path, CompatibilityRule::ShrunkEnum));
    }
}

/// Three-way exact comparison of JSON schema numbers. Only comparisons the
/// application's validator semantics make exact are supported: equal
/// values, and integers including integral floats within the exactly
/// representable range. Anything precision-sensitive is `None` so the
/// caller fails closed instead of comparing through a lossy `f64`.
fn compare_numbers(a: &Value, b: &Value) -> Option<Ordering> {
    if a == b {
        return Some(Ordering::Equal);
    }
    match (integer_value(a), integer_value(b)) {
        (Some(x), Some(y)) => Some(x.cmp(&y)),
        _ => None,
    }
}

/// The exact integer a numeric keyword denotes: stored integers directly,
/// integral floats only while `f64` still represents every integer exactly.
fn integer_value(value: &Value) -> Option<i128> {
    let number = value.as_number()?;
    if let Some(unsigned) = number.as_u64() {
        return Some(i128::from(unsigned));
    }
    if let Some(signed) = number.as_i64() {
        return Some(i128::from(signed));
    }
    let float = number.as_f64()?;
    if float.fract() == 0.0 && float.abs() <= 9_007_199_254_740_992.0 {
        Some(float as i128)
    } else {
        None
    }
}

fn positive_integer(value: &Value) -> Option<i128> {
    let integer = integer_value(value)?;
    (integer > 0).then_some(integer)
}

fn exclusive(schema: &SchemaObject, keyword: &str) -> bool {
    schema
        .get(keyword)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The candidate's lower bound must not exclude values the predecessor
/// admitted: a lower `minimum`, or the same bound newly exclusive.
pub(super) fn check_minimum(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    match (pred.get("minimum"), cand.get("minimum")) {
        (None, None) | (Some(_), None) => {}
        (None, Some(_)) => out.push(inc(path, CompatibilityRule::RaisedMinimum)),
        (Some(p), Some(c)) => match compare_numbers(c, p) {
            None => out.push(inc(path, CompatibilityRule::UnprovenNumericComparison)),
            Some(Ordering::Greater) => out.push(inc(path, CompatibilityRule::RaisedMinimum)),
            Some(Ordering::Equal) => {
                if exclusive(cand, "exclusiveMinimum") && !exclusive(pred, "exclusiveMinimum") {
                    out.push(inc(path, CompatibilityRule::RaisedMinimum));
                }
            }
            Some(Ordering::Less) => {}
        },
    }
}

/// The candidate's upper bound must not exclude values the predecessor
/// admitted: a lower `maximum`, or the same bound newly exclusive.
pub(super) fn check_maximum(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    match (pred.get("maximum"), cand.get("maximum")) {
        (None, None) | (Some(_), None) => {}
        (None, Some(_)) => out.push(inc(path, CompatibilityRule::LoweredMaximum)),
        (Some(p), Some(c)) => match compare_numbers(c, p) {
            None => out.push(inc(path, CompatibilityRule::UnprovenNumericComparison)),
            Some(Ordering::Less) => out.push(inc(path, CompatibilityRule::LoweredMaximum)),
            Some(Ordering::Equal) => {
                if exclusive(cand, "exclusiveMaximum") && !exclusive(pred, "exclusiveMaximum") {
                    out.push(inc(path, CompatibilityRule::LoweredMaximum));
                }
            }
            Some(Ordering::Greater) => {}
        },
    }
}

/// The exact count a size keyword denotes; present-but-non-`u64` values
/// cannot be compared exactly and fail closed as unproven.
fn count_value(value: Option<&Value>, default: u64) -> Option<u64> {
    match value {
        None => Some(default),
        Some(value) => value.as_u64(),
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
    let (p, c) = (pred.get(keyword), cand.get(keyword));
    if p == c {
        return;
    }
    match (count_value(p, 0), count_value(c, 0)) {
        (Some(p), Some(c)) => {
            if c > p {
                out.push(inc(path, rule));
            }
        }
        _ => out.push(inc(path, CompatibilityRule::UnprovenNumericComparison)),
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
    let (p, c) = (pred.get(keyword), cand.get(keyword));
    if p == c {
        return;
    }
    match (count_value(p, u64::MAX), count_value(c, u64::MAX)) {
        (Some(p), Some(c)) => {
            if c < p {
                out.push(inc(path, rule));
            }
        }
        _ => out.push(inc(path, CompatibilityRule::UnprovenNumericComparison)),
    }
}

/// The candidate's `multipleOf` must divide the predecessor's exactly:
/// every multiple of the predecessor's divisor stays a multiple of the
/// candidate's. Only positive-integer divisibility is proven; fractional
/// or inexact representations fail closed rather than dividing rounded
/// floats.
pub(super) fn check_multiple_of(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    out: &mut Vec<Incompatibility>,
) {
    match (pred.get("multipleOf"), cand.get("multipleOf")) {
        (None, None) | (Some(_), None) => {}
        (None, Some(_)) => out.push(inc(path, CompatibilityRule::TightenedMultipleOf)),
        (Some(p), Some(c)) if p == c => {}
        (Some(p), Some(c)) => match (positive_integer(p), positive_integer(c)) {
            (Some(p), Some(c)) => {
                if p % c != 0 {
                    out.push(inc(path, CompatibilityRule::TightenedMultipleOf));
                }
            }
            _ => out.push(inc(path, CompatibilityRule::UnprovenNumericComparison)),
        },
    }
}

/// Regex containment is outside this proof domain: a pattern may only stay
/// identical or be removed.
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

/// Draft-4 `additionalProperties` admits everything (absent/`true`),
/// nothing (`false`), or whatever a subschema admits (object).
pub(super) enum AdditionalProperties<'a> {
    Any,
    None,
    Schema(&'a Value),
}

pub(super) fn additional_properties(schema: &SchemaObject) -> AdditionalProperties<'_> {
    match schema.get("additionalProperties") {
        None | Some(Value::Bool(true)) => AdditionalProperties::Any,
        Some(Value::Bool(false)) => AdditionalProperties::None,
        Some(subschema) => AdditionalProperties::Schema(subschema),
    }
}

/// The candidate's additional-property policy must admit every property
/// the predecessor's did. Schema-valued policies recurse through the same
/// sound subschema rules as any other position.
pub(super) fn check_additional_properties(
    pred: &SchemaObject,
    cand: &SchemaObject,
    path: &str,
    depth: usize,
    budget: &mut Budget,
    out: &mut Vec<Incompatibility>,
) {
    use AdditionalProperties::{Any, None as NoAdditional, Schema};
    match (additional_properties(pred), additional_properties(cand)) {
        (Any, Any) | (NoAdditional, _) | (Schema(_), Any) => {}
        (Schema(p), Schema(c)) => {
            check_schema(p, c, &format!("{path}/*"), depth + 1, budget, out);
        }
        (Any, NoAdditional | Schema(_)) | (Schema(_), NoAdditional) => {
            out.push(inc(path, CompatibilityRule::RestrictedAdditionalProperties));
        }
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
