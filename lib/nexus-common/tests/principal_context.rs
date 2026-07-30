//! Fail-closed reconstruction of the trusted principal from the propagation
//! surfaces (Section 5): SQS message-attribute envelope and the API Gateway
//! proxy request context.

use std::collections::HashMap;

use nexus_common::principal::{
    AuthenticationMechanism, AuthorizationDecision, FoundationAuthorizer, GlobalRole,
    ResourceAuthorizer, TrustedPrincipal,
};
use nexus_common::principal_context::{attr, log_principal, PrincipalContextError};

fn attrs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn oauth_envelope() -> HashMap<String, String> {
    attrs(&[
        (attr::VERSION, "1"),
        (attr::MECHANISM, "oauth"),
        (attr::PRINCIPAL_ID, "a1b2c3d4-1111"),
        (attr::ISSUER, "https://cognito-idp.example/pool"),
        (attr::CLIENT_ID, "client123"),
        (attr::GLOBAL_ROLES, r#"["platform-admin"]"#),
        (attr::TEAM_GROUPS, r#"["team-payments","team-orders"]"#),
    ])
}

#[test]
fn sqs_oauth_envelope_preserves_team_groups_and_global_roles() {
    let principal = TrustedPrincipal::from_sqs_attributes(&oauth_envelope()).unwrap();
    assert_eq!(
        principal.authentication_mechanism(),
        AuthenticationMechanism::Oauth
    );
    assert_eq!(principal.principal_id(), "a1b2c3d4-1111");
    assert_eq!(principal.global_roles(), &[GlobalRole::PlatformAdmin]);
    assert_eq!(
        principal.team_groups(),
        &["team-payments".to_string(), "team-orders".to_string()]
    );
}

#[test]
fn sqs_oauth_envelope_treats_team_groups_as_opaque_not_entitlements() {
    // A team group that shares a name with a controlled global-role group must
    // not be promoted to a permission; it is carried opaquely only.
    let mut env = oauth_envelope();
    env.insert(attr::GLOBAL_ROLES.into(), "[]".into());
    env.insert(attr::TEAM_GROUPS.into(), r#"["platform-admin"]"#.into());
    let principal = TrustedPrincipal::from_sqs_attributes(&env).unwrap();
    assert!(principal.global_roles().is_empty());
    assert!(!principal.is_platform_admin());
    assert_eq!(principal.team_groups(), &["platform-admin".to_string()]);
}

#[test]
fn sqs_oauth_envelope_rejects_uncontrolled_global_role() {
    let mut env = oauth_envelope();
    env.insert(attr::GLOBAL_ROLES.into(), r#"["team-payments"]"#.into());
    let err = TrustedPrincipal::from_sqs_attributes(&env).unwrap_err();
    assert!(matches!(err, PrincipalContextError::MalformedField { .. }));
}

#[test]
fn sqs_iam_envelope_binds_account_despite_differing_principal_arns() {
    let base = [
        (attr::VERSION, "1"),
        (attr::MECHANISM, "iam"),
        (attr::ACCOUNT_ID, "123456789012"),
    ];
    let no_arn = TrustedPrincipal::from_sqs_attributes(&attrs(&base)).unwrap();
    let role_arn = TrustedPrincipal::from_sqs_attributes(&attrs(&[
        (attr::VERSION, "1"),
        (attr::MECHANISM, "iam"),
        (attr::ACCOUNT_ID, "123456789012"),
        (attr::PRINCIPAL_ARN, "arn:aws:iam::123456789012:role/Admin"),
    ]))
    .unwrap();
    let user_arn = TrustedPrincipal::from_sqs_attributes(&attrs(&[
        (attr::VERSION, "1"),
        (attr::MECHANISM, "iam"),
        (attr::ACCOUNT_ID, "123456789012"),
        (attr::PRINCIPAL_ARN, "arn:aws:iam::123456789012:user/alice"),
    ]))
    .unwrap();

    assert_eq!(no_arn.principal_id(), "123456789012");
    assert_eq!(role_arn.principal_id(), no_arn.principal_id());
    assert_eq!(user_arn.principal_id(), no_arn.principal_id());
}

#[test]
fn sqs_envelope_fails_closed_on_missing_version() {
    let mut env = oauth_envelope();
    env.remove(attr::VERSION);
    let err = TrustedPrincipal::from_sqs_attributes(&env).unwrap_err();
    assert!(matches!(
        err,
        PrincipalContextError::MissingField(attr::VERSION)
    ));
}

#[test]
fn sqs_envelope_fails_closed_on_unsupported_version() {
    let mut env = oauth_envelope();
    env.insert(attr::VERSION.into(), "2".into());
    let err = TrustedPrincipal::from_sqs_attributes(&env).unwrap_err();
    assert!(matches!(err, PrincipalContextError::UnsupportedVersion(_)));
}

#[test]
fn sqs_envelope_fails_closed_on_unknown_mechanism() {
    let mut env = oauth_envelope();
    env.insert(attr::MECHANISM.into(), "api-key".into());
    let err = TrustedPrincipal::from_sqs_attributes(&env).unwrap_err();
    assert!(matches!(err, PrincipalContextError::UnknownMechanism(_)));
}

#[test]
fn sqs_envelope_fails_closed_on_malformed_roles() {
    let mut env = oauth_envelope();
    env.insert(attr::GLOBAL_ROLES.into(), "not-json".into());
    let err = TrustedPrincipal::from_sqs_attributes(&env).unwrap_err();
    assert!(matches!(err, PrincipalContextError::MalformedField { .. }));
}

#[test]
fn sqs_envelope_fails_closed_on_missing_oauth_field() {
    let mut env = oauth_envelope();
    env.remove(attr::ISSUER);
    let err = TrustedPrincipal::from_sqs_attributes(&env).unwrap_err();
    assert!(matches!(
        err,
        PrincipalContextError::MissingField(attr::ISSUER)
    ));
}

#[test]
fn proxy_oauth_context_parses_principal() {
    let ctx = serde_json::json!({
        "authorizer": {
            "authenticationMechanism": "oauth",
            "principalId": "a1b2c3d4-1111",
            "issuer": "https://cognito-idp.example/pool",
            "clientId": "client123",
            "globalRoles": "[\"platform-auditor\"]",
            "teamGroups": "[\"team-payments\"]"
        }
    });
    let principal = TrustedPrincipal::from_proxy_request_context(&ctx).unwrap();
    assert_eq!(
        principal.authentication_mechanism(),
        AuthenticationMechanism::Oauth
    );
    assert_eq!(principal.principal_id(), "a1b2c3d4-1111");
    assert_eq!(principal.global_roles(), &[GlobalRole::PlatformAuditor]);
    assert_eq!(principal.team_groups(), &["team-payments".to_string()]);
}

#[test]
fn proxy_iam_context_parses_account_principal() {
    let ctx = serde_json::json!({
        "identity": {
            "accountId": "123456789012",
            "userArn": "arn:aws:sts::123456789012:assumed-role/Deploy/ci"
        }
    });
    let principal = TrustedPrincipal::from_proxy_request_context(&ctx).unwrap();
    assert_eq!(
        principal.authentication_mechanism(),
        AuthenticationMechanism::Iam
    );
    assert_eq!(principal.principal_id(), "123456789012");
}

#[test]
fn proxy_missing_context_fails_closed() {
    let ctx = serde_json::json!({});
    let err = TrustedPrincipal::from_proxy_request_context(&ctx).unwrap_err();
    assert!(matches!(err, PrincipalContextError::MissingContext));
}

#[tokio::test]
async fn foundation_authorizer_retains_iam_and_denies_oauth() {
    let authorizer = FoundationAuthorizer;
    let iam = TrustedPrincipal::iam("123456789012", None);
    let oauth = TrustedPrincipal::from_sqs_attributes(&oauth_envelope()).unwrap();

    assert_eq!(
        authorizer
            .authorize(&iam, "domains:Register", "domain:x")
            .await,
        AuthorizationDecision::Allow
    );
    assert_eq!(
        authorizer
            .authorize(&oauth, "domains:Register", "domain:x")
            .await,
        AuthorizationDecision::Deny
    );
}

#[test]
fn log_principal_returns_iam_from_proxy_context() {
    let ctx = serde_json::json!({
        "identity": {
            "accountId": "111122223333",
            "userArn": "arn:aws:sts::111122223333:assumed-role/nexus-trust-role/session"
        }
    });
    let principal = log_principal(&ctx).unwrap();
    assert_eq!(
        principal.authentication_mechanism(),
        AuthenticationMechanism::Iam
    );
    assert_eq!(principal.principal_id(), "111122223333");
    assert_eq!(principal.iam_account_id(), Some("111122223333"));
}

#[test]
fn log_principal_returns_oauth_from_authorizer_context() {
    let ctx = serde_json::json!({
        "authorizer": {
            "authenticationMechanism": "oauth",
            "principalId": "user-abc-123",
            "issuer": "https://cognito-idp.example.com/pool",
            "clientId": "client123",
            "globalRoles": "[\"platform-admin\"]",
            "teamGroups": "[\"team-payments\"]"
        }
    });
    let principal = log_principal(&ctx).unwrap();
    assert_eq!(
        principal.authentication_mechanism(),
        AuthenticationMechanism::Oauth
    );
    assert_eq!(principal.principal_id(), "user-abc-123");
    assert_eq!(principal.iam_account_id(), None);
    assert_eq!(principal.global_roles(), &[GlobalRole::PlatformAdmin]);
}

#[test]
fn log_principal_returns_error_for_missing_context() {
    let ctx = serde_json::json!({});
    let err = log_principal(&ctx).unwrap_err();
    assert!(matches!(err, PrincipalContextError::MissingContext));
}
