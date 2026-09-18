//! Table-driven fixtures for the organization guardrail package
//! (OpenSpec change `protect-domain-managed-resources`, task 1.2).
//!
//! Each row describes one (principal, action, resource) triple and the
//! outcome the SCP guardrail must produce. The harness resolves the
//! repository-root `guardrail-stack.yaml`, walks its SCP statements, and
//! asserts the row's expected result.

mod support;

use support::repo_yaml;

const GUARDRAIL_TEMPLATE: &str = "guardrail-stack.yaml";

const ORG: &str = "acme";
const MEMBER_ACCOUNT_ID: &str = "111122223333";
const PLATFORM_ACCOUNT_ID: &str = "999988887777";

fn trust_role_arn() -> String {
    format!("arn:aws:iam::{MEMBER_ACCOUNT_ID}:role/{ORG}-konductor-trust-role")
}

fn recovery_role_arn() -> String {
    format!("arn:aws:iam::{PLATFORM_ACCOUNT_ID}:role/{ORG}-konductor-recovery-role")
}

fn application_bus_arn() -> String {
    format!("arn:aws:events:us-east-1:{MEMBER_ACCOUNT_ID}:event-bus/{ORG}-prod-payments")
}

fn integration_dlq_arn() -> String {
    format!("arn:aws:sqs:us-east-1:{MEMBER_ACCOUNT_ID}:{ORG}-prod-payments-integration-dlq")
}

fn unrelated_bus_arn() -> String {
    "arn:aws:events:us-east-1:111122223333:event-bus/team-other-bus".to_string()
}

