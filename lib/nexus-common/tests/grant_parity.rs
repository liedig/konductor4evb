//! Decision-parity suite for the grant engines, seeded from the
//! authorization-engine spike's frozen fixture manifest. Every case runs
//! against `InMemoryGrantEngine` and `DynamoGrantEngine` (backed by a
//! smithy-mocks client serving the same seeds) and must produce identical
//! decisions.
//!
//! Scope: binding-level decisions plus the subscribe-family entitlement
//! composition (`add-subscriber-entitlements`). Manifest scenarios expecting
//! `deny-as-unknown` map to `Deny` at the engine level: response shaping is an
//! API concern, and deny-as-unknown was explicitly dropped. Discover and
//! asynchronous-command scenarios (S12, S13) are exercised by the catalog and
//! subscription-command handler tests respectively.

use aws_sdk_dynamodb::operation::get_item::{GetItemInput, GetItemOutput};
use aws_sdk_dynamodb::operation::query::{QueryInput, QueryOutput};
use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
use chrono::DateTime;
use serde::Deserialize;

use nexus_common::entitlement::{Entitlement, ScopePattern};
use nexus_common::entitlement_store::{entitlement_to_item, EntitlementStore};
use nexus_common::grant::{BindingPrincipal, ResourceRole, RoleBinding};
use nexus_common::grant_engine::{actions, DynamoGrantEngine, InMemoryGrantEngine};
use nexus_common::grant_store::{binding_to_item, BindingStore};
use nexus_common::principal::{
    AuthorizationDecision, GlobalRole, ResourceAuthorizer, TrustedPrincipal,
};

// Frozen copy of the authorization-engine spike's fixture manifest (archived
// at openspec/changes/archive/2026-08-05-evaluate-authorization-engine). The
// parity suite owns this copy so archived-change moves cannot break the build.
const MANIFEST_JSON: &str = include_str!("fixtures/grant-parity-manifest.json");

const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

#[derive(Deserialize)]
struct Manifest {
    #[serde(rename = "baselineGrantSeeds")]
    baseline_grant_seeds: Vec<GrantSeed>,
}

#[derive(Deserialize)]
struct GrantSeed {
    #[serde(rename = "itemType")]
    item_type: String,
    principal: Option<BindingPrincipal>,
    role: Option<ResourceRole>,
    domain: Option<String>,
    consumer: Option<String>,
    #[serde(rename = "producerDomain")]
    producer_domain: Option<String>,
    #[serde(rename = "scopePattern")]
    scope_pattern: Option<String>,
    #[serde(rename = "grantId")]
    grant_id: String,
}

fn fixed_ts() -> chrono::DateTime<chrono::Utc> {
    DateTime::parse_from_rfc3339("2026-07-28T00:00:00Z")
        .unwrap()
        .to_utc()
}

fn manifest() -> Manifest {
    serde_json::from_str(MANIFEST_JSON).expect("fixture manifest must parse")
}

fn manifest_bindings() -> Vec<RoleBinding> {
    manifest()
        .baseline_grant_seeds
        .iter()
        .filter(|seed| seed.item_type == "RoleBinding")
        .map(|seed| {
            RoleBinding::new(
                seed.grant_id.clone(),
                seed.principal.clone().expect("RoleBinding seed principal"),
                seed.role.expect("RoleBinding seed role"),
                seed.domain.clone().expect("RoleBinding seed domain"),
                fixed_ts(),
                "fixture-manifest",
            )
            .expect("fixture seed must be a valid binding")
        })
        .collect()
}

fn manifest_entitlements() -> Vec<Entitlement> {
    manifest()
        .baseline_grant_seeds
        .iter()
        .filter(|seed| seed.item_type == "Entitlement")
        .map(|seed| {
            Entitlement::new(
                seed.consumer.clone().expect("Entitlement seed consumer"),
                seed.producer_domain
                    .clone()
                    .expect("Entitlement seed producerDomain"),
                ScopePattern::new(
                    seed.scope_pattern
                        .clone()
                        .expect("Entitlement seed scopePattern"),
                )
                .expect("fixture seed pattern must be valid"),
                fixed_ts(),
                "fixture-manifest",
            )
            .expect("fixture seed must be a valid entitlement")
        })
        .collect()
}

