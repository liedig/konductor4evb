use serde::{Deserialize, Serialize};

/// Wire version of the trusted-principal contract. Consumers must reject any
/// serialized principal whose version they do not understand.
pub const TRUSTED_PRINCIPAL_VERSION: u32 = 1;

/// The authentication mechanism that produced a [`TrustedPrincipal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthenticationMechanism {
    /// Interactive user authenticated by a Cognito OAuth access token.
    Oauth,
    /// AWS caller authenticated by Signature Version 4.
    Iam,
}

/// A coarse, platform-wide role mapped from a controlled Cognito group.
///
/// These are the only global roles Nexus recognizes at this foundation stage;
/// publisher and subscriber access is intentionally not represented here and is
/// never inferred from team-group identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlobalRole {
    #[serde(rename = "platform-admin")]
    PlatformAdmin,
    #[serde(rename = "platform-auditor")]
    PlatformAuditor,
}

impl GlobalRole {
    /// The controlled Cognito group name that maps to this role.
    pub fn as_group_name(&self) -> &'static str {
        match self {
            Self::PlatformAdmin => "platform-admin",
            Self::PlatformAuditor => "platform-auditor",
        }
    }

    /// Maps a validated `cognito:groups` value to a global role. Returns `None`
    /// for any group that is not a controlled global-role group; such groups
    /// are carried opaquely as team-group identifiers, never as permissions.
    pub fn from_group_name(group: &str) -> Option<Self> {
        match group {
            "platform-admin" => Some(Self::PlatformAdmin),
            "platform-auditor" => Some(Self::PlatformAuditor),
            _ => None,
        }
    }
}

/// OAuth-specific provenance for an interactive [`TrustedPrincipal`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OAuthPrincipal {
    /// The immutable Cognito `sub` claim; the OAuth `principalId`.
    pub subject: String,
    /// The user-pool issuer that produced the access token.
    pub issuer: String,
    /// The OAuth app client the access token was issued for.
    pub client_id: String,
    /// Coarse global roles mapped from controlled Cognito groups.
    pub global_roles: Vec<GlobalRole>,
    /// Validated non-global-role Cognito group values, carried opaquely as
    /// trusted team-group identifiers for later resource binding. They are
    /// identity attributes, never precomputed entitlements.
    pub team_groups: Vec<String>,
}

/// IAM-specific provenance for an AWS [`TrustedPrincipal`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IamPrincipal {
    /// The authenticated AWS account ID; the IAM `principalId` and the
    /// account-granular resource-binding identity.
    pub account_id: String,
    /// Optional IAM principal ARN retained only as audit provenance. It must
    /// never introduce role- or user-level authorization granularity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_arn: Option<String>,
}

/// The authenticated identity of a control-plane caller, normalized from the
/// API surface that authenticated it.
///
/// The `authenticationMechanism` field is the mechanism discriminator and the
/// mechanism-specific fields are mandatory. A serialized context that omits the
/// discriminator, names an unknown mechanism, or mixes fields from another
/// mechanism fails to deserialize, so identity is never silently reinterpreted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "authenticationMechanism")]
pub enum TrustedPrincipal {
    /// An interactive user authenticated on the OAuth surface.
    #[serde(rename = "oauth")]
    OAuth(OAuthPrincipal),
    /// An AWS caller authenticated on the IAM surface.
    #[serde(rename = "iam")]
    Iam(IamPrincipal),
}

impl TrustedPrincipal {
    /// Builds an OAuth principal from validated access-token claims.
    pub fn oauth(
        subject: impl Into<String>,
        issuer: impl Into<String>,
        client_id: impl Into<String>,
        global_roles: Vec<GlobalRole>,
        team_groups: Vec<String>,
    ) -> Self {
        Self::OAuth(OAuthPrincipal {
            subject: subject.into(),
            issuer: issuer.into(),
            client_id: client_id.into(),
            global_roles,
            team_groups,
        })
    }

    /// Builds an IAM principal bound to an AWS account, with optional principal
    /// ARN audit provenance.
    pub fn iam(account_id: impl Into<String>, principal_arn: Option<String>) -> Self {
        Self::Iam(IamPrincipal {
            account_id: account_id.into(),
            principal_arn,
        })
    }

    /// The mechanism that authenticated this principal.
    pub fn authentication_mechanism(&self) -> AuthenticationMechanism {
        match self {
            Self::OAuth(_) => AuthenticationMechanism::Oauth,
            Self::Iam(_) => AuthenticationMechanism::Iam,
        }
    }

    /// The normalized principal identifier and resource-binding identity: the
    /// Cognito subject for OAuth, the AWS account ID for IAM. For IAM this is
    /// always the account regardless of any principal ARN provenance.
    pub fn principal_id(&self) -> &str {
        match self {
            Self::OAuth(p) => &p.subject,
            Self::Iam(p) => &p.account_id,
        }
    }

