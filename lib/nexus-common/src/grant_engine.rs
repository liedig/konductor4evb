//! The production grant engines behind [`ResourceAuthorizer`]: additive
//! role-binding evaluation over the bindings table with default denial,
//! strongly consistent decision reads, and fail-closed error mapping.
//!
//! `DynamoGrantEngine` is the cloud implementation; `InMemoryGrantEngine` is
//! the fixture-fed twin for tests and local development. A shared parity suite
//! (`tests/grant_parity.rs`) proves identical decisions on both.

use std::collections::HashSet;

use tracing::warn;

use crate::entitlement::Entitlement;
use crate::entitlement_store::{EntitlementStore, EntitlementStoreError};
use crate::grant::{BindingPrincipal, ResourceRole, RoleBinding};
use crate::grant_store::BindingStore;
use crate::principal::{AuthorizationDecision, ResourceAuthorizer, TrustedPrincipal};
use crate::subscription_resource::SubscriptionResource;

/// The action strings the engine recognizes, centralized so call-site typos
/// surface in tests rather than as silent production denials.
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

/// Direct binding check for administration paths that authorize by role and
/// domain outside an engine action (entitlement administration authorizes a
/// publisher binding on the producer domain or a subscriber binding on the
/// consumer domain). Uses the same candidate-principal mapping and strongly
/// consistent point reads as engine decisions; a store error fails closed.
pub async fn has_role_binding(
    store: &BindingStore,
    principal: &TrustedPrincipal,
    role: ResourceRole,
    domain: &str,
) -> bool {
    for candidate in candidate_principals(principal) {
        let pk = format!("PRINCIPAL#{}", candidate.key_segment());
        let sk = format!("ROLE#{}#DOMAIN#{}", role.as_str(), domain);
        match store.get_binding(&pk, &sk).await {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            Err(error) => {
                warn!(
                    domain,
                    error = %error,
                    "binding check: store error, fail closed"
                );
                return false;
            }
        }
    }
    false
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

/// The entitlement-lookup seam both engines share, so the subscribe-family
/// composition (binding check, self-subscription shortcut, coverage check,
/// fail-closed errors) exists exactly once.
trait EntitlementLookup {
    fn has_covering_entitlement(
        &self,
        consumer_domain: &str,
        producer_domain: &str,
        schema_name: &str,
    ) -> impl std::future::Future<Output = Result<bool, EntitlementStoreError>> + Send;
}

/// The additive coverage rule: any current entitlement granted to the
/// consumer whose producer matches and whose pattern matches the schema.
/// Shared by the engines and the activation-path final check.
pub fn covers(entitlements: &[Entitlement], producer_domain: &str, schema_name: &str) -> bool {
    entitlements
        .iter()
        .any(|e| e.producer_domain == producer_domain && e.scope.matches(schema_name))
}

async fn evaluate(
    lookup: &impl BindingLookup,
    entitlements: &impl EntitlementLookup,
    principal: &TrustedPrincipal,
    action: &str,
    resource: &str,
) -> AuthorizationDecision {
    match action {
        actions::SUBSCRIPTION_CREATE | actions::SUBSCRIPTION_ENABLE => {
            evaluate_subscription(lookup, entitlements, principal, action, resource, true).await
        }
        actions::SUBSCRIPTION_DISABLE | actions::SUBSCRIPTION_DELETE => {
            evaluate_subscription(lookup, entitlements, principal, action, resource, false).await
        }
        _ => evaluate_binding_action(lookup, principal, action, resource).await,
    }
}

async fn evaluate_binding_action(
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

/// Subscribe-family composition: a `subscriber` binding on the consumer
/// domain, plus — for create and enable — a covering entitlement, unless the
/// consumer subscribes to its own domain. Disable and delete consult the
/// binding only, so subscribers retain lifecycle control after revocation.
async fn evaluate_subscription(
    lookup: &impl BindingLookup,
    entitlements: &impl EntitlementLookup,
    principal: &TrustedPrincipal,
    action: &str,
    resource: &str,
    coverage_required: bool,
) -> AuthorizationDecision {
    let parsed = match SubscriptionResource::parse(resource) {
        Ok(parsed) => parsed,
        Err(error) => {
            warn!(
                action,
                resource,
                error = %error,
                "grant decision: deny (malformed subscription resource)"
            );
            return AuthorizationDecision::Deny;
        }
    };
    let bound = first_applicable_binding(
        lookup,
        principal,
        ResourceRole::Subscriber,
        action,
        parsed.consumer_domain(),
    )
    .await;
    // platform-admin operates subscriptions without a subscriber binding;
    // the covering-entitlement requirement below is unaffected.
    if bound != AuthorizationDecision::Allow && !principal.is_platform_admin() {
        return bound;
    }
    if !coverage_required || parsed.consumer_domain() == parsed.producer_domain() {
        return AuthorizationDecision::Allow;
    }
    coverage_decision(entitlements, principal, action, resource, &parsed).await
}

/// The entitlement half of a create/enable decision; every failure mode is a
/// denial (fail closed).
async fn coverage_decision(
    entitlements: &impl EntitlementLookup,
    principal: &TrustedPrincipal,
    action: &str,
    resource: &str,
    parsed: &SubscriptionResource,
) -> AuthorizationDecision {
    match entitlements
        .has_covering_entitlement(
            parsed.consumer_domain(),
            parsed.producer_domain(),
            parsed.event_name(),
        )
        .await
    {
        Ok(true) => AuthorizationDecision::Allow,
        Ok(false) => {
            warn!(
                action,
                resource,
                mechanism = ?principal.authentication_mechanism(),
                principal_id = %principal.principal_id(),
                "grant decision: deny (no covering entitlement)"
            );
            AuthorizationDecision::Deny
        }
        Err(error) => {
            warn!(
                action,
                resource,
                error = %error,
                "grant decision: deny (entitlement store error, fail closed)"
            );
            AuthorizationDecision::Deny
        }
    }
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

/// The DynamoDB-backed production engine. Binding decisions are strongly
/// consistent point reads per candidate principal, and coverage decisions a
/// strongly consistent query on the consumer partition; the administration
/// GSIs never feed an allow decision.
pub struct DynamoGrantEngine {
    store: BindingStore,
    entitlement_store: EntitlementStore,
}

impl DynamoGrantEngine {
    pub fn new(store: BindingStore, entitlement_store: EntitlementStore) -> Self {
        Self {
            store,
            entitlement_store,
        }
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

impl EntitlementLookup for DynamoGrantEngine {
    async fn has_covering_entitlement(
        &self,
        consumer_domain: &str,
        producer_domain: &str,
        schema_name: &str,
    ) -> Result<bool, EntitlementStoreError> {
        let current = self
            .entitlement_store
            .entitlements_for_consumer(consumer_domain)
            .await?;
        Ok(covers(&current, producer_domain, schema_name))
    }
}

impl ResourceAuthorizer for DynamoGrantEngine {
    async fn authorize(
        &self,
        principal: &TrustedPrincipal,
        action: &str,
        resource: &str,
    ) -> AuthorizationDecision {
        evaluate(self, self, principal, action, resource).await
    }
}

/// The fixture-fed in-memory twin for tests and local development. Decisions
/// require no AWS credentials or network access.
pub struct InMemoryGrantEngine {
    binding_keys: HashSet<(String, String)>,
    entitlements: Vec<Entitlement>,
}

impl InMemoryGrantEngine {
    /// Builds an engine from active bindings, keyed exactly as the DynamoDB
    /// store keys them so parity fixtures exercise identical key construction.
    /// The engine holds no entitlements; subscribe-family create and enable
    /// actions deny outside self-subscription.
    pub fn new(bindings: Vec<RoleBinding>) -> Self {
        Self::with_entitlements(bindings, Vec::new())
    }

    /// Builds an engine from active bindings and current entitlements.
    pub fn with_entitlements(bindings: Vec<RoleBinding>, entitlements: Vec<Entitlement>) -> Self {
        Self {
            binding_keys: bindings.iter().map(|b| (b.pk(), b.sk())).collect(),
            entitlements,
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

impl EntitlementLookup for InMemoryGrantEngine {
    async fn has_covering_entitlement(
        &self,
        consumer_domain: &str,
        producer_domain: &str,
        schema_name: &str,
    ) -> Result<bool, EntitlementStoreError> {
        let current: Vec<Entitlement> = self
            .entitlements
            .iter()
            .filter(|e| e.consumer_domain == consumer_domain)
            .cloned()
            .collect();
        Ok(covers(&current, producer_domain, schema_name))
    }
}

impl ResourceAuthorizer for InMemoryGrantEngine {
    async fn authorize(
        &self,
        principal: &TrustedPrincipal,
        action: &str,
        resource: &str,
    ) -> AuthorizationDecision {
        evaluate(self, self, principal, action, resource).await
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
    async fn owner_binding_does_not_satisfy_publisher_or_subscriber_actions() {
        let owner_binding = RoleBinding::new(
            "rb-owner",
            BindingPrincipal::cognito_group(ISSUER, "team-orders").unwrap(),
            ResourceRole::Owner,
            "orders",
            chrono::Utc::now(),
            "fixture",
        )
        .unwrap();
        let engine = InMemoryGrantEngine::new(vec![owner_binding]);
        let principal = oauth(&["team-orders"]);
        assert_eq!(
            engine
                .authorize(&principal, actions::SCHEMA_PUBLISH, "orders")
                .await,
            AuthorizationDecision::Deny,
            "owner must not publish"
        );
        assert_eq!(
            engine
                .authorize(
                    &principal,
                    actions::SUBSCRIPTION_CREATE,
                    "checkout/orders/OrderCreated"
                )
                .await,
            AuthorizationDecision::Deny,
            "owner without a subscriber binding must not subscribe"
        );
    }

    #[tokio::test]
    async fn publisher_binding_does_not_satisfy_owner_checks() {
        let engine = InMemoryGrantEngine::new(vec![orders_publisher_binding()]);
        let principal = oauth(&["orders-publishers"]);
        assert!(
            !engine
                .has_binding(
                    &BindingPrincipal::cognito_group(ISSUER, "orders-publishers").unwrap(),
                    ResourceRole::Owner,
                    "orders"
                )
                .await
                .unwrap(),
            "publisher binding must not answer an owner lookup"
        );
        drop(principal);
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