fn test_entitlement(consumer: &str, producer: &str, pattern: &str) -> Entitlement {
    Entitlement::new(
        consumer,
        producer,
        ScopePattern::new(pattern).unwrap(),
        fixed_ts(),
        "fixture",
    )
    .unwrap()
}

/// A DynamoDB engine whose reads are served from the same seeds. Rules match
/// only strongly consistent reads, so a regression to eventually consistent
/// decision reads fails every allow case here.
fn dynamo_engine(bindings: &[RoleBinding], entitlements: &[Entitlement]) -> DynamoGrantEngine {
    let mut rules: Vec<Rule> = bindings
        .iter()
        .map(|b| {
            let item = binding_to_item(b);
            let pk = b.pk();
            let sk = b.sk();
            mock!(aws_sdk_dynamodb::Client::get_item)
                .match_requests(move |req: &GetItemInput| {
                    let key = req.key();
                    let has_key = |name: &str, want: &String| {
                        key.and_then(|k| k.get(name)).and_then(|v| v.as_s().ok()) == Some(want)
                    };
                    req.consistent_read() == Some(true) && has_key("PK", &pk) && has_key("SK", &sk)
                })
                .then_output(move || {
                    GetItemOutput::builder()
                        .set_item(Some(item.clone()))
                        .build()
                })
        })
        .collect();

    let mut consumers: Vec<&str> = entitlements
        .iter()
        .map(|e| e.consumer_domain.as_str())
        .collect();
    consumers.sort_unstable();
    consumers.dedup();
    for consumer in consumers {
        let items: Vec<_> = entitlements
            .iter()
            .filter(|e| e.consumer_domain == consumer)
            .map(entitlement_to_item)
            .collect();
        let expected_pk = format!("CONSUMER#{consumer}");
        rules.push(
            mock!(aws_sdk_dynamodb::Client::query)
                .match_requests(move |req: &QueryInput| {
                    req.consistent_read() == Some(true)
                        && req
                            .expression_attribute_values()
                            .and_then(|v| v.get(":pk"))
                            .and_then(|v| v.as_s().ok())
                            == Some(&expected_pk)
                })
                .then_output(move || {
                    QueryOutput::builder()
                        .set_items(Some(items.clone()))
                        .build()
                }),
        );
    }

    rules.push(
        mock!(aws_sdk_dynamodb::Client::query).then_output(|| QueryOutput::builder().build()),
    );
    rules.push(
        mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| GetItemOutput::builder().build()),
    );
    let client = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, &rules);
    DynamoGrantEngine::new(
        BindingStore::new(client.clone(), "fixture-table"),
        EntitlementStore::new(client, "fixture-table"),
    )
}

/// The decision contract the fixtures assume: global platform roles resolve
/// from authorizer-supplied context (never the grant table), everything else
/// delegates to the engine.
async fn decide(
    engine: &impl ResourceAuthorizer,
    principal: &TrustedPrincipal,
    action: &str,
    resource: &str,
) -> AuthorizationDecision {
    let is_admin = principal.is_platform_admin();
    let is_auditor = principal
        .global_roles()
        .contains(&GlobalRole::PlatformAuditor);
    match action {
        "platform:admin" if is_admin => AuthorizationDecision::Allow,
        "platform:read" if is_admin || is_auditor => AuthorizationDecision::Allow,
        "platform:admin" | "platform:read" => AuthorizationDecision::Deny,
        _ => engine.authorize(principal, action, resource).await,
    }
}

