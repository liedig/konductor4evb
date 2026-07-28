use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrincipalType {
    CognitoGroup,
    AwsAccount,
    ConsumerDomain,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationSubject {
    #[serde(rename = "type")]
    pub kind: PrincipalType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub consumer_domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub via: Option<Box<AuthorizationSubject>>,
}

impl AuthorizationSubject {
    pub fn cognito_group(issuer: &str, group: &str) -> Self {
        Self {
            kind: PrincipalType::CognitoGroup,
            issuer: Some(issuer.to_string()),
            group: Some(group.to_string()),
            account_id: None,
            consumer_domain: None,
            via: None,
        }
    }

    pub fn aws_account(account_id: &str) -> Self {
        Self {
            kind: PrincipalType::AwsAccount,
            issuer: None,
            group: None,
            account_id: Some(account_id.to_string()),
            consumer_domain: None,
            via: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditProvenance {
    #[serde(rename = "type")]
    pub kind: AuditProvenanceKind,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub arn: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditProvenanceKind {
    CognitoSubject,
    IamPrincipalArn,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    #[serde(rename = "platform:admin")]
    PlatformAdmin,
    #[serde(rename = "platform:read")]
    PlatformRead,
    #[serde(rename = "domain:register")]
    DomainRegister,
    #[serde(rename = "schema:publish")]
    SchemaPublish,
    #[serde(rename = "schema:subscribe")]
    SchemaSubscribe,
    #[serde(rename = "schema:discover")]
    SchemaDiscover,
    #[serde(rename = "subscription:admin")]
    SubscriptionAdmin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Resource {
    Domain {
        id: String,
    },
    Schema {
        #[serde(rename = "producerDomain")]
        producer_domain: String,
        schema: String,
    },
    Subscription {
        #[serde(rename = "consumerDomain")]
        consumer_domain: String,
        #[serde(rename = "producerDomain")]
        producer_domain: String,
        schema: String,
    },
    SchemaList {
        #[serde(rename = "producerDomain")]
        producer_domain: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthContext {
    #[serde(rename = "authMethod")]
    pub auth_method: AuthMethod,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub global_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub acting_as: Option<String>,
    #[serde(rename = "commandId")]
    pub command_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub queue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub replay_of: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub missing_field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tampered: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub trusted_issuer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Oauth,
    Iam,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Decision {
    Allow,
    Deny { reason: DenyReason },
    DenyAsUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DenyReason {
    AuditorRoleNotAuthorizedForMutation,
    PublisherNotBoundToDomain,
    SubscriberNotBoundToConsumerDomain,
    NoApplicableEntitlement,
    UntrustedIssuer,
    DuplicateCommand,
    IncompletePrincipalContext,
    TamperedPrincipalContext,
    #[serde(other)]
    Unknown,
}