    /// The AWS account ID for IAM principals, or `None` for OAuth principals
    /// who have no account-level binding without the resource-policy engine.
    pub fn iam_account_id(&self) -> Option<&str> {
        match self {
            Self::Iam(p) => Some(&p.account_id),
            Self::OAuth(_) => None,
        }
    }

    /// The coarse global roles for this principal (always empty for IAM).
    pub fn global_roles(&self) -> &[GlobalRole] {
        match self {
            Self::OAuth(p) => &p.global_roles,
            Self::Iam(_) => &[],
        }
    }

    /// The trusted team-group identifiers (always empty for IAM).
    pub fn team_groups(&self) -> &[String] {
        match self {
            Self::OAuth(p) => &p.team_groups,
            Self::Iam(_) => &[],
        }
    }

    /// `true` when this principal carries the `platform-admin` global role.
    pub fn is_platform_admin(&self) -> bool {
        self.global_roles().contains(&GlobalRole::PlatformAdmin)
    }
}

/// The allow/deny outcome of a resource-authorization check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationDecision {
    Allow,
    Deny,
}

impl AuthorizationDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// The resource-authorization enforcement boundary for control-plane commands.
///
/// Implementations evaluate a [`TrustedPrincipal`] against a target resource
/// and return only allow or deny; they MUST fail closed (return
/// [`AuthorizationDecision::Deny`]) on any internal error. The DynamoDB-backed
/// grant engine validated by the authorization-engine spike will provide the
/// production implementation; this trait and its decision type are the stable
/// contract consumers depend on now.
pub trait ResourceAuthorizer {
    /// Authorizes `principal` to perform `action` on `resource`.
    fn authorize(
        &self,
        principal: &TrustedPrincipal,
        action: &str,
        resource: &str,
    ) -> impl std::future::Future<Output = AuthorizationDecision> + Send;
}

/// A [`ResourceAuthorizer`] that denies every request.
///
/// Useful as the wired default on paths that must stay closed until a
/// production engine is explicitly wired in.
#[derive(Debug, Default, Clone, Copy)]
pub struct DenyAllAuthorizer;

impl ResourceAuthorizer for DenyAllAuthorizer {
    async fn authorize(
        &self,
        _principal: &TrustedPrincipal,
        _action: &str,
        _resource: &str,
    ) -> AuthorizationDecision {
        AuthorizationDecision::Deny
    }
}

/// The standing guard for platform-operated control-plane paths: IAM callers
/// allow (platform teams authenticate with IAM and drive approvals and
/// provisioning), OAuth callers deny.
///
/// Wired into domain registration and lifecycle (`domain-registrations`),
/// which platform teams operate. Publisher and subscriber paths used by
/// application teams authorize through `DynamoGrantEngine` (see
/// `grant_engine`) instead; do not wire this authorizer into those paths.
#[derive(Debug, Default, Clone, Copy)]
pub struct IamOnlyAuthorizer;

impl ResourceAuthorizer for IamOnlyAuthorizer {
    async fn authorize(
        &self,
        principal: &TrustedPrincipal,
        _action: &str,
        _resource: &str,
    ) -> AuthorizationDecision {
        match principal.authentication_mechanism() {
            AuthenticationMechanism::Iam => AuthorizationDecision::Allow,
            AuthenticationMechanism::Oauth => AuthorizationDecision::Deny,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oauth_principal() -> TrustedPrincipal {
        TrustedPrincipal::oauth(
            "a1b2c3d4-1111-2222-3333-444455556666",
            "https://cognito-idp.ap-southeast-2.amazonaws.com/ap-southeast-2_example",
            "client123",
            vec![GlobalRole::PlatformAdmin],
            vec!["team-payments".to_string(), "team-orders".to_string()],
        )
    }

    #[test]
    fn oauth_principal_round_trips_preserving_team_groups() {
        let principal = oauth_principal();
        let json = serde_json::to_string(&principal).unwrap();
        let parsed: TrustedPrincipal = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, principal);
        assert_eq!(
            parsed.team_groups(),
            &["team-payments".to_string(), "team-orders".to_string()]
        );
        assert_eq!(parsed.global_roles(), &[GlobalRole::PlatformAdmin]);
        assert_eq!(
            parsed.principal_id(),
            "a1b2c3d4-1111-2222-3333-444455556666"
        );
        assert_eq!(
            parsed.authentication_mechanism(),
            AuthenticationMechanism::Oauth
        );
    }

    #[test]
    fn oauth_wire_format_carries_mechanism_discriminator() {
        let value = serde_json::to_value(oauth_principal()).unwrap();
        assert_eq!(value["authenticationMechanism"], "oauth");
        assert_eq!(value["subject"], "a1b2c3d4-1111-2222-3333-444455556666");
        assert_eq!(value["globalRoles"], serde_json::json!(["platform-admin"]));
        assert_eq!(
            value["teamGroups"],
            serde_json::json!(["team-payments", "team-orders"])
        );
    }

    #[test]
    fn iam_principal_uses_account_as_principal_id() {
        let principal = TrustedPrincipal::iam("123456789012", None);
        assert_eq!(principal.principal_id(), "123456789012");
        assert_eq!(
            principal.authentication_mechanism(),
            AuthenticationMechanism::Iam
        );
        assert!(principal.global_roles().is_empty());
        assert!(principal.team_groups().is_empty());
        assert!(!principal.is_platform_admin());
    }

    #[test]
    fn iam_principal_arn_does_not_alter_account_level_identity() {
        let plain = TrustedPrincipal::iam("123456789012", None);
        let with_arn = TrustedPrincipal::iam(
            "123456789012",
            Some("arn:aws:sts::123456789012:assumed-role/SomeRole/session".to_string()),
        );
        assert_eq!(plain.principal_id(), with_arn.principal_id());
        assert_eq!(with_arn.principal_id(), "123456789012");
        let value = serde_json::to_value(&with_arn).unwrap();
        assert_eq!(value["authenticationMechanism"], "iam");
        assert_eq!(value["accountId"], "123456789012");
    }

    #[test]
    fn iam_principal_arn_is_optional_audit_provenance() {
        let with_arn = TrustedPrincipal::iam(
            "123456789012",
            Some("arn:aws:iam::123456789012:role/Admin".to_string()),
        );
        let json = serde_json::to_string(&with_arn).unwrap();
        let parsed: TrustedPrincipal = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, with_arn);
        let without_arn = TrustedPrincipal::iam("123456789012", None);
        let value = serde_json::to_value(&without_arn).unwrap();
        assert!(value.get("principalArn").is_none());
    }

