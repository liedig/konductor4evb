//! Typed grant-model primitives for the authorization bindings store: the
//! principal types that can hold a role binding, the resource roles, the
//! binding and audit item shapes, and the DynamoDB key construction shared by
//! the store, the engines, and the administration API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A failure to construct a valid grant-model value.
///
/// The model rejects invalid states at construction so stored bindings can
/// never carry a malformed principal, an unkeyable segment, or a global
/// (domain-less) scope.
#[derive(Debug, thiserror::Error)]
pub enum GrantModelError {
    #[error("AWS account id must be exactly 12 ASCII digits: {0:?}")]
    InvalidAccountId(String),
    #[error("Cognito issuer must be an https URL: {0:?}")]
    InvalidIssuer(String),
    #[error("team group must be non-empty")]
    EmptyTeamGroup,
    #[error("binding domain must be non-empty; global bindings are not representable")]
    EmptyDomain,
    #[error("key segment must not contain '#': {0:?}")]
    UnsafeKeySegment(String),
    #[error("trusted actor must be non-empty")]
    EmptyActor,
}

/// A principal that can hold a role binding: an issuer-qualified Cognito team
/// group for OAuth callers, or a twelve-digit AWS account for IAM callers.
///
/// The serde shape matches the spike's frozen fixture manifest so fixture
/// principals deserialize directly into this type. Deserialization runs the
/// validating constructors, so a malformed principal is unrepresentable
/// whether it arrives through code, fixtures, or an API request body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum BindingPrincipal {
    /// An issuer-qualified Cognito team group.
    #[serde(rename = "cognito-group")]
    CognitoGroup { issuer: String, group: String },
    /// A twelve-digit AWS account; IAM role, user, and session ARNs within the
    /// account never select a different binding.
    #[serde(rename = "aws-account", rename_all = "camelCase")]
    AwsAccount { account_id: String },
}

impl<'de> Deserialize<'de> for BindingPrincipal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "kebab-case")]
        enum Shadow {
            #[serde(rename = "cognito-group")]
            CognitoGroup { issuer: String, group: String },
            #[serde(rename = "aws-account", rename_all = "camelCase")]
            AwsAccount { account_id: String },
        }
        match Shadow::deserialize(deserializer)? {
            Shadow::CognitoGroup { issuer, group } => Self::cognito_group(issuer, group),
            Shadow::AwsAccount { account_id } => Self::aws_account(account_id),
        }
        .map_err(serde::de::Error::custom)
    }
}

impl BindingPrincipal {
    /// Builds an issuer-qualified Cognito team-group principal.
    pub fn cognito_group(
        issuer: impl Into<String>,
        group: impl Into<String>,
    ) -> Result<Self, GrantModelError> {
        let issuer = issuer.into();
        let group = group.into();
        if !issuer.starts_with("https://") {
            return Err(GrantModelError::InvalidIssuer(issuer));
        }
        if group.is_empty() {
            return Err(GrantModelError::EmptyTeamGroup);
        }
        reject_unsafe_segment(&issuer)?;
        reject_unsafe_segment(&group)?;
        Ok(Self::CognitoGroup { issuer, group })
    }

    /// Builds an AWS-account principal from a twelve-digit account id.
    pub fn aws_account(account_id: impl Into<String>) -> Result<Self, GrantModelError> {
        let account_id = account_id.into();
        if account_id.len() != 12 || !account_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(GrantModelError::InvalidAccountId(account_id));
        }
        Ok(Self::AwsAccount { account_id })
    }

    /// The principal portion of the binding partition key, e.g.
    /// `cognito-group#https://issuer#team-orders` or `aws-account#111122223333`.
    pub fn key_segment(&self) -> String {
        match self {
            Self::CognitoGroup { issuer, group } => {
                format!("cognito-group#{issuer}#{group}")
            }
            Self::AwsAccount { account_id } => format!("aws-account#{account_id}"),
        }
    }
}

/// The resource role a binding grants on its domain. `Owner` authorizes
/// domain-management operations (metadata update, full-record reads) and
/// never substitutes for `Publisher` or `Subscriber`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceRole {
    Publisher,
    Subscriber,
    Owner,
}

impl ResourceRole {
    /// The stable lowercase token used in keys and stored items.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Publisher => "publisher",
            Self::Subscriber => "subscriber",
            Self::Owner => "owner",
        }
    }
}

/// The lifecycle state of a stored binding. Revocation is a durable delete, so
/// `Active` is the only state a stored binding can carry; revoked history lives
/// only in audit records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BindingState {
    Active,
}

/// A role binding between a typed principal and a domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleBinding {
    pub binding_id: String,
    pub principal: BindingPrincipal,
    pub role: ResourceRole,
    pub domain: String,
    pub state: BindingState,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
}

