//! Pins the platform-role isolation invariant (configure-customer-platform
//! D5): platform global roles (`platform-admin`, `platform-auditor`) resolve
//! only from trusted Cognito group claims, never from the grant/entitlement
//! stores, and no API-reachable code path mutates Cognito group membership.
//! Membership administration is Cognito-native (console or CLI by a platform
//! operator); the bindings and entitlements surface must not be able to
//! express or grant a platform role.

mod support;

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::json;

use konductor_common::entitlement::{CreateEntitlementRequest, Entitlement, ScopePattern};
use konductor_common::grant::{CreateBindingRequest, ResourceRole};

const GLOBAL_ROLE_TOKENS: [&str; 2] = ["platform-admin", "platform-auditor"];

#[test]
fn resource_role_rejects_platform_global_role_tokens() {
    for token in GLOBAL_ROLE_TOKENS {
        assert!(
            serde_json::from_value::<ResourceRole>(json!(token)).is_err(),
            "global role {token:?} must not parse as a resource role"
        );
        let body = json!({
            "domain": "orders",
            "role": token,
            "principal": {"type": "aws-account", "accountId": "111122223333"}
        });
        assert!(
            serde_json::from_value::<CreateBindingRequest>(body).is_err(),
            "binding requests must not carry global role {token:?}"
        );
    }
}

#[test]
fn bindings_cannot_widen_to_platform_scope() {
    for token in GLOBAL_ROLE_TOKENS {
        let body = json!({
            "domain": "",
            "role": "owner",
            "principal": {"type": "cognito-group", "issuer": "https://issuer.example", "group": token}
        });
        let request: CreateBindingRequest = serde_json::from_value(body)
            .expect("a binding naming a controlled group still parses as a request");
        assert!(
            request.into_binding("rb-x", Utc::now(), "actor").is_err(),
            "global (domain-less) bindings must stay unrepresentable"
        );
    }
}

#[test]
fn entitlement_requests_have_no_role_or_principal_surface() {
    for field in ["role", "principal", "globalRole", "group"] {
        let body = json!({
            "consumerDomain": "analytics",
            "producerDomain": "orders",
            "scopePattern": "*",
            field: "platform-admin"
        });
        assert!(
            serde_json::from_value::<CreateEntitlementRequest>(body).is_err(),
            "entitlement requests must reject role-like field {field:?}"
        );
    }
}

#[test]
fn entitlement_wire_shape_carries_no_role_material() {
    let entitlement = Entitlement::new(
        "analytics",
        "orders",
        ScopePattern::new("*").unwrap(),
        Utc::now(),
        "platform-admin-sub",
    )
    .unwrap();
    let value = serde_json::to_value(&entitlement).unwrap();
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "consumerDomain",
            "createdAt",
            "createdBy",
            "entitlementId",
            "producerDomain",
            "scopePattern"
        ],
        "entitlements express coverage only — no role, principal, or group fields"
    );
}

/// Rust SDK method names and API names that mutate Cognito group membership.
const GROUP_MUTATION_CALLS: [&str; 8] = [
    "admin_add_user_to_group",
    "admin_remove_user_from_group",
    "AdminAddUserToGroup",
    "AdminRemoveUserFromGroup",
    "admin_create_group",
    "admin_delete_group",
    "AdminCreateGroup",
    "AdminDeleteGroup",
];

/// IAM actions that would let a function mutate Cognito group membership.
const GROUP_MUTATION_ACTIONS: [&str; 6] = [
    "cognito-idp:AdminAddUserToGroup",
    "cognito-idp:AdminRemoveUserFromGroup",
    "cognito-idp:AdminCreateGroup",
    "cognito-idp:AdminDeleteGroup",
    "cognito-idp:Admin*",
    "cognito-idp:*",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_sources_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().unwrap() != "target" {
                rust_sources_under(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_handler_source_mutates_cognito_group_membership() {
    let root = repo_root();
    let mut sources = Vec::new();
    for dir in [
        "control-plane/functions",
        "domain-management/functions",
        "event-streams/functions",
        "lib/konductor-common/src",
    ] {
        rust_sources_under(&root.join(dir), &mut sources);
    }
    assert!(!sources.is_empty(), "scan must cover handler sources");

    for source in sources {
        let text = std::fs::read_to_string(&source).unwrap();
        for call in GROUP_MUTATION_CALLS {
            assert!(
                !text.contains(call),
                "{} references Cognito group mutation {call} — group membership is \
                 Cognito-native and must stay unreachable from API handlers",
                source.display()
            );
        }
    }
}

fn collect_strings(value: &serde_yaml::Value, out: &mut Vec<String>) {
    match support::untag(value) {
        serde_yaml::Value::String(s) => out.push(s.clone()),
        serde_yaml::Value::Sequence(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        serde_yaml::Value::Mapping(map) => {
            for (key, item) in map {
                collect_strings(key, out);
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}

#[test]
fn no_deployed_stack_grants_cognito_group_mutation() {
    for template in [
        "konductor-core-stack.yaml",
        "trust-stack.yaml",
        "notifications-stack.yaml",
        "guardrail-stack.yaml",
    ] {
        let doc = support::repo_yaml(template);
        let mut strings = Vec::new();
        collect_strings(&doc, &mut strings);
        for action in GROUP_MUTATION_ACTIONS {
            assert!(
                !strings.iter().any(|s| s == action),
                "{template} grants {action} — no platform function may mutate Cognito \
                 group membership"
            );
        }
    }
}
