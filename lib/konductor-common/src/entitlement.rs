//! Typed entitlement primitives for the consumer-coverage store: the scope
//! pattern grammar, the entitlement item shape with deterministic
//! identifiers, the token-free audit record, and the DynamoDB key
//! construction shared by the store, the engines, and the administration API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::grant::AuditMutation;

/// The fixed UUID v5 namespace from which entitlement identifiers derive.
/// Deterministic derivation from the (consumer, producer, pattern) triple is
/// what makes the conditional not-exists create idempotent under retries.
const ENTITLEMENT_NAMESPACE: Uuid = Uuid::from_u128(0x6e65_7875_7365_6e74_6974_6c65_6d65_6e74);

/// A failure to construct a valid entitlement-model value.
#[derive(Debug, thiserror::Error)]
pub enum EntitlementModelError {
    #[error(
        "scope pattern must be a schema-name prefix with an optional single trailing '*': {0:?}"
    )]
    InvalidScopePattern(String),
    #[error("consumer domain must be non-empty")]
    EmptyConsumerDomain,
    #[error("producer domain must be non-empty")]
    EmptyProducerDomain,
    #[error("key segment must not contain '#': {0:?}")]
    UnsafeKeySegment(String),
    #[error("trusted actor must be non-empty")]
    EmptyActor,
}

fn is_schema_name_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')
}

fn reject_unsafe_segment(segment: &str) -> Result<(), EntitlementModelError> {
    if segment.contains('#') {
        return Err(EntitlementModelError::UnsafeKeySegment(segment.to_string()));
    }
    Ok(())
}

/// A validated coverage pattern over a producer domain's schema names.
///
/// The grammar is deliberately minimal: EventBridge schema-name characters
/// with one optional trailing wildcard. `*` alone covers the whole producer
/// domain including schemas created later; an exact name covers all versions
/// of one schema; a prefix with a trailing `*` covers the matching family
/// including future members. `*` is not a legal schema-name character, so a
/// pattern can never collide with a literal name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopePattern(String);

impl ScopePattern {
    /// Builds a scope pattern, rejecting anything outside the grammar.
    pub fn new(value: impl Into<String>) -> Result<Self, EntitlementModelError> {
        let value = value.into();
        let prefix = value.strip_suffix('*').unwrap_or(&value);
        let valid =
            !value.is_empty() && !prefix.contains('*') && prefix.bytes().all(is_schema_name_char);
        if valid {
            Ok(Self(value))
        } else {
            Err(EntitlementModelError::InvalidScopePattern(value))
        }
    }

    /// The pattern as stored and serialized.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `true` when the pattern is exactly `*` (covers the producer domain).
    pub fn is_domain_wide(&self) -> bool {
        self.0 == "*"
    }

    /// Whether this pattern covers `schema_name`: literal equality for exact
    /// patterns, prefix matching for wildcard patterns (`*` matches all).
    pub fn matches(&self, schema_name: &str) -> bool {
        match self.0.strip_suffix('*') {
            Some(prefix) => schema_name.starts_with(prefix),
            None => self.0 == schema_name,
        }
    }
}

impl<'de> Deserialize<'de> for ScopePattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// A coverage grant from a producer domain to a consumer domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entitlement {
    pub entitlement_id: String,
    pub consumer_domain: String,
    pub producer_domain: String,
    #[serde(rename = "scopePattern")]
    pub scope: ScopePattern,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
}

impl Entitlement {
    /// Builds an entitlement, deriving the stable identifier from the
    /// (consumer, producer, pattern) triple and rejecting malformed segments.
    pub fn new(
        consumer_domain: impl Into<String>,
        producer_domain: impl Into<String>,
        scope: ScopePattern,
        created_at: DateTime<Utc>,
        created_by: impl Into<String>,
    ) -> Result<Self, EntitlementModelError> {
        let consumer_domain = consumer_domain.into();
        let producer_domain = producer_domain.into();
        let created_by = created_by.into();
        if consumer_domain.is_empty() {
            return Err(EntitlementModelError::EmptyConsumerDomain);
        }
        if producer_domain.is_empty() {
            return Err(EntitlementModelError::EmptyProducerDomain);
        }
        reject_unsafe_segment(&consumer_domain)?;
        reject_unsafe_segment(&producer_domain)?;
        if created_by.is_empty() {
            return Err(EntitlementModelError::EmptyActor);
        }
        let entitlement_id = derive_id(&consumer_domain, &producer_domain, scope.as_str());
        Ok(Self {
            entitlement_id,
            consumer_domain,
            producer_domain,
            scope,
            created_at,
            created_by,
        })
    }