impl RoleBinding {
    /// Builds an active binding, rejecting global (domain-less) scope and
    /// unkeyable segments at the model level.
    pub fn new(
        binding_id: impl Into<String>,
        principal: BindingPrincipal,
        role: ResourceRole,
        domain: impl Into<String>,
        created_at: DateTime<Utc>,
        created_by: impl Into<String>,
    ) -> Result<Self, GrantModelError> {
        let domain = domain.into();
        let created_by = created_by.into();
        if domain.is_empty() {
            return Err(GrantModelError::EmptyDomain);
        }
        reject_unsafe_segment(&domain)?;
        if created_by.is_empty() {
            return Err(GrantModelError::EmptyActor);
        }
        Ok(Self {
            binding_id: binding_id.into(),
            principal,
            role,
            domain,
            state: BindingState::Active,
            created_at,
            created_by,
        })
    }

    /// `PRINCIPAL#{principal key segment}` — the binding partition key.
    pub fn pk(&self) -> String {
        format!("PRINCIPAL#{}", self.principal.key_segment())
    }

    /// `ROLE#{role}#DOMAIN#{domain}` — the binding sort key.
    pub fn sk(&self) -> String {
        format!("ROLE#{}#DOMAIN#{}", self.role.as_str(), self.domain)
    }

    /// `DOMAIN#{domain}` — the admin GSI partition key for domain enumeration.
    pub fn gsi1pk(&self) -> String {
        format!("DOMAIN#{}", self.domain)
    }

    /// `ROLE#{role}#PRINCIPAL#{principal key segment}` — the admin GSI sort key.
    pub fn gsi1sk(&self) -> String {
        format!(
            "ROLE#{}#PRINCIPAL#{}",
            self.role.as_str(),
            self.principal.key_segment()
        )
    }
}

/// The mutation recorded by an [`AuditRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditMutation {
    Create,
    Revoke,
}

/// A token-free before/after view of a binding for audit history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingSummary {
    pub principal: String,
    pub role: ResourceRole,
    pub domain: String,
    pub state: BindingState,
}

impl From<&RoleBinding> for BindingSummary {
    fn from(binding: &RoleBinding) -> Self {
        Self {
            principal: binding.principal.key_segment(),
            role: binding.role,
            domain: binding.domain.clone(),
            state: binding.state,
        }
    }
}

/// An audit record committed in the same transaction as its binding mutation.
/// Carries mutation type, trusted actor, timestamp, and before/after summary;
/// never tokens, claims material, or IAM principal ARNs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    pub binding_id: String,
    pub timestamp: DateTime<Utc>,
    pub mutation: AuditMutation,
    pub actor: String,
    pub before: Option<BindingSummary>,
    pub after: Option<BindingSummary>,
}

impl AuditRecord {
    /// `GRANT#{binding_id}#AUDIT` — the audit partition key.
    pub fn pk(&self) -> String {
        format!("GRANT#{}#AUDIT", self.binding_id)
    }

    /// `TS#{rfc3339 timestamp}` — the audit sort key; RFC 3339 orders
    /// lexicographically, so audit history is time-ordered within a binding.
    pub fn sk(&self) -> String {
        format!("TS#{}", self.timestamp.to_rfc3339())
    }
}

/// The `POST /bindings` request body.
///
/// Strict: `deny_unknown_fields` means callers cannot supply server-assigned
/// fields (`bindingId`, `state`, `createdAt`, `createdBy`), and construction
/// through [`CreateBindingRequest::into_binding`] keeps global (domain-less)
/// bindings unrepresentable.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateBindingRequest {
    pub domain: String,
    pub role: ResourceRole,
    pub principal: BindingPrincipal,
}

impl CreateBindingRequest {
    /// Builds the binding, assigning server-side identity and audit metadata.
    /// Rejects global scope and unkeyable segments via the model.
    pub fn into_binding(
        self,
        binding_id: impl Into<String>,
        created_at: DateTime<Utc>,
        created_by: impl Into<String>,
    ) -> Result<RoleBinding, GrantModelError> {
        RoleBinding::new(
            binding_id,
            self.principal,
            self.role,
            self.domain,
            created_at,
            created_by,
        )
    }
}

/// The API view of a stored binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingView {
    pub binding_id: String,
    pub domain: String,
    pub role: ResourceRole,
    pub principal: BindingPrincipal,
    pub created_at: String,
    pub created_by: String,
}

impl From<&RoleBinding> for BindingView {
    fn from(binding: &RoleBinding) -> Self {
        Self {
            binding_id: binding.binding_id.clone(),
            domain: binding.domain.clone(),
            role: binding.role,
            principal: binding.principal.clone(),
            created_at: binding.created_at.to_rfc3339(),
            created_by: binding.created_by.clone(),
        }
    }
}

