//! Shared helpers for SAM template tests. CloudFormation short-form intrinsic
//! functions (`!Ref`, `!Sub`, ...) parse as `Value::Tagged`; `untag` unwraps
//! them so assertions can target the underlying scalar/sequence/mapping.

// Shared helpers are compiled into each test binary; not every binary uses
// every helper.
#![allow(dead_code)]

use serde_yaml::{Mapping, Value};
use std::path::PathBuf;

/// Loads the core control-plane SAM template from the repository root.
pub fn core_template() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../nexus-core-stack.yaml");
    let text = std::fs::read_to_string(&path).expect("read nexus-core-stack.yaml");
    serde_yaml::from_str(&text).expect("parse nexus-core-stack.yaml")
}

/// Unwraps nested CloudFormation intrinsic tags to the underlying value.
pub fn untag(v: &Value) -> &Value {
    match v {
        Value::Tagged(t) => untag(&t.value),
        other => other,
    }
}

/// Returns the value as a mapping, panicking with context otherwise.
pub fn mapping(v: &Value) -> &Mapping {
    match untag(v) {
        Value::Mapping(m) => m,
        other => panic!("expected mapping, got {other:?}"),
    }
}

/// Returns the value as a sequence, panicking with context otherwise.
pub fn seq(v: &Value) -> &Vec<Value> {
    match untag(v) {
        Value::Sequence(s) => s,
        other => panic!("expected sequence, got {other:?}"),
    }
}

/// Looks up `key` in a mapping value, panicking if absent.
pub fn at<'a>(v: &'a Value, key: &str) -> &'a Value {
    mapping(v)
        .get(key)
        .unwrap_or_else(|| panic!("missing key '{key}'"))
}

/// Looks up a top-level resource by logical id.
pub fn resource<'a>(doc: &'a Value, logical_id: &str) -> &'a Value {
    at(at(doc, "Resources"), logical_id)
}

/// The `Properties` node of a resource.
pub fn props(resource: &Value) -> &Value {
    at(resource, "Properties")
}

/// Returns the value as a string slice.
pub fn text(v: &Value) -> &str {
    untag(v)
        .as_str()
        .unwrap_or_else(|| panic!("expected string, got {v:?}"))
}

/// Returns the value as a bool.
pub fn boolean(v: &Value) -> bool {
    untag(v)
        .as_bool()
        .unwrap_or_else(|| panic!("expected bool, got {v:?}"))
}

/// Returns the value as an i64.
pub fn int(v: &Value) -> i64 {
    untag(v)
        .as_i64()
        .unwrap_or_else(|| panic!("expected integer, got {v:?}"))
}

/// Loads any repository-root YAML file (OpenAPI contracts, SAM template) as a
/// serde_yaml Value with CFN intrinsics as `Value::Tagged`.
pub fn repo_yaml(relative_path: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../").join(relative_path);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {relative_path}: {e}"));
    serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {relative_path}: {e}"))
}