    #[test]
    fn missing_mechanism_discriminator_fails_closed() {
        let value = serde_json::json!({
            "subject": "a1b2c3d4",
            "issuer": "https://issuer",
            "clientId": "client",
            "globalRoles": [],
            "teamGroups": []
        });
        let parsed: Result<TrustedPrincipal, _> = serde_json::from_value(value);
        assert!(parsed.is_err());
    }

    #[test]
    fn unknown_mechanism_fails_closed() {
        let value = serde_json::json!({
            "authenticationMechanism": "api-key",
            "subject": "a1b2c3d4"
        });
        let parsed: Result<TrustedPrincipal, _> = serde_json::from_value(value);
        assert!(parsed.is_err());
    }

    #[test]
    fn missing_required_mechanism_fields_fail_closed() {
        let value = serde_json::json!({
            "authenticationMechanism": "oauth",
            "subject": "a1b2c3d4"
        });
        let parsed: Result<TrustedPrincipal, _> = serde_json::from_value(value);
        assert!(parsed.is_err());
    }

    #[test]
    fn mixed_mechanism_context_fails_closed() {
        let iam_with_oauth_fields = serde_json::json!({
            "authenticationMechanism": "iam",
            "accountId": "123456789012",
            "subject": "attacker-subject",
            "globalRoles": ["platform-admin"]
        });
        let parsed: Result<TrustedPrincipal, _> = serde_json::from_value(iam_with_oauth_fields);
        assert!(parsed.is_err());

        let oauth_with_iam_fields = serde_json::json!({
            "authenticationMechanism": "oauth",
            "subject": "a1b2c3d4",
            "issuer": "https://issuer",
            "clientId": "client",
            "globalRoles": [],
            "teamGroups": [],
            "accountId": "123456789012"
        });
        let parsed: Result<TrustedPrincipal, _> = serde_json::from_value(oauth_with_iam_fields);
        assert!(parsed.is_err());
    }

    #[test]
    fn global_role_maps_only_controlled_group_names() {
        assert_eq!(
            GlobalRole::from_group_name("platform-admin"),
            Some(GlobalRole::PlatformAdmin)
        );
        assert_eq!(
            GlobalRole::from_group_name("platform-auditor"),
            Some(GlobalRole::PlatformAuditor)
        );
        assert_eq!(GlobalRole::from_group_name("team-payments"), None);
        assert_eq!(GlobalRole::from_group_name("Platform-Admin"), None);
        assert_eq!(GlobalRole::from_group_name(""), None);
    }

    #[test]
    fn global_role_group_name_round_trips() {
        assert_eq!(GlobalRole::PlatformAdmin.as_group_name(), "platform-admin");
        assert_eq!(
            GlobalRole::PlatformAuditor.as_group_name(),
            "platform-auditor"
        );
        for role in [GlobalRole::PlatformAdmin, GlobalRole::PlatformAuditor] {
            assert_eq!(
                GlobalRole::from_group_name(role.as_group_name()),
                Some(role)
            );
        }
    }

    #[tokio::test]
    async fn fail_closed_authorizer_denies_oauth_and_iam() {
        let authorizer = DenyAllAuthorizer;
        let oauth = oauth_principal();
        let iam = TrustedPrincipal::iam("123456789012", None);
        assert_eq!(
            authorizer
                .authorize(&oauth, "domains:Register", "domain:billing")
                .await,
            AuthorizationDecision::Deny
        );
        assert_eq!(
            authorizer
                .authorize(&iam, "domains:Update", "domain:billing")
                .await,
            AuthorizationDecision::Deny
        );
    }
}