/// The `GET /bindings` response body.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListBindingsResponse {
    pub bindings: Vec<BindingView>,
}

fn reject_unsafe_segment(segment: &str) -> Result<(), GrantModelError> {
    if segment.contains('#') {
        return Err(GrantModelError::UnsafeKeySegment(segment.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

    fn group_principal() -> BindingPrincipal {
        BindingPrincipal::cognito_group(ISSUER, "team-orders").unwrap()
    }

    fn account_principal() -> BindingPrincipal {
        BindingPrincipal::aws_account("111122223333").unwrap()
    }

    fn binding(principal: BindingPrincipal) -> RoleBinding {
        RoleBinding::new(
            "rb-001",
            principal,
            ResourceRole::Publisher,
            "orders",
            DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z")
                .unwrap()
                .to_utc(),
            "platform-admin-sub",
        )
        .unwrap()
    }

    #[test]
    fn aws_account_requires_twelve_digits() {
        assert!(matches!(
            BindingPrincipal::aws_account("12345"),
            Err(GrantModelError::InvalidAccountId(_))
        ));
        assert!(matches!(
            BindingPrincipal::aws_account("11112222333x"),
            Err(GrantModelError::InvalidAccountId(_))
        ));
        assert!(BindingPrincipal::aws_account("111122223333").is_ok());
    }

    #[test]
    fn cognito_group_requires_https_issuer_and_group() {
        assert!(matches!(
            BindingPrincipal::cognito_group("http://issuer", "team"),
            Err(GrantModelError::InvalidIssuer(_))
        ));
        assert!(matches!(
            BindingPrincipal::cognito_group(ISSUER, ""),
            Err(GrantModelError::EmptyTeamGroup)
        ));
    }

    #[test]
    fn key_segments_reject_hash_delimiter() {
        assert!(matches!(
            BindingPrincipal::cognito_group(ISSUER, "team#forged"),
            Err(GrantModelError::UnsafeKeySegment(_))
        ));
        assert!(matches!(
            RoleBinding::new(
                "rb-001",
                account_principal(),
                ResourceRole::Publisher,
                "orders#evil",
                Utc::now(),
                "actor",
            ),
            Err(GrantModelError::UnsafeKeySegment(_))
        ));
    }

    #[test]
    fn global_binding_is_unrepresentable() {
        assert!(matches!(
            RoleBinding::new(
                "rb-001",
                account_principal(),
                ResourceRole::Publisher,
                "",
                Utc::now(),
                "actor",
            ),
            Err(GrantModelError::EmptyDomain)
        ));
    }

    #[test]
    fn binding_keys_match_documented_patterns() {
        let group_binding = binding(group_principal());
        assert_eq!(
            group_binding.pk(),
            format!("PRINCIPAL#cognito-group#{ISSUER}#team-orders")
        );
        assert_eq!(group_binding.sk(), "ROLE#publisher#DOMAIN#orders");
        assert_eq!(group_binding.gsi1pk(), "DOMAIN#orders");
        assert_eq!(
            group_binding.gsi1sk(),
            format!("ROLE#publisher#PRINCIPAL#cognito-group#{ISSUER}#team-orders")
        );

        let account_binding = binding(account_principal());
        assert_eq!(account_binding.pk(), "PRINCIPAL#aws-account#111122223333");
    }

    #[test]
    fn audit_keys_are_time_ordered_per_binding() {
        let record = AuditRecord {
            binding_id: "rb-001".to_string(),
            timestamp: DateTime::parse_from_rfc3339("2026-08-01T12:00:00Z")
                .unwrap()
                .to_utc(),
            mutation: AuditMutation::Create,
            actor: "platform-admin-sub".to_string(),
            before: None,
            after: Some(BindingSummary::from(&binding(account_principal()))),
        };
        assert_eq!(record.pk(), "GRANT#rb-001#AUDIT");
        assert_eq!(record.sk(), "TS#2026-08-01T12:00:00+00:00");
    }

    #[test]
    fn fixture_manifest_principal_shape_deserializes() {
        let group: BindingPrincipal = serde_json::from_value(serde_json::json!({
            "type": "cognito-group",
            "issuer": ISSUER,
            "group": "payments-publishers"
        }))
        .unwrap();
        assert_eq!(
            group,
            BindingPrincipal::CognitoGroup {
                issuer: ISSUER.to_string(),
                group: "payments-publishers".to_string()
            }
        );

        let account: BindingPrincipal = serde_json::from_value(serde_json::json!({
            "type": "aws-account",
            "accountId": "111122223333"
        }))
        .unwrap();
        assert_eq!(
            account,
            BindingPrincipal::AwsAccount {
                account_id: "111122223333".to_string()
            }
        );
    }

    #[test]
    fn create_request_parses_documented_shapes() {
        let group: CreateBindingRequest = serde_json::from_value(serde_json::json!({
            "domain": "orders",
            "role": "publisher",
            "principal": {"type": "cognito-group", "issuer": ISSUER, "group": "team-orders"}
        }))
        .unwrap();
        assert_eq!(group.domain, "orders");
        assert_eq!(group.role, ResourceRole::Publisher);

        let account: CreateBindingRequest = serde_json::from_value(serde_json::json!({
            "domain": "analytics",
            "role": "subscriber",
            "principal": {"type": "aws-account", "accountId": "111122223333"}
        }))
        .unwrap();
        assert_eq!(account.role, ResourceRole::Subscriber);
    }

    #[test]
    fn create_request_rejects_server_assigned_fields() {
        for field in ["bindingId", "state", "createdAt", "createdBy"] {
            let body = serde_json::json!({
                "domain": "orders",
                "role": "publisher",
                "principal": {"type": "aws-account", "accountId": "111122223333"},
                field: "caller-supplied"
            });
            assert!(
                serde_json::from_value::<CreateBindingRequest>(body).is_err(),
                "callers must not supply {field}"
            );
        }
    }

    #[test]
    fn create_request_cannot_build_global_binding() {
        let request = CreateBindingRequest {
            domain: String::new(),
            role: ResourceRole::Publisher,
            principal: account_principal(),
        };
        assert!(matches!(
            request.into_binding("rb-x", Utc::now(), "actor"),
            Err(GrantModelError::EmptyDomain)
        ));
    }

    #[test]
    fn binding_view_serializes_camel_case() {
        let view = BindingView::from(&binding(account_principal()));
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["bindingId"], "rb-001");
        assert_eq!(json["domain"], "orders");
        assert_eq!(json["role"], "publisher");
        assert_eq!(json["principal"]["type"], "aws-account");
        assert_eq!(json["createdAt"], "2026-08-01T00:00:00+00:00");
        assert_eq!(json["createdBy"], "platform-admin-sub");
    }

    #[test]
    fn deserialization_rejects_invalid_principals() {
        for body in [
            serde_json::json!({"type": "aws-account", "accountId": "not-digits!"}),
            serde_json::json!({"type": "aws-account", "accountId": "123"}),
            serde_json::json!({"type": "cognito-group", "issuer": "http://issuer", "group": "team"}),
            serde_json::json!({"type": "cognito-group", "issuer": ISSUER, "group": "team#forged"}),
            serde_json::json!({"type": "cognito-group", "issuer": ISSUER, "group": ""}),
        ] {
            assert!(
                serde_json::from_value::<BindingPrincipal>(body.clone()).is_err(),
                "must reject {body}"
            );
        }
    }

    #[test]
    fn role_and_state_serde_tokens_are_stable() {
        assert_eq!(
            serde_json::to_value(ResourceRole::Publisher).unwrap(),
            "publisher"
        );
        assert_eq!(
            serde_json::to_value(ResourceRole::Subscriber).unwrap(),
            "subscriber"
        );
        assert_eq!(serde_json::to_value(ResourceRole::Owner).unwrap(), "owner");
        assert_eq!(
            serde_json::to_value(BindingState::Active).unwrap(),
            "active"
        );
        assert_eq!(
            serde_json::to_value(AuditMutation::Revoke).unwrap(),
            "revoke"
        );
    }

    #[test]
    fn owner_role_uses_the_stable_owner_token() {
        assert_eq!(ResourceRole::Owner.as_str(), "owner");
        let binding = RoleBinding::new(
            "rb-owner",
            group_principal(),
            ResourceRole::Owner,
            "orders",
            Utc::now(),
            "fixture",
        )
        .unwrap();
        assert_eq!(binding.sk(), "ROLE#owner#DOMAIN#orders");
        assert_eq!(
            binding.gsi1sk(),
            format!("ROLE#owner#PRINCIPAL#cognito-group#{ISSUER}#team-orders")
        );
    }

    #[test]
    fn create_request_accepts_owner_role() {
        let request: CreateBindingRequest = serde_json::from_value(serde_json::json!({
            "domain": "orders",
            "role": "owner",
            "principal": {"type": "cognito-group", "issuer": ISSUER, "group": "team-orders"}
        }))
        .unwrap();
        assert_eq!(request.role, ResourceRole::Owner);
    }

    #[test]
    fn role_parsing_stays_closed_to_unknown_values() {
        for role in ["admin", "member", "Owner", "OWNER"] {
            let body = serde_json::json!({
                "domain": "orders",
                "role": role,
                "principal": {"type": "aws-account", "accountId": "111122223333"}
            });
            assert!(
                serde_json::from_value::<CreateBindingRequest>(body).is_err(),
                "must reject role {role:?}"
            );
        }
    }
}
