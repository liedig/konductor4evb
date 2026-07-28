use serde::Deserialize;
use serde_json::Value;

use crate::contract::{
    Action, AuditProvenance, AuthContext, AuthorizationSubject, DenyReason, Resource,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub manifest_version: String,
    pub change: String,
    pub description: String,
    pub frozen_at: String,
    pub hash_record: String,
    pub action_set: Vec<String>,
    pub principal_types: Vec<String>,
    pub topology: Topology,
    pub subject_bindings: SubjectBindings,
    pub baseline_grant_seeds: Vec<GrantSeed>,
    pub baseline_subscription_seeds: Vec<SubscriptionSeed>,
    pub scenarios: Vec<Scenario>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topology {
    pub cognito_issuer: String,
    pub producer_domains: Vec<ProducerDomain>,
    pub consumer_domains: Vec<String>,
    #[serde(default)]
    pub note_on_producer_growth: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProducerDomain {
    pub id: String,
    pub schemas: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectBindings {
    #[serde(default)]
    pub cognito_groups: Vec<CognitoGroupBinding>,
    #[serde(default)]
    pub aws_accounts: Vec<AwsAccountBinding>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitoGroupBinding {
    pub group: String,
    pub issuer: String,
    #[serde(default)]
    pub global_role: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsAccountBinding {
    pub account_id: String,
    pub role: String,
    pub domain: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantSeed {
    pub item_type: String,
    #[serde(default)]
    pub principal: Option<SeedPrincipal>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub consumer: Option<String>,
    #[serde(default)]
    pub producer_domain: Option<String>,
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    pub state: String,
    pub grant_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedPrincipal {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionSeed {
    pub subscription_id: String,
    pub consumer: String,
    pub producer_domain: String,
    pub schema: String,
    pub state: String,
    pub covering_grants: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub id: String,
    pub scenario: String,
    pub spec_reference: String,
    #[serde(default)]
    pub precondition: Option<String>,
    #[serde(default)]
    pub authorization_subject: Option<AuthorizationSubject>,
    #[serde(default)]
    pub audit_provenance: Option<AuditProvenance>,
    #[serde(default)]
    pub action: Option<Action>,
    #[serde(default)]
    pub resource: Option<Resource>,
    #[serde(default)]
    pub context: Option<AuthContext>,
    #[serde(default)]
    pub seed_grants: Vec<String>,
    #[serde(default)]
    pub seed_subscriptions: Vec<String>,
    #[serde(default)]
    pub expected: Option<Value>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub authorization_subject: Option<AuthorizationSubject>,
    #[serde(default)]
    pub audit_provenance: Option<AuditProvenance>,
    #[serde(default)]
    pub action: Option<Action>,
    #[serde(default)]
    pub resource: Option<Resource>,
    #[serde(default)]
    pub context: Option<AuthContext>,
    #[serde(default)]
    pub expected: Option<Value>,
    #[serde(default)]
    pub deny_reason: Option<DenyReason>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub applicable_grants: Vec<String>,
    #[serde(default)]
    pub revoke: Option<String>,
    #[serde(default)]
    pub expected_decision_after_revocation: Option<Value>,
    #[serde(default)]
    pub expected_subscription_state: Option<ExpectedSubscriptionState>,
    #[serde(default)]
    pub measure_separately: Vec<String>,
    #[serde(default)]
    pub enqueue: Option<Enqueue>,
    #[serde(default)]
    pub before_consume: Option<BeforeConsume>,
    #[serde(default)]
    pub expected_handler_decision: Option<Value>,
    #[serde(default)]
    pub outcome: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedSubscriptionState {
    pub subscription_id: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Enqueue {
    pub authorization_subject: AuthorizationSubject,
    #[serde(default)]
    pub audit_provenance: Option<AuditProvenance>,
    pub action: Action,
    pub resource: Resource,
    pub context: AuthContext,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeConsume {
    pub revoke: String,
}
