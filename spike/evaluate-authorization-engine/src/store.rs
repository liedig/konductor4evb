use crate::contract::{AuthorizationSubject, PrincipalType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantState {
    Active,
    Revoked,
}

#[derive(Debug, Clone)]
pub struct BindingItem {
    pub grant_id: String,
    pub role: String,
    pub domain: String,
    pub state: GrantState,
}

#[derive(Debug, Clone)]
pub struct EntitlementItem {
    pub grant_id: String,
    pub consumer: String,
    pub producer_domain: String,
    pub schema: Option<String>,
    pub scope: String,
    pub state: GrantState,
}

#[derive(Debug, Clone)]
pub struct SubscriptionItem {
    pub subscription_id: String,
    pub consumer: String,
    pub producer_domain: String,
    pub schema: String,
    pub state: String,
}

#[derive(Debug, Clone)]
pub struct AuditRecord {
    pub grant_id: String,
    pub action: String,
    pub timestamp_ms: i64,
}

/// Build the grant-table partition key for a principal. Consumer-domain subjects
/// act via their `via` principal (the bound Cognito group or AWS account).
/// IAM principal ARNs are never part of this key (audit provenance only).
pub fn principal_key(subject: &AuthorizationSubject) -> String {
    match subject.kind {
        PrincipalType::CognitoGroup => {
            let issuer = subject.issuer.as_deref().unwrap_or_default();
            let group = subject.group.as_deref().unwrap_or_default();
            format!("PRINCIPAL#cognito-group#{issuer}#{group}")
        }
        PrincipalType::AwsAccount => {
            let account = subject.account_id.as_deref().unwrap_or_default();
            format!("PRINCIPAL#aws-account#{account}")
        }
        PrincipalType::ConsumerDomain => match subject.via.as_ref() {
            Some(via) => principal_key(via),
            None => format!(
                "PRINCIPAL#consumer-domain#{}",
                subject.consumer_domain.as_deref().unwrap_or_default()
            ),
        },
    }
}

/// Build the grant-table sort key for a role binding: ROLE#{role}#DOMAIN#{domain}.
pub fn binding_sort_key(role: &str, domain: &str) -> String {
    format!("ROLE#{role}#DOMAIN#{domain}")
}

/// Build the grant-table sort key for an entitlement: ENT#PRODUCER#{producer}[#SCHEMA#{schema}].
pub fn entitlement_sort_key(producer_domain: &str, schema: Option<&str>) -> String {
    match schema {
        Some(schema) => format!("ENT#PRODUCER#{producer_domain}#SCHEMA#{schema}"),
        None => format!("ENT#PRODUCER#{producer_domain}"),
    }
}

/// Build the subscription-store GSI key for revocation candidate lookup:
/// CONSUMER#{consumer}#PRODUCER#{producer}.
pub fn subscription_scope_key(consumer: &str, producer: &str) -> String {
    format!("CONSUMER#{consumer}#PRODUCER#{producer}")
}
