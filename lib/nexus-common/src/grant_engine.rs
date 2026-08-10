//! The production grant engines behind [`ResourceAuthorizer`]: additive
//! role-binding evaluation over the bindings table with default denial,
//! strongly consistent decision reads, and fail-closed error mapping.
//!
//! `DynamoGrantEngine` is the cloud implementation; `InMemoryGrantEngine` is
//! the fixture-fed twin for tests and local development. A shared parity suite
//! (`tests/grant_parity.rs`) proves identical decisions on both.

use std::collections::HashSet;

use tracing::warn;

use crate::grant::{BindingPrincipal, ResourceRole, RoleBinding};
use crate::grant_store::BindingStore;
use crate::principal::{AuthorizationDecision, ResourceAuthorizer, TrustedPrincipal};

/// The action strings the engine recognizes, centralized so call-site typos
/// surface in tests rather than as silent production denials.
///
/// Subscribe-family actions arrive with `add-subscriber-entitlements`, which
/// extends the engine with composite resources and entitlement composition.
pub mod actions {
    /// Publish an event to a producer domain (store-and-forward publish API).
    pub const SCHEMA_PUBLISH: &str = "schema:publish";
    /// Create a schema in a producer domain's registry.
    pub const SCHEMA_CREATE: &str = "schema:create";
    /// Append a version to an existing producer schema.
    pub const SCHEMA_VERSION_APPEND: &str = "schema:version-append";
    /// Create a subscription against a composite resource
    /// (`{consumer}/{producer}/{event}`, see `subscription_resource`).
    pub const SUBSCRIPTION_CREATE: &str = "subscription:create";
    /// Enable a subscription's rules (initial activation or per-environment).
    pub const SUBSCRIPTION_ENABLE: &str = "subscription:enable";
    /// Disable a subscription's rules; binding-only so subscribers retain
    /// lifecycle control after revocation.
    pub const SUBSCRIPTION_DISABLE: &str = "subscription:disable";
    /// Delete a subscription; binding-only like disable.
    pub const SUBSCRIPTION_DELETE: &str = "subscription:delete";
}

fn role_for_action(action: &str) -> Option<ResourceRole> {
    match action {
        actions::SCHEMA_PUBLISH | actions::SCHEMA_CREATE | actions::SCHEMA_VERSION_APPEND => {
            Some(ResourceRole::Publisher)
        }
        _ => None,
    }
}

/// The principals a trusted principal can claim bindings through: each
/// issuer-qualified team group for OAuth, the account for IAM. Groups that
/// cannot form a key segment are dropped; the model rejects `#` segments on
/// both sides, so no binding can ever exist for them.
fn candidate_principals(principal: &TrustedPrincipal) -> Vec<BindingPrincipal> {
    match principal {
        TrustedPrincipal::OAuth(p) => p
            .team_groups
            .iter()
            .filter_map(|g| BindingPrincipal::cognito_group(&p.issuer, g).ok())
            .collect(),
        TrustedPrincipal::Iam(p) => BindingPrincipal::aws_account(&p.account_id)
            .into_iter()
            .collect(),
    }
}

/// The binding-lookup seam both engines share, so the decision loop (action
/// mapping, resource validation, additive union, fail-closed errors, deny
/// logging) exists exactly once.
trait BindingLookup {
    fn has_binding(
        &self,
        principal: &BindingPrincipal,
        role: ResourceRole,
        domain: &str,
    ) -> impl std::future::Future<Output = Result<bool, crate::grant_store::GrantStoreError>> + Send;
}

async fn evaluate(
    lookup: &impl BindingLookup,
    principal: &TrustedPrincipal,
    action: &str,
    resource: &str,
) -> AuthorizationDecision {
    let Some(role) = role_for_action(action) else {
        warn!(
            action,
            resource, "grant decision: deny (unrecognized action)"
        );
        return AuthorizationDecision::Deny;
    };
    if resource.is_empty() || resource.contains('/') || resource.contains('#') {
        warn!(
            action,
            resource, "grant decision: deny (malformed domain resource)"
        );
        return AuthorizationDecision::Deny;
    }
    first_applicable_binding(lookup, principal, role, action, resource).await
}

/// The additive union: the first applicable binding allows; a store error or
/// an exhausted candidate list denies.
async fn first_applicable_binding(
    lookup: &impl BindingLookup,
    principal: &TrustedPrincipal,
    role: ResourceRole,
    action: &str,
    resource: &str,
) -> AuthorizationDecision {
    for candidate in candidate_principals(principal) {
        match lookup.has_binding(&candidate, role, resource).await {
            Ok(true) => return AuthorizationDecision::Allow,
            Ok(false) => {}
            Err(e) => {
                warn!(
                    action,
                    resource,
                    error = %e,
                    "grant decision: deny (binding store error, fail closed)"
                );
                return AuthorizationDecision::Deny;
            }
        }
    }
    warn!(
        action,
        resource,
        mechanism = ?principal.authentication_mechanism(),
        principal_id = %principal.principal_id(),
        "grant decision: deny (no applicable binding)"
    );
    AuthorizationDecision::Deny
}

/// The DynamoDB-backed production engine. Every decision is a strongly
/// consistent point read per candidate principal; the administration GSIs
/// never feed an allow decision.
pub struct DynamoGrantEngine {
    store: BindingStore,
}

impl DynamoGrantEngine {
    pub fn new(store: BindingStore) -> Self {
        Self { store }
    }
}

impl BindingLookup for DynamoGrantEngine {
    async fn has_binding(
        &self,
        principal: &BindingPrincipal,
        role: ResourceRole,
        domain: &str,
    ) -> Result<bool, crate::grant_store::GrantStoreError> {
        let pk = format!("PRINCIPAL#{}", principal.key_segment());
        let sk = format!("ROLE#{}#DOMAIN#{}", role.as_str(), domain);
        Ok(self.store.get_binding(&pk, &sk).await?.is_some())
    }
}