    /// `CONSUMER#{consumerDomain}` — the entitlement partition key.
    pub fn pk(&self) -> String {
        format!("CONSUMER#{}", self.consumer_domain)
    }

    /// `ENTITLEMENT#{entitlementId}` — the entitlement sort key.
    pub fn sk(&self) -> String {
        format!("ENTITLEMENT#{}", self.entitlement_id)
    }

    /// `PRODUCER#{producerDomain}` — the producer reverse GSI partition key.
    pub fn gsi1pk(&self) -> String {
        format!("PRODUCER#{}", self.producer_domain)
    }

    /// `CONSUMER#{consumerDomain}#ENTITLEMENT#{entitlementId}` — the producer
    /// reverse GSI sort key.
    pub fn gsi1sk(&self) -> String {
        format!(
            "CONSUMER#{}#ENTITLEMENT#{}",
            self.consumer_domain, self.entitlement_id
        )
    }
}

fn derive_id(consumer_domain: &str, producer_domain: &str, pattern: &str) -> String {
    Uuid::new_v5(
        &ENTITLEMENT_NAMESPACE,
        format!("{consumer_domain}\n{producer_domain}\n{pattern}").as_bytes(),
    )
    .to_string()
}

/// A token-free before/after view of an entitlement for audit history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementSummary {
    pub consumer_domain: String,
    pub producer_domain: String,
    pub scope_pattern: String,
}

impl From<&Entitlement> for EntitlementSummary {
    fn from(entitlement: &Entitlement) -> Self {
        Self {
            consumer_domain: entitlement.consumer_domain.clone(),
            producer_domain: entitlement.producer_domain.clone(),
            scope_pattern: entitlement.scope.as_str().to_string(),
        }
    }
}

/// An audit record committed in the same transaction as its entitlement
/// mutation. Carries mutation type, trusted actor, timestamp, and
/// before/after summary; never tokens, claims material, or IAM principal ARNs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementAuditRecord {
    pub entitlement_id: String,
    pub timestamp: DateTime<Utc>,
    pub mutation: AuditMutation,
    pub actor: String,
    pub before: Option<EntitlementSummary>,
    pub after: Option<EntitlementSummary>,
}

impl EntitlementAuditRecord {
    /// `ENTITLEMENT#{entitlementId}#AUDIT` — the audit partition key.
    pub fn pk(&self) -> String {
        format!("ENTITLEMENT#{}#AUDIT", self.entitlement_id)
    }

    /// `TS#{rfc3339 timestamp}` — the audit sort key; RFC 3339 orders
    /// lexicographically, so audit history is time-ordered per entitlement.
    pub fn sk(&self) -> String {
        format!("TS#{}", self.timestamp.to_rfc3339())
    }
}

/// The `POST /entitlements` request body.
///
/// Strict: `deny_unknown_fields` means callers cannot supply server-assigned
/// fields (`entitlementId`, `createdAt`, `createdBy`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateEntitlementRequest {
    pub consumer_domain: String,
    pub producer_domain: String,
    pub scope_pattern: String,
}

impl CreateEntitlementRequest {
    /// Builds the entitlement, deriving the stable identifier and validating
    /// the scope pattern and segments via the model.
    pub fn into_entitlement(
        self,
        created_at: DateTime<Utc>,
        created_by: impl Into<String>,
    ) -> Result<Entitlement, EntitlementModelError> {
        Entitlement::new(
            self.consumer_domain,
            self.producer_domain,
            ScopePattern::new(self.scope_pattern)?,
            created_at,
            created_by,
        )
    }
}