async fn assert_parity(
    bindings: &[RoleBinding],
    entitlements: &[Entitlement],
    principal: &TrustedPrincipal,
    action: &str,
    resource: &str,
    expected: AuthorizationDecision,
) {
    let memory = InMemoryGrantEngine::with_entitlements(bindings.to_vec(), entitlements.to_vec());
    let dynamo = dynamo_engine(bindings, entitlements);
    let via_memory = decide(&memory, principal, action, resource).await;
    let via_dynamo = decide(&dynamo, principal, action, resource).await;
    assert_eq!(
        via_memory, via_dynamo,
        "engines diverged on {action} {resource}"
    );
    assert_eq!(
        via_memory, expected,
        "unexpected decision for {action} {resource}"
    );
}

fn oauth(groups: &[&str], global_roles: Vec<GlobalRole>) -> TrustedPrincipal {
    TrustedPrincipal::oauth(
        "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        ISSUER,
        "fixture-client",
        global_roles,
        groups.iter().map(|g| g.to_string()).collect(),
    )
}

#[tokio::test]
async fn s01_platform_admin_allowed_by_global_role_composition() {
    let bindings = manifest_bindings();
    let admin = oauth(&["nexus-platform-admins"], vec![GlobalRole::PlatformAdmin]);
    assert_parity(
        &bindings,
        &[],
        &admin,
        "platform:admin",
        "marketing-prod",
        AuthorizationDecision::Allow,
    )
    .await;
}