impl ResourceAuthorizer for DynamoGrantEngine {
    async fn authorize(
        &self,
        principal: &TrustedPrincipal,
        action: &str,
        resource: &str,
    ) -> AuthorizationDecision {
        evaluate(self, principal, action, resource).await
    }
}

/// The fixture-fed in-memory twin for tests and local development. Decisions
/// require no AWS credentials or network access.
pub struct InMemoryGrantEngine {
    binding_keys: HashSet<(String, String)>,
}

impl InMemoryGrantEngine {
    /// Builds an engine from active bindings, keyed exactly as the DynamoDB
    /// store keys them so parity fixtures exercise identical key construction.
    pub fn new(bindings: Vec<RoleBinding>) -> Self {
        Self {
            binding_keys: bindings.iter().map(|b| (b.pk(), b.sk())).collect(),
        }
    }
}

impl BindingLookup for InMemoryGrantEngine {
    async fn has_binding(
        &self,
        principal: &BindingPrincipal,
        role: ResourceRole,
        domain: &str,
    ) -> Result<bool, crate::grant_store::GrantStoreError> {
        let pk = format!("PRINCIPAL#{}", principal.key_segment());
        let sk = format!("ROLE#{}#DOMAIN#{}", role.as_str(), domain);
        Ok(self.binding_keys.contains(&(pk, sk)))
    }
}

impl ResourceAuthorizer for InMemoryGrantEngine {
    async fn authorize(
        &self,
        principal: &TrustedPrincipal,
        action: &str,
        resource: &str,
    ) -> AuthorizationDecision {
        evaluate(self, principal, action, resource).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::GlobalRole;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

    fn orders_publisher_binding() -> RoleBinding {
        RoleBinding::new(
            "rb-001",
            BindingPrincipal::cognito_group(ISSUER, "orders-publishers").unwrap(),
            ResourceRole::Publisher,
            "orders",
            chrono::Utc::now(),
            "fixture",
        )
        .unwrap()
    }

    fn oauth(groups: &[&str]) -> TrustedPrincipal {
        TrustedPrincipal::oauth(
            "a1b2c3d4-1111-2222-3333-444455556666",
            ISSUER,
            "client123",
            vec![],
            groups.iter().map(|g| g.to_string()).collect(),
        )
    }

    #[tokio::test]
    async fn in_memory_allows_bound_group_and_denies_otherwise() {
        let engine = InMemoryGrantEngine::new(vec![orders_publisher_binding()]);
        assert_eq!(
            engine
                .authorize(
                    &oauth(&["orders-publishers"]),
                    actions::SCHEMA_PUBLISH,
                    "orders"
                )
                .await,
            AuthorizationDecision::Allow
        );
        assert_eq!(
            engine
                .authorize(
                    &oauth(&["orders-publishers"]),
                    actions::SCHEMA_PUBLISH,
                    "payments"
                )
                .await,
            AuthorizationDecision::Deny
        );
        assert_eq!(
            engine
                .authorize(&oauth(&["team-nobody"]), actions::SCHEMA_PUBLISH, "orders")
                .await,
            AuthorizationDecision::Deny
        );
    }

    #[tokio::test]
    async fn in_memory_denies_unknown_actions_and_malformed_resources() {
        let engine = InMemoryGrantEngine::new(vec![orders_publisher_binding()]);
        let principal = oauth(&["orders-publishers"]);
        assert_eq!(
            engine
                .authorize(&principal, "schema:delete", "orders")
                .await,
            AuthorizationDecision::Deny
        );
        assert_eq!(
            engine
                .authorize(&principal, actions::SCHEMA_PUBLISH, "orders/extra")
                .await,
            AuthorizationDecision::Deny
        );
        assert_eq!(
            engine
                .authorize(&principal, actions::SCHEMA_PUBLISH, "")
                .await,
            AuthorizationDecision::Deny
        );
    }

    #[tokio::test]
    async fn in_memory_iam_account_binding_allows_at_account_granularity() {
        let binding = RoleBinding::new(
            "rb-002",
            BindingPrincipal::aws_account("111122223333").unwrap(),
            ResourceRole::Publisher,
            "orders",
            chrono::Utc::now(),
            "fixture",
        )
        .unwrap();
        let engine = InMemoryGrantEngine::new(vec![binding]);
        let with_arn = TrustedPrincipal::iam(
            "111122223333",
            Some("arn:aws:sts::111122223333:assumed-role/AnyRole/session".to_string()),
        );
        assert_eq!(
            engine
                .authorize(&with_arn, actions::SCHEMA_PUBLISH, "orders")
                .await,
            AuthorizationDecision::Allow
        );
        let other_account = TrustedPrincipal::iam("999988887777", None);
        assert_eq!(
            engine
                .authorize(&other_account, actions::SCHEMA_PUBLISH, "orders")
                .await,
            AuthorizationDecision::Deny
        );
    }

    #[tokio::test]
    async fn global_roles_do_not_substitute_for_bindings() {
        let engine = InMemoryGrantEngine::new(vec![orders_publisher_binding()]);
        let admin = TrustedPrincipal::oauth(
            "a1b2c3d4-1111-2222-3333-444455556666",
            ISSUER,
            "client123",
            vec![GlobalRole::PlatformAdmin],
            vec![],
        );
        assert_eq!(
            engine
                .authorize(&admin, actions::SCHEMA_PUBLISH, "orders")
                .await,
            AuthorizationDecision::Deny
        );
    }
}