fn unrelated_queue_arn() -> String {
    "arn:aws:sqs:us-east-1:111122223333:team-other-queue".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrincipalKind {
    OrdinaryApplication,
    LocalAdministrator,
    KonductorTrustRoleSession,
    CentralRecoveryRole,
    SimilarlyNamedUnauthorizedRole,
}

impl PrincipalKind {
    fn arn(self) -> String {
        match self {
            Self::OrdinaryApplication => {
                format!("arn:aws:iam::{MEMBER_ACCOUNT_ID}:role/team-payments-app")
            }
            Self::LocalAdministrator => {
                format!("arn:aws:iam::{MEMBER_ACCOUNT_ID}:role/DomainAdmin")
            }
            Self::KonductorTrustRoleSession => trust_role_arn(),
            Self::CentralRecoveryRole => recovery_role_arn(),
            Self::SimilarlyNamedUnauthorizedRole => {
                format!("arn:aws:iam::{MEMBER_ACCOUNT_ID}:role/{ORG}-konductor-trust-role-evil")
            }
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::OrdinaryApplication => "ordinary application principal",
            Self::LocalAdministrator => "local administrator",
            Self::KonductorTrustRoleSession => "Konductor trust-role session",
            Self::CentralRecoveryRole => "central recovery role",
            Self::SimilarlyNamedUnauthorizedRole => "similarly named unauthorized role",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    Allowed,
    Denied,
}

impl Expected {
    fn label(self) -> &'static str {
        match self {
            Self::Allowed => "ALLOWED",
            Self::Denied => "DENIED",
        }
    }
}

#[derive(Debug)]
struct FixtureRow {
    principal: PrincipalKind,
    action: &'static str,
    resource: String,
    expected: Expected,
    rationale: &'static str,
}

fn fixtures() -> Vec<FixtureRow> {
    use Expected::{Allowed, Denied};
    use PrincipalKind::{
        CentralRecoveryRole, KonductorTrustRoleSession, LocalAdministrator, OrdinaryApplication,
        SimilarlyNamedUnauthorizedRole,
    };

    let bus = application_bus_arn();
    let dlq = integration_dlq_arn();
    let trust = trust_role_arn();
    let unrelated_bus = unrelated_bus_arn();
    let unrelated_queue = unrelated_queue_arn();

    let mut rows = Vec::new();

    // ---- Allowed integration operations on domain resources -------------

    let allowed_integration: &[(&str, &str, &str)] = &[
        (
            "events:PutRule",
            "bus",
            "team creates an integration rule on the application bus",
        ),
        (
            "events:DeleteRule",
            "bus",
            "team deletes its own integration rule",
        ),
        (
            "events:EnableRule",
            "bus",
            "team enables its own integration rule",
        ),
        (
            "events:DisableRule",
            "bus",
            "team disables its own integration rule",
        ),
        (
            "events:PutTargets",
            "bus",
            "team attaches targets to its own rule",
        ),
        (
            "events:RemoveTargets",
            "bus",
            "team detaches targets from its own rule",
        ),
        ("events:DescribeRule", "bus", "team reads its own rule"),
        (
            "sqs:ReceiveMessage",
            "dlq",
            "team consumes integration DLQ messages",
        ),
        (
            "sqs:DeleteMessage",
            "dlq",
            "team acknowledges a DLQ message",
        ),
        (
            "sqs:DeleteMessageBatch",
            "dlq",
            "batch acknowledgement is covered by sqs:DeleteMessage",
        ),
        (
            "sqs:ChangeMessageVisibility",
            "dlq",
            "team extends visibility for in-flight work",
        ),
        (
            "sqs:ChangeMessageVisibilityBatch",
            "dlq",
            "batch visibility change is covered by sqs:ChangeMessageVisibility",
        ),
        (
            "sqs:SendMessage",
            "dlq",
            "team / EventBridge delivers into the DLQ",
        ),
        (
            "sqs:SendMessageBatch",
            "dlq",
            "batch send is covered by sqs:SendMessage",
        ),
        (
            "sqs:GetQueueAttributes",
            "dlq",
            "team inspects DLQ configuration",
        ),
        ("sqs:GetQueueUrl", "dlq", "team resolves the queue URL"),
        ("sqs:ListQueueTags", "dlq", "team reads queue tags"),
        (
            "events:DescribeEventBus",
            "bus",
            "team describes the application bus",
        ),
    ];

    for (action, target, rationale) in allowed_integration {
        let resource = match *target {
            "bus" => bus.clone(),
            "dlq" => dlq.clone(),
            other => panic!("unknown target {other}"),
        };
        rows.push(FixtureRow {
            principal: OrdinaryApplication,
            action,
            resource,
            expected: Allowed,
            rationale,
        });
    }

    // ---- Denied infrastructure changes on protected resources -----------

    let denied_infra: &[(&str, &str, &str)] = &[
        (
            "events:DeleteEventBus",
            "bus",
            "deleting the application bus is infrastructure destruction",
        ),
        (
            "events:CreateEventBus",
            "bus",
            "creating a reserved-name bus could squat or shadow the managed namespace",
        ),
        (
            "events:PutPermission",
            "bus",
            "replacing the bus resource policy opens it to foreign accounts",
        ),
        (
            "events:RemovePermission",
            "bus",
            "stripping bus permissions removes the Konductor access path",
        ),
        (
            "sqs:DeleteQueue",
            "dlq",
            "deleting the integration DLQ destroys durable state",
        ),
        (
            "sqs:CreateQueue",
            "dlq",
            "creating a reserved-name queue could squat or shadow the managed namespace",
        ),
        (
            "sqs:SetQueueAttributes",
            "dlq",
            "attribute edits cover policy, redrive, retention, and KMS configuration",
        ),
        (
            "sqs:PurgeQueue",
            "dlq",
            "purge is destructive and prohibited by default",
        ),
    ];

    for (action, target, rationale) in denied_infra {
        let resource = match *target {
            "bus" => bus.clone(),
            "dlq" => dlq.clone(),
            other => panic!("unknown target {other}"),
        };
        rows.push(FixtureRow {
            principal: OrdinaryApplication,
            action,
            resource: resource.clone(),
            expected: Denied,
            rationale,
        });
        rows.push(FixtureRow {
            principal: LocalAdministrator,
            action,
            resource,
            expected: Denied,
            rationale: "local administrator is subject to the same SCP ceiling",
        });
    }

    // ---- Unrelated-resource controls ------------------------------------
    //
    // Resources outside the `{org}-*` namespace must not be denied by this
    // SCP, even for actions that are PROTECTED against the managed
    // namespace.

    let unrelated: &[(&str, &str, &str)] = &[
        (
            "events:DeleteEventBus",
            "unrelated_bus",
            "non-Konductor bus is outside the protected selector",
        ),
        (
            "events:CreateEventBus",
            "unrelated_bus",
            "creating an unrelated bus is not a reserved-namespace operation",
        ),
        (
            "sqs:DeleteQueue",
            "unrelated_queue",
            "non-Konductor queue is outside the protected selector",
        ),
        (
            "sqs:CreateQueue",
            "unrelated_queue",
            "creating an unrelated queue is not a reserved-namespace operation",
        ),
        (
            "sqs:SetQueueAttributes",
            "unrelated_queue",
            "attribute edits on an unrelated queue are unaffected",
        ),
        (
            "sqs:PurgeQueue",
            "unrelated_queue",
            "purge on an unrelated queue is unaffected",
        ),
    ];

    for (action, target, rationale) in unrelated {
        let resource = match *target {
            "unrelated_bus" => unrelated_bus.clone(),
            "unrelated_queue" => unrelated_queue.clone(),
            other => panic!("unknown target {other}"),
        };
        rows.push(FixtureRow {
            principal: LocalAdministrator,
            action,
            resource,
            expected: Allowed,
            rationale,
        });
    }

    // ---- Tag / name bypass attempts on protected resources --------------

    let tag_bypass: &[(&str, &str, &str)] = &[
        (
            "events:TagResource",
            "bus",
            "mutating the ownership tag on the application bus would bypass tag-based selectors",
        ),
        (
            "events:UntagResource",
            "bus",
            "stripping the ownership tag from the application bus would evade tag-based enforcement",
        ),
        (
            "sqs:TagQueue",
            "dlq",
            "mutating the ownership tag on the integration DLQ would bypass tag-based selectors",
        ),
        (
            "sqs:UntagQueue",
            "dlq",
            "stripping the ownership tag from the integration DLQ would evade tag-based enforcement",
        ),
    ];

    for (action, target, rationale) in tag_bypass {
        let resource = match *target {
            "bus" => bus.clone(),
            "dlq" => dlq.clone(),
            other => panic!("unknown target {other}"),
        };
        rows.push(FixtureRow {
            principal: OrdinaryApplication,
            action,
            resource: resource.clone(),
            expected: Denied,
            rationale,
        });
        rows.push(FixtureRow {
            principal: LocalAdministrator,
            action,
            resource,
            expected: Denied,
            rationale: "local administrator must not bypass via tag mutation",
        });
    }

    // ---- Protected-role escalation --------------------------------------
    //
    // The trust role itself is exempt for resource management but must NOT
    // be able to modify its own trust or permissions.

    let role_escalation_actions: &[(&str, &str)] = &[
        (
            "iam:UpdateAssumeRolePolicy",
            "rewriting the trust policy opens the role to attacker principals",
        ),
        (
            "iam:AttachRolePolicy",
            "attaching extra policies expands the role's authority",
        ),
        (
            "iam:DetachRolePolicy",
            "detaching policies strips operational or protective grants",
        ),
        (
            "iam:PutRolePolicy",
            "inline policy edits change effective permissions",
        ),
        ("iam:DeleteRolePolicy", "inline policy removal"),
        (
            "iam:PutRolePermissionsBoundary",
            "boundary changes raise or lower ceilings",
        ),
        (
            "iam:DeleteRole",
            "deleting the role breaks the management path",
        ),
        (
            "iam:PassRole",
            "passing the role hands its authority to a service",
        ),
    ];

    for (action, rationale) in role_escalation_actions {
        for principal in [
            OrdinaryApplication,
            LocalAdministrator,
            KonductorTrustRoleSession,
            SimilarlyNamedUnauthorizedRole,
        ] {
            rows.push(FixtureRow {
                principal,
                action,
                resource: trust.clone(),
                expected: Denied,
                rationale,
            });
        }
        // Central recovery role is the only identity allowed to perform
        // role maintenance on the protected trust role.
        rows.push(FixtureRow {
            principal: CentralRecoveryRole,
            action,
            resource: trust.clone(),
            expected: Allowed,
            rationale: "central recovery is the designated maintenance path",
        });
    }

    // Deleting or recreating the exact trust-role name is denied for all
    // non-recovery principals, including the trust role itself.
    for action in ["iam:CreateRole", "iam:DeleteRole"] {
        for principal in [
            OrdinaryApplication,
            LocalAdministrator,
            KonductorTrustRoleSession,
            SimilarlyNamedUnauthorizedRole,
        ] {
            rows.push(FixtureRow {
                principal,
                action,
                resource: trust.clone(),
                expected: Denied,
                rationale: "role-name recreation must go through central recovery",
            });
        }
    }

    // A similarly named role must not inherit the exception.
    rows.push(FixtureRow {
        principal: SimilarlyNamedUnauthorizedRole,
        action: "events:DeleteEventBus",
        resource: bus.clone(),
        expected: Denied,
        rationale: "the SCP exception matches the exact trust-role ARN, not a name prefix",
    });
    rows.push(FixtureRow {
        principal: SimilarlyNamedUnauthorizedRole,
        action: "sqs:DeleteQueue",
        resource: dlq.clone(),
        expected: Denied,
        rationale: "the SCP exception matches the exact trust-role ARN, not a name prefix",
    });

    // The trust role itself is allowed to perform resource management.
    for (action, resource) in [
        ("events:DeleteEventBus", bus.clone()),
        ("events:CreateEventBus", bus.clone()),
        ("sqs:DeleteQueue", dlq.clone()),
        ("sqs:SetQueueAttributes", dlq.clone()),
        ("sqs:PurgeQueue", dlq.clone()),
    ] {
        rows.push(FixtureRow {
            principal: KonductorTrustRoleSession,
            action,
            resource,
            expected: Allowed,
            rationale: "the exact trust-role session is the designated resource-management path",
        });
    }

    // ---- Organization departure -----------------------------------------

    for principal in [
        OrdinaryApplication,
        LocalAdministrator,
        KonductorTrustRoleSession,
        SimilarlyNamedUnauthorizedRole,
    ] {
        rows.push(FixtureRow {
            principal,
            action: "organizations:LeaveOrganization",
            resource: format!("arn:aws:organizations::{MEMBER_ACCOUNT_ID}:account/o-exampleorg/{MEMBER_ACCOUNT_ID}"),
            expected: Denied,
            rationale: "member accounts must not shed guardrails by leaving the organization",
        });
    }

    rows
}

/// Resolves the guardrail template, failing with a clear "template not
/// found" message until task 2.1 lands it.
fn guardrail_template() -> serde_yaml::Value {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(GUARDRAIL_TEMPLATE);
    assert!(
        path.exists(),
        "guardrail template not found at {} — task 2.1 must add it before these fixtures can pass",
        path.display()
    );
    repo_yaml(GUARDRAIL_TEMPLATE)
}

/// Resolves CloudFormation intrinsics and pseudo-parameters in a string
/// extracted from the template Content section.
///
/// Handles:
/// - `${Organization}` → the fixture org prefix
/// - `${AWS::Partition}` → `aws` (the standard partition)
/// - `${AWS::AccountId}` → `*` (SCPs apply per-account; wildcard matches any)
/// - `Ref` to `KonductorTrustRoleArn` → the fixture trust-role ARN
/// - `Ref` to `RecoveryRoleArn` → the fixture recovery-role ARN
fn resolve_template_string(s: &str) -> String {
    s.replace("${Organization}", ORG)
        .replace("${AWS::Partition}", "aws")
        .replace("${AWS::AccountId}", "*")
}

/// Extracts a string from a YAML value, resolving `Ref` and `Sub` tags to
/// known parameter values and applying template-string substitution.
fn resolve_value(v: &serde_yaml::Value) -> String {
    if let serde_yaml::Value::Tagged(tagged) = v {
        let tag_str = tagged.tag.to_string();
        let inner = &tagged.value;
        match tag_str.as_str() {
            "!Ref" => {
                let param_name = inner.as_str().expect("Ref value must be a string");
                return match param_name {
                    "KonductorTrustRoleArn" => trust_role_arn(),
                    "RecoveryRoleArn" => recovery_role_arn(),
                    "Organization" => ORG.to_string(),
                    "AWS::Partition" => "aws".to_string(),
                    "AWS::AccountId" => "*".to_string(),
                    other => panic!("unresolvable Ref: {other}"),
                };
            }
            "!Sub" => {
                let s = inner.as_str().expect("Sub value must be a string");
                return resolve_template_string(s);
            }
            _ => return resolve_value(inner),
        }
    }

    match v {
        serde_yaml::Value::String(s) => resolve_template_string(s),
        other => panic!("unexpected value shape: {other:?}"),
    }
}

/// Glob-style matching where `*` matches any sequence of characters.
/// The pattern is split on `*` and each segment must appear in order.
fn glob_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == value;
    }
    let segments: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0usize;
    let bytes = value.as_bytes();

    for (i, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            continue;
        }
        let seg_bytes = segment.as_bytes();
        if i == 0 {
            if bytes.len() < seg_bytes.len() || &bytes[..seg_bytes.len()] != seg_bytes {
                return false;
            }
            pos = seg_bytes.len();
        } else if i == segments.len() - 1 {
            if bytes.len() < pos + seg_bytes.len() {
                return false;
            }
            if &bytes[bytes.len() - seg_bytes.len()..] != seg_bytes {
                return false;
            }
            pos = bytes.len();
        } else {
            match value[pos..].find(segment) {
                Some(idx) => pos = pos + idx + seg_bytes.len(),
                None => return false,
            }
        }
    }
    true
}