/// The API view of a stored entitlement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementView {
    pub entitlement_id: String,
    pub consumer_domain: String,
    pub producer_domain: String,
    pub scope_pattern: String,
    pub created_at: String,
    pub created_by: String,
}

impl From<&Entitlement> for EntitlementView {
    fn from(entitlement: &Entitlement) -> Self {
        Self {
            entitlement_id: entitlement.entitlement_id.clone(),
            consumer_domain: entitlement.consumer_domain.clone(),
            producer_domain: entitlement.producer_domain.clone(),
            scope_pattern: entitlement.scope.as_str().to_string(),
            created_at: entitlement.created_at.to_rfc3339(),
            created_by: entitlement.created_by.clone(),
        }
    }
}

/// The `GET /entitlements` response body.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEntitlementsResponse {
    pub entitlements: Vec<EntitlementView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_ts() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z")
            .unwrap()
            .to_utc()
    }

    fn entitlement(scope: &str) -> Entitlement {
        Entitlement::new(
            "checkout",
            "orders",
            ScopePattern::new(scope).unwrap(),
            fixed_ts(),
            "platform-admin-sub",
        )
        .unwrap()
    }

    #[test]
    fn accepts_domain_wide_exact_and_prefix_patterns() {
        assert!(ScopePattern::new("*").is_ok());
        assert!(ScopePattern::new("OrderCreated").is_ok());
        assert!(ScopePattern::new("Order*").is_ok());
        assert!(ScopePattern::new("Order-V2.Beta_1*").is_ok());
    }

    #[test]
    fn rejects_malformed_patterns() {
        for value in [
            "",
            "*Order",
            "Or*der",
            "**",
            "Order Created",
            "order#x",
            "order/x",
            "order*extra*",
        ] {
            assert!(
                matches!(
                    ScopePattern::new(value),
                    Err(EntitlementModelError::InvalidScopePattern(_))
                ),
                "must reject {value:?}"
            );
        }
    }

    #[test]
    fn domain_wide_matches_everything() {
        let pattern = ScopePattern::new("*").unwrap();
        assert!(pattern.is_domain_wide());
        assert!(pattern.matches("OrderCreated"));
        assert!(pattern.matches("anything.else_V2"));
    }

    #[test]
    fn exact_pattern_matches_only_itself() {
        let pattern = ScopePattern::new("OrderCreated").unwrap();
        assert!(!pattern.is_domain_wide());
        assert!(pattern.matches("OrderCreated"));
        assert!(!pattern.matches("OrderCreatedV2"));
        assert!(!pattern.matches("ordercreated"));
    }

    #[test]
    fn prefix_pattern_matches_family() {
        let pattern = ScopePattern::new("Order*").unwrap();
        assert!(pattern.matches("OrderCreated"));
        assert!(pattern.matches("Order"));
        assert!(!pattern.matches("Ord"));
        assert!(!pattern.matches("CustomerOrderCreated"));
    }

    #[test]
    fn entitlement_id_is_derived_from_the_scope_triple() {
        let a = entitlement("*");
        let b = entitlement("*");
        let c = entitlement("OrderCreated");
        assert_eq!(a.entitlement_id, b.entitlement_id);
        assert_ne!(a.entitlement_id, c.entitlement_id);
        let other_consumer = Entitlement::new(
            "returns",
            "orders",
            ScopePattern::new("*").unwrap(),
            fixed_ts(),
            "platform-admin-sub",
        )
        .unwrap();
        assert_ne!(a.entitlement_id, other_consumer.entitlement_id);
    }

    #[test]
    fn entitlement_keys_match_documented_patterns() {
        let entitlement = entitlement("Order*");
        assert_eq!(entitlement.pk(), "CONSUMER#checkout");
        assert_eq!(
            entitlement.sk(),
            format!("ENTITLEMENT#{}", entitlement.entitlement_id)
        );
        assert_eq!(entitlement.gsi1pk(), "PRODUCER#orders");
        assert_eq!(
            entitlement.gsi1sk(),
            format!(
                "CONSUMER#checkout#ENTITLEMENT#{}",
                entitlement.entitlement_id
            )
        );
    }

    #[test]
    fn model_rejects_empty_or_unsafe_segments() {
        let pattern = ScopePattern::new("*").unwrap();
        assert!(matches!(
            Entitlement::new("", "orders", pattern.clone(), fixed_ts(), "actor"),
            Err(EntitlementModelError::EmptyConsumerDomain)
        ));
        assert!(matches!(
            Entitlement::new("checkout", "", pattern.clone(), fixed_ts(), "actor"),
            Err(EntitlementModelError::EmptyProducerDomain)
        ));
        assert!(matches!(
            Entitlement::new("check#out", "orders", pattern.clone(), fixed_ts(), "actor"),
            Err(EntitlementModelError::UnsafeKeySegment(_))
        ));
        assert!(matches!(
            Entitlement::new("checkout", "orders", pattern, fixed_ts(), ""),
            Err(EntitlementModelError::EmptyActor)
        ));
    }

    #[test]
    fn audit_keys_are_time_ordered_per_entitlement() {
        let entitlement = entitlement("*");
        let record = EntitlementAuditRecord {
            entitlement_id: entitlement.entitlement_id.clone(),
            timestamp: fixed_ts(),
            mutation: AuditMutation::Revoke,
            actor: "platform-admin-sub".to_string(),
            before: Some(EntitlementSummary::from(&entitlement)),
            after: None,
        };
        assert_eq!(
            record.pk(),
            format!("ENTITLEMENT#{}#AUDIT", entitlement.entitlement_id)
        );
        assert_eq!(record.sk(), "TS#2026-08-01T00:00:00+00:00");
    }

    #[test]
    fn scope_pattern_deserialization_validates() {
        let ok: ScopePattern = serde_json::from_value(serde_json::json!("Order*")).unwrap();
        assert_eq!(ok.as_str(), "Order*");
        assert!(serde_json::from_value::<ScopePattern>(serde_json::json!("Or*der")).is_err());
    }

    #[test]
    fn entitlement_serde_round_trips() {
        let entitlement = entitlement("Order*");
        let json = serde_json::to_value(&entitlement).unwrap();
        assert_eq!(json["consumerDomain"], "checkout");
        assert_eq!(json["producerDomain"], "orders");
        assert_eq!(json["scopePattern"], "Order*");
        let parsed: Entitlement = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, entitlement);
    }

    #[test]
    fn create_request_builds_entitlement_and_rejects_server_fields() {
        let request: CreateEntitlementRequest = serde_json::from_value(serde_json::json!({
            "consumerDomain": "checkout",
            "producerDomain": "orders",
            "scopePattern": "Order*"
        }))
        .unwrap();
        let entitlement = request
            .into_entitlement(fixed_ts(), "platform-admin-sub")
            .unwrap();
        assert_eq!(entitlement.consumer_domain, "checkout");
        assert_eq!(entitlement.scope, ScopePattern::new("Order*").unwrap());

        let with_server_field = serde_json::json!({
            "consumerDomain": "checkout",
            "producerDomain": "orders",
            "scopePattern": "*",
            "entitlementId": "caller-supplied"
        });
        assert!(serde_json::from_value::<CreateEntitlementRequest>(with_server_field).is_err());
    }

    #[test]
    fn create_request_rejects_malformed_scope() {
        let request: CreateEntitlementRequest = serde_json::from_value(serde_json::json!({
            "consumerDomain": "checkout",
            "producerDomain": "orders",
            "scopePattern": "Or*der"
        }))
        .unwrap();
        assert!(matches!(
            request.into_entitlement(fixed_ts(), "platform-admin-sub"),
            Err(EntitlementModelError::InvalidScopePattern(_))
        ));
    }

    #[test]
    fn entitlement_view_serializes_camel_case() {
        let view = EntitlementView::from(&entitlement("Order*"));
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["consumerDomain"], "checkout");
        assert_eq!(json["producerDomain"], "orders");
        assert_eq!(json["scopePattern"], "Order*");
        assert_eq!(json["createdAt"], "2026-08-01T00:00:00+00:00");
        assert_eq!(json["createdBy"], "platform-admin-sub");
        assert!(json["entitlementId"].is_string());
    }
}
