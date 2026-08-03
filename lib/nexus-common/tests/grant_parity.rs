//! Decision-parity suite for the grant engines, seeded from the
//! authorization-engine spike's frozen fixture manifest. Every case runs
//! against `InMemoryGrantEngine` and `DynamoGrantEngine` (backed by a
//! smithy-mocks client serving the same seeds) and must produce identical
//! decisions.
//!
//! Scope: binding-level decisions (this change). Entitlement, revocation, and
//! catalog scenarios from the manifest land with `add-subscriber-entitlements`,
//! which extends this suite. Manifest scenarios expecting `deny-as-unknown`
//! map to `Deny` at the engine level: response shaping is an API concern, and
//! deny-as-unknown was explicitly dropped.

use aws_sdk_dynamodb::operation::get_item::GetItemInput;
use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
use chrono::DateTime;
use serde::Deserialize;

use nexus_common::grant::{BindingPrincipal, ResourceRole, RoleBinding};
use nexus_common::grant_engine::{DynamoGrantEngine, InMemoryGrantEngine};
use nexus_common::grant_store::{binding_to_item, BindingStore};
use nexus_common::principal::{
    AuthorizationDecision, GlobalRole, ResourceAuthorizer, TrustedPrincipal,
};

const MANIFEST_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../openspec/changes/evaluate-authorization-engine/evidence/fixture-manifest.json"
));

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
    #[serde(rename = "grantId")]
    grant_id: String,
}

fn manifest_bindings() -> Vec<RoleBinding> {
    let manifest: Manifest =
        serde_json::from_str(MANIFEST_JSON).expect("fixture manifest must parse");
    let fixed_ts = DateTime::parse_from_rfc3339("2026-07-28T00:00:00Z")
        .unwrap()
        .to_utc();
    manifest
        .baseline_grant_seeds
        .iter()
        .filter(|seed| seed.item_type == "RoleBinding")
        .map(|seed| {
            RoleBinding::new(
                seed.grant_id.clone(),
                seed.principal.clone().expect("RoleBinding seed principal"),
                seed.role.expect("RoleBinding seed role"),
                seed.domain.clone().expect("RoleBinding seed domain"),
                fixed_ts,
                "fixture-manifest",
            )
            .expect("fixture seed must be a valid binding")
        })
        .collect()
}

/// A DynamoDB engine whose get_item calls are served from the same seeds.
/// Rules match only strongly consistent reads, so a regression to eventually
/// consistent decision reads fails every allow case here.
fn dynamo_engine(bindings: &[RoleBinding]) -> DynamoGrantEngine {
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
                        key.and_then(|k| k.get(name))
                            .and_then(|v| v.as_s().ok())
                            == Some(want)
                    };
                    req.consistent_read() == Some(true) && has_key("PK", &pk) && has_key("SK", &sk)
                })
                .then_output(move || {
                    aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                        .set_item(Some(item.clone()))
                        .build()
                })
        })
        .collect();
    rules.push(
        mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build()),
    );
    let client = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, &rules);
    DynamoGrantEngine::new(BindingStore::new(client, "fixture-table"))
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
    principal: &TrustedPrincipal,
    action: &str,
    resource: &str,
    expected: AuthorizationDecision,
) {
    let memory = InMemoryGrantEngine::new(bindings.to_vec());
    let dynamo = dynamo_engine(bindings);
    let via_memory = decide(&memory, principal, action, resource).await;
    let via_dynamo = decide(&dynamo, principal, action, resource).await;
    assert_eq!(
        via_memory, via_dynamo,
        "engines diverged on {action} {resource}"
    );
    assert_eq!(via_memory, expected, "unexpected decision for {action} {resource}");
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
    let auditor = oauth(&["nexus-platform-auditors"], vec![GlobalRole::PlatformAuditor]);
    assert_parity(
        &bindings,
        &auditor,
        "platform:read",
        "payments-prod",
        AuthorizationDecision::Allow,
    )
    .await;
    assert_parity(
        &bindings,
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
        &principal,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Deny,
    )
    .await;
}

#[tokio::test]
async fn s11_cognito_group_and_aws_account_parity() {
    let bindings = manifest_bindings();
    let group_principal = oauth(&["orders-publishers"], vec![]);
    assert_parity(
        &bindings,
        &group_principal,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Allow,
    )
    .await;

    let account_principal = TrustedPrincipal::iam("111122223333", None);
    assert_parity(
        &bindings,
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
        &both_groups,
        "schema:publish",
        "payments-prod",
        AuthorizationDecision::Allow,
    )
    .await;
    assert_parity(
        &bindings,
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
        &account_principal,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Deny,
    )
    .await;
    assert_parity(
        &without_account_binding,
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
        &nobody,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Deny,
    )
    .await;

    let publisher = oauth(&["orders-publishers"], vec![]);
    assert_parity(
        &bindings,
        &publisher,
        "schema:delete",
        "orders-prod",
        AuthorizationDecision::Deny,
    )
    .await;
    assert_parity(
        &bindings,
        &publisher,
        "schema:publish",
        "orders-prod/payment-captured",
        AuthorizationDecision::Deny,
    )
    .await;
    assert_parity(
        &bindings,
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
        &group_less,
        "schema:publish",
        "orders-prod",
        AuthorizationDecision::Deny,
    )
    .await;
}