#[tokio::test]
async fn s02_platform_auditor_reads_but_cannot_administer() {
    let bindings = manifest_bindings();
    let auditor = oauth(
        &["nexus-platform-auditors"],
        vec![GlobalRole::PlatformAuditor],
    );
    assert_parity(
        &bindings,
        &[],
        &auditor,
        "platform:read",
        "payments-prod",
        AuthorizationDecision::Allow,
    )
    .await;
    assert_parity(
        &bindings,
        &[],
        &auditor,
        "platform:admin",
        "payments-prod",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn s03_cognito_group_binding_allows_publish() {
    let bindings = manifest_bindings();
    let principal = oauth(&["payments-publishers"], vec![]);
    assert_parity(
        &bindings,
        &[],
        &principal,
        "schema:publish",
        "payments-prod",
        AuthorizationDecision::Allow,
    )
    .await;
}

#[tokio::test]
async fn s04_binding_does_not_cross_domain_boundaries() {
    let bindings = manifest_bindings();
    let principal = oauth(&["payments-publishers"], vec![]);
    assert_parity(
        &bindings,
        &[],
        &principal,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn s05_subscriber_binding_bound_and_unbound() {
    let bindings = manifest_bindings();
    let subscriber = oauth(&["analytics-subscribers"], vec![]);
    assert_parity(
        &bindings,
        &[],
        &subscriber,
        actions::SUBSCRIPTION_DISABLE,
        "analytics-cons/payments-prod/payment-captured",
        AuthorizationDecision::Allow,
    )
    .await;
    assert_parity(
        &bindings,
        &[],
        &subscriber,
        actions::SUBSCRIPTION_DELETE,
        "risk-cons/payments-prod/payment-captured",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn s06_domain_wide_entitlement_covers_producer_domain() {
    let bindings = manifest_bindings();
    let entitlements = manifest_entitlements();
    let subscriber = oauth(&["analytics-subscribers"], vec![]);
    for schema in ["payment-captured", "payment-refunded"] {
        assert_parity(
            &bindings,
            &entitlements,
            &subscriber,
            actions::SUBSCRIPTION_CREATE,
            &format!("analytics-cons/payments-prod/{schema}"),
            AuthorizationDecision::Allow,
        )
        .await;
    }
    assert_parity(
        &bindings,
        &entitlements,
        &subscriber,
        actions::SUBSCRIPTION_CREATE,
        "analytics-cons/orders-prod/order-placed",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn s07_exact_entitlement_does_not_expand() {
    let bindings = manifest_bindings();
    let entitlements = manifest_entitlements();
    let subscriber = oauth(&["risk-subscribers"], vec![]);
    assert_parity(
        &bindings,
        &entitlements,
        &subscriber,
        actions::SUBSCRIPTION_CREATE,
        "risk-cons/payments-prod/payment-captured",
        AuthorizationDecision::Allow,
    )
    .await;
    assert_parity(
        &bindings,
        &entitlements,
        &subscriber,
        actions::SUBSCRIPTION_CREATE,
        "risk-cons/payments-prod/payment-refunded",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn s08_producer_growth_after_grant() {
    let bindings = manifest_bindings();
    let entitlements = manifest_entitlements();
    assert_parity(
        &bindings,
        &entitlements,
        &oauth(&["analytics-subscribers"], vec![]),
        actions::SUBSCRIPTION_CREATE,
        "analytics-cons/payments-prod/payment-disputed",
        AuthorizationDecision::Allow,
    )
    .await;
    assert_parity(
        &bindings,
        &entitlements,
        &oauth(&["risk-subscribers"], vec![]),
        actions::SUBSCRIPTION_CREATE,
        "risk-cons/payments-prod/payment-disputed",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn s09_overlapping_grants_union_and_removal() {
    let bindings = manifest_bindings();
    let all = manifest_entitlements();
    let subscriber = oauth(&["analytics-subscribers"], vec![]);
    let resource = "analytics-cons/payments-prod/payment-captured";

    assert_parity(
        &bindings,
        &all,
        &subscriber,
        actions::SUBSCRIPTION_CREATE,
        resource,
        AuthorizationDecision::Allow,
    )
    .await;

    let without_exact: Vec<Entitlement> = all
        .iter()
        .filter(|e| {
            !(e.consumer_domain == "analytics-cons" && e.scope.as_str() == "payment-captured")
        })
        .cloned()
        .collect();
    assert_parity(
        &bindings,
        &without_exact,
        &subscriber,
        actions::SUBSCRIPTION_CREATE,
        resource,
        AuthorizationDecision::Allow,
    )
    .await;

    let none: Vec<Entitlement> = vec![];
    assert_parity(
        &bindings,
        &none,
        &subscriber,
        actions::SUBSCRIPTION_CREATE,
        resource,
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn s10_revocation_changes_decisions_immediately() {
    let bindings = manifest_bindings();
    let entitlements = manifest_entitlements();

    let risk_without_exact: Vec<Entitlement> = entitlements
        .iter()
        .filter(|e| e.consumer_domain != "risk-cons")
        .cloned()
        .collect();
    assert_parity(
        &bindings,
        &risk_without_exact,
        &oauth(&["risk-subscribers"], vec![]),
        actions::SUBSCRIPTION_CREATE,
        "risk-cons/payments-prod/payment-captured",
        AuthorizationDecision::Deny,
    )
    .await;

    let analytics_without_exact: Vec<Entitlement> = entitlements
        .iter()
        .filter(|e| {
            !(e.consumer_domain == "analytics-cons" && e.scope.as_str() == "payment-captured")
        })
        .cloned()
        .collect();
    assert_parity(
        &bindings,
        &analytics_without_exact,
        &oauth(&["analytics-subscribers"], vec![]),
        actions::SUBSCRIPTION_CREATE,
        "analytics-cons/payments-prod/payment-captured",
        AuthorizationDecision::Allow,
    )
    .await;
}

#[tokio::test]
async fn prefix_entitlement_covers_family_only() {
    let bindings = manifest_bindings();
    let prefix = test_entitlement("risk-cons", "payments-prod", "payment-re*");
    let subscriber = oauth(&["risk-subscribers"], vec![]);
    assert_parity(
        &bindings,
        std::slice::from_ref(&prefix),
        &subscriber,
        actions::SUBSCRIPTION_CREATE,
        "risk-cons/payments-prod/payment-refunded",
        AuthorizationDecision::Allow,
    )
    .await;
    assert_parity(
        &bindings,
        &[prefix],
        &subscriber,
        actions::SUBSCRIPTION_CREATE,
        "risk-cons/payments-prod/payment-captured",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn self_subscription_needs_binding_but_no_entitlement() {
    let mut bindings = manifest_bindings();
    bindings.push(
        RoleBinding::new(
            "rb-self-001",
            BindingPrincipal::cognito_group(ISSUER, "payments-subscribers").unwrap(),
            ResourceRole::Subscriber,
            "payments-prod",
            fixed_ts(),
            "fixture",
        )
        .unwrap(),
    );
    let self_subscriber = oauth(&["payments-subscribers"], vec![]);
    assert_parity(
        &bindings,
        &[],
        &self_subscriber,
        actions::SUBSCRIPTION_CREATE,
        "payments-prod/payments-prod/payment-captured",
        AuthorizationDecision::Allow,
    )
    .await;

    let outsider = oauth(&["analytics-subscribers"], vec![]);
    assert_parity(
        &bindings,
        &[],
        &outsider,
        actions::SUBSCRIPTION_CREATE,
        "payments-prod/payments-prod/payment-captured",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn iam_account_subscribe_parity_with_cognito_group() {
    let bindings = manifest_bindings();
    let entitlements = manifest_entitlements();
    let resource = "risk-cons/payments-prod/payment-captured";

    let account = TrustedPrincipal::iam("444455556666", None);
    assert_parity(
        &bindings,
        &entitlements,
        &account,
        actions::SUBSCRIPTION_CREATE,
        resource,
        AuthorizationDecision::Allow,
    )
    .await;

    let session = TrustedPrincipal::iam(
        "444455556666",
        Some("arn:aws:sts::444455556666:assumed-role/nexus-subscriber/spike-session".to_string()),
    );
    assert_parity(
        &bindings,
        &entitlements,
        &session,
        actions::SUBSCRIPTION_CREATE,
        resource,
        AuthorizationDecision::Allow,
    )
    .await;

    let other_account = TrustedPrincipal::iam("999988887777", None);
    assert_parity(
        &bindings,
        &entitlements,
        &other_account,
        actions::SUBSCRIPTION_CREATE,
        resource,
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn enable_requires_coverage_but_disable_and_delete_are_binding_only() {
    let bindings = manifest_bindings();
    let subscriber = oauth(&["risk-subscribers"], vec![]);
    let resource = "risk-cons/payments-prod/payment-captured";

    assert_parity(
        &bindings,
        &[],
        &subscriber,
        actions::SUBSCRIPTION_ENABLE,
        resource,
        AuthorizationDecision::Deny,
    )
    .await;
    assert_parity(
        &bindings,
        &[],
        &subscriber,
        actions::SUBSCRIPTION_DISABLE,
        resource,
        AuthorizationDecision::Allow,
    )
    .await;
    assert_parity(
        &bindings,
        &[],
        &subscriber,
        actions::SUBSCRIPTION_DELETE,
        resource,
        AuthorizationDecision::Allow,
    )
    .await;
}

#[tokio::test]
async fn malformed_composite_resources_fail_closed() {
    let bindings = manifest_bindings();
    let entitlements = manifest_entitlements();
    let subscriber = oauth(&["analytics-subscribers"], vec![]);
    for resource in [
        "analytics-cons/payments-prod",
        "analytics-cons/payments-prod/payment-captured/extra",
        "analytics-cons//payment-captured",
        "",
    ] {
        assert_parity(
            &bindings,
            &entitlements,
            &subscriber,
            actions::SUBSCRIPTION_CREATE,
            resource,
            AuthorizationDecision::Deny,
        )
        .await;
    }
}

#[tokio::test]
async fn s11_cognito_group_and_aws_account_parity() {
    let bindings = manifest_bindings();
    let group_principal = oauth(&["orders-publishers"], vec![]);
    assert_parity(
        &bindings,
        &[],
        &group_principal,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Allow,
    )
    .await;

    let account_principal = TrustedPrincipal::iam("111122223333", None);
    assert_parity(
        &bindings,
        &[],
        &account_principal,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Allow,
    )
    .await;

    let session_principal = TrustedPrincipal::iam(
        "111122223333",
        Some("arn:aws:sts::111122223333:assumed-role/nexus-publisher/spike-session".to_string()),
    );
    assert_parity(
        &bindings,
        &[],
        &session_principal,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Allow,
    )
    .await;
}

#[tokio::test]
async fn s11d_binding_is_isolated_by_issuer() {
    let bindings = manifest_bindings();
    let forged = TrustedPrincipal::oauth(
        "forged-subject-id",
        "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_UnTrUsTeD",
        "fixture-client",
        vec![],
        vec!["orders-publishers".to_string()],
    );
    assert_parity(
        &bindings,
        &[],
        &forged,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn additive_bindings_form_a_union() {
    let bindings = manifest_bindings();
    let both_groups = oauth(&["payments-publishers", "orders-publishers"], vec![]);
    assert_parity(
        &bindings,
        &[],
        &both_groups,
        "schema:publish",
        "payments-prod",
        AuthorizationDecision::Allow,
    )
    .await;
    assert_parity(
        &bindings,
        &[],
        &both_groups,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Allow,
    )
    .await;

    let without_account_binding: Vec<RoleBinding> = bindings
        .iter()
        .filter(|b| b.binding_id != "rb-ordpub-002")
        .cloned()
        .collect();
    let account_principal = TrustedPrincipal::iam("111122223333", None);
    assert_parity(
        &without_account_binding,
        &[],
        &account_principal,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Deny,
    )
    .await;
    assert_parity(
        &without_account_binding,
        &[],
        &group_principal_orders(),
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Allow,
    )
    .await;
}

fn group_principal_orders() -> TrustedPrincipal {
    oauth(&["orders-publishers"], vec![])
}

#[tokio::test]
async fn subscriber_binding_does_not_grant_publisher_actions() {
    let bindings = manifest_bindings();
    let subscriber = oauth(&["analytics-subscribers"], vec![]);
    assert_parity(
        &bindings,
        &[],
        &subscriber,
        "schema:publish",
        "analytics-cons",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn default_denial_for_unknown_principal_action_and_resource() {
    let bindings = manifest_bindings();
    let nobody = oauth(&["team-nobody"], vec![]);
    assert_parity(
        &bindings,
        &[],
        &nobody,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Deny,
    )
    .await;

    let publisher = oauth(&["orders-publishers"], vec![]);
    assert_parity(
        &bindings,
        &[],
        &publisher,
        "schema:delete",
        "orders-prod",
        AuthorizationDecision::Deny,
    )
    .await;
    assert_parity(
        &bindings,
        &[],
        &publisher,
        "schema:publish",
        "orders-prod/payment-captured",
        AuthorizationDecision::Deny,
    )
    .await;
    assert_parity(
        &bindings,
        &[],
        &publisher,
        "schema:publish",
        "",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn empty_team_groups_deny() {
    let bindings = manifest_bindings();
    let group_less = oauth(&[], vec![]);
    assert_parity(
        &bindings,
        &[],
        &group_less,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Deny,
    )
    .await;
}

/// Dynamo-only: the in-memory engine has no failure mode, so fail-closed
/// error mapping is proven against a failing store directly.
#[tokio::test]
async fn dynamo_store_error_fails_closed() {
    let rule = mock!(aws_sdk_dynamodb::Client::get_item).then_error(|| {
        aws_sdk_dynamodb::operation::get_item::GetItemError::generic(
            aws_sdk_dynamodb::error::ErrorMetadata::builder()
                .message("simulated store outage")
                .build(),
        )
    });
    let client = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&rule]);
    let engine = DynamoGrantEngine::new(
        BindingStore::new(client.clone(), "fixture-table"),
        EntitlementStore::new(client, "fixture-table"),
    );
    let principal = oauth(&["orders-publishers"], vec![]);
    assert_eq!(
        engine
            .authorize(&principal, "schema:publish", "orders-prod")
            .await,
        AuthorizationDecision::Deny,
        "a store error must deny, never allow"
    );
}