/// Walks the SCP statements in the guardrail template and decides whether
/// the (principal, action, resource) triple is denied.
///
/// This is a deliberately naive evaluator: it only understands the
/// statement shapes this package uses (Deny on `Action` + `Resource` with
/// an optional `StringNotEquals`/`ArnNotEquals` exception on
/// `aws:PrincipalArn`). It is not a general IAM evaluator.
///
/// CloudFormation intrinsics in the Content section are resolved via
/// [`resolve_value`] and [`resolve_template_string`].
fn scp_denies(doc: &serde_yaml::Value, principal_arn: &str, action: &str, resource: &str) -> bool {
    use support::{at, mapping, seq, text, untag};

    let resources = at(doc, "Resources");
    let resources_map = mapping(resources);

    for (_logical_id, resource_value) in resources_map {
        let resource_map = mapping(resource_value);
        let type_value = match resource_map.get("Type") {
            Some(v) => text(v),
            None => continue,
        };
        if type_value != "AWS::Organizations::Policy" {
            continue;
        }

        let properties = match resource_map.get("Properties") {
            Some(v) => v,
            None => continue,
        };
        let content = match mapping(untag(properties)).get("Content") {
            Some(v) => v,
            None => continue,
        };
        let statements = match mapping(untag(content)).get("Statement") {
            Some(v) => seq(v),
            None => continue,
        };

        for statement in statements {
            let statement_map = mapping(untag(statement));
            let effect = match statement_map.get("Effect") {
                Some(v) => text(v),
                None => continue,
            };
            if effect != "Deny" {
                continue;
            }

            let actions: Vec<&str> = match statement_map.get("Action") {
                Some(v) => match untag(v) {
                    serde_yaml::Value::Sequence(items) => items.iter().map(text).collect(),
                    serde_yaml::Value::String(_) => vec![text(v)],
                    other => panic!("unexpected Action shape: {other:?}"),
                },
                None => continue,
            };
            if !actions.iter().any(|a| *a == action || *a == "*") {
                continue;
            }

            let resources_in_statement: Vec<String> = match statement_map.get("Resource") {
                Some(v) => match untag(v) {
                    serde_yaml::Value::Sequence(items) => items.iter().map(resolve_value).collect(),
                    serde_yaml::Value::String(_) => vec![resolve_value(v)],
                    other => panic!("unexpected Resource shape: {other:?}"),
                },
                None => continue,
            };

            let resource_matches = resources_in_statement
                .iter()
                .any(|pattern| glob_matches(pattern, resource));
            if !resource_matches {
                continue;
            }

            let mut principal_exempt = false;
            if let Some(condition) = statement_map.get("Condition") {
                let condition_map = mapping(untag(condition));
                for (_operator, condition_value) in condition_map {
                    let condition_kv = mapping(untag(condition_value));
                    if let Some(principal_arns) = condition_kv.get("aws:PrincipalArn") {
                        let arns: Vec<String> = match untag(principal_arns) {
                            serde_yaml::Value::Sequence(items) => {
                                items.iter().map(resolve_value).collect()
                            }
                            serde_yaml::Value::String(_) => vec![resolve_value(principal_arns)],
                            other => panic!("unexpected aws:PrincipalArn shape: {other:?}"),
                        };
                        if arns.iter().any(|arn| arn == principal_arn) {
                            principal_exempt = true;
                        }
                    }
                }
            }

            if !principal_exempt {
                return true;
            }
        }
    }

    false
}

#[test]
fn guardrail_template_is_present() {
    let _ = guardrail_template();
}

#[test]
fn every_fixture_row_matches_the_guardrail() {
    let doc = guardrail_template();
    let rows = fixtures();
    assert!(!rows.is_empty(), "fixture table must not be empty");

    let mut failures = Vec::new();
    for row in &rows {
        let principal_arn = row.principal.arn();
        let denied = scp_denies(&doc, &principal_arn, row.action, &row.resource);
        let expected_denied = row.expected == Expected::Denied;
        if denied != expected_denied {
            failures.push(format!(
                "principal={} action={} resource={} expected={} got={} — {}",
                row.principal.label(),
                row.action,
                row.resource,
                row.expected.label(),
                if denied { "DENIED" } else { "ALLOWED" },
                row.rationale
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} fixture row(s) mismatched:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn fixture_table_covers_all_principal_kinds() {
    use std::collections::HashSet;
    let rows = fixtures();
    let principals: HashSet<_> = rows.iter().map(|r| r.principal.label()).collect();
    for expected in [
        "ordinary application principal",
        "local administrator",
        "Konductor trust-role session",
        "central recovery role",
        "similarly named unauthorized role",
    ] {
        assert!(
            principals.contains(expected),
            "fixture table is missing principal kind: {expected}"
        );
    }
}

#[test]
fn fixture_table_covers_required_scenarios() {
    let rows = fixtures();

    let has_allowed_integration = rows.iter().any(|r| {
        r.expected == Expected::Allowed
            && (r.action.starts_with("events:PutRule")
                || r.action.starts_with("events:DeleteRule")
                || r.action.starts_with("sqs:ReceiveMessage")
                || r.action.starts_with("sqs:DeleteMessage")
                || r.action.starts_with("sqs:ChangeMessageVisibility"))
    });
    assert!(
        has_allowed_integration,
        "missing allowed integration scenarios"
    );

    let has_denied_infra = rows.iter().any(|r| {
        r.expected == Expected::Denied
            && (r.action == "events:DeleteEventBus"
                || r.action == "sqs:DeleteQueue"
                || r.action == "sqs:SetQueueAttributes"
                || r.action == "sqs:PurgeQueue")
    });
    assert!(has_denied_infra, "missing denied infrastructure scenarios");

    let has_unrelated = rows.iter().any(|r| {
        r.expected == Expected::Allowed
            && !r.resource.contains(&format!("{ORG}-"))
            && (r.action == "events:DeleteEventBus"
                || r.action == "sqs:DeleteQueue"
                || r.action == "sqs:SetQueueAttributes")
    });
    assert!(
        has_unrelated,
        "missing unrelated-resource control scenarios"
    );

    let has_tag_bypass = rows.iter().any(|r| {
        r.expected == Expected::Denied
            && (r.action == "events:TagResource"
                || r.action == "events:UntagResource"
                || r.action == "sqs:TagQueue"
                || r.action == "sqs:UntagQueue")
    });
    assert!(has_tag_bypass, "missing tag-bypass scenarios");

    let has_role_escalation = rows.iter().any(|r| {
        r.expected == Expected::Denied
            && r.resource.contains("konductor-trust-role")
            && (r.action == "iam:UpdateAssumeRolePolicy"
                || r.action == "iam:AttachRolePolicy"
                || r.action == "iam:PassRole")
    });
    assert!(
        has_role_escalation,
        "missing protected-role escalation scenarios"
    );

    let has_leave_org = rows
        .iter()
        .any(|r| r.expected == Expected::Denied && r.action == "organizations:LeaveOrganization");
    assert!(has_leave_org, "missing LeaveOrganization denial");
}
