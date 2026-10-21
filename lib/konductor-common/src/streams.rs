use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{ControlPlaneError, ValidationExceptionField, ValidationExceptionReason};

/// Days a removed stream record is retained (via DynamoDB TTL) for audit.
pub const REMOVED_RETENTION_DAYS: i64 = 90;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StreamStatus {
    Pending,
    Active,
    Failed,
    Removed,
    AwaitingPromotion,
}

impl StreamStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Active => "ACTIVE",
            Self::Failed => "FAILED",
            Self::Removed => "REMOVED",
            Self::AwaitingPromotion => "AWAITING_PROMOTION",
        }
    }
}

impl std::fmt::Display for StreamStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for StreamStatus {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "PENDING" => Ok(Self::Pending),
            "ACTIVE" => Ok(Self::Active),
            "FAILED" => Ok(Self::Failed),
            "REMOVED" => Ok(Self::Removed),
            "AWAITING_PROMOTION" => Ok(Self::AwaitingPromotion),
            _ => Err(()),
        }
    }
}

/// Per-environment materialization of a subscription: the pinned contract
/// version (resolved by the activation workflow — `None` while `PENDING`),
/// the rule realizing it, the desired enablement, and the materialized status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvStream {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_version: Option<i64>,
    pub rule_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_arn: Option<String>,
    pub enabled: bool,
    pub status: StreamStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_detail: Option<String>,
}

/// An event stream: the record of a subscription contract between a consumer
/// domain and a producer domain's schema. Source of intent — EventBridge
/// rules are its materialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventStream {
    pub producer_domain: String,
    pub consumer_domain: String,
    pub source: String,
    pub detail_type: String,
    pub schema_name: String,
    pub status: StreamStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_detail: Option<String>,
    pub environments: HashMap<String, EnvStream>,
    /// Latest committed upgrade phase per environment, kept outside
    /// `environments.<env>` so per-env repair writes cannot erase it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade_events: Option<HashMap<String, crate::system_events::UpgradeEventSnapshot>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

impl EventStream {
    /// Sort key in the event-streams table: `{schemaName}#{consumerDomain}`.
    /// Prefixing with the schema name makes the version census a single
    /// `begins_with` query per schema.
    pub fn sort_key(schema_name: &str, consumer_domain: &str) -> String {
        format!("{schema_name}#{consumer_domain}")
    }

    pub fn subscription_id(&self) -> SubscriptionId {
        SubscriptionId {
            consumer_domain: self.consumer_domain.clone(),
            producer_domain: self.producer_domain.clone(),
            detail_type: self.detail_type.clone(),
        }
    }
}

/// API-facing subscription identity: `{consumer}.{producer}.{detailType}`.
/// Domain names cannot contain dots, so parsing splits on the first two dots;
/// the detail-type portion may itself contain dots.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscriptionId {
    pub consumer_domain: String,
    pub producer_domain: String,
    pub detail_type: String,
}

impl SubscriptionId {
    pub fn parse(id: &str) -> Result<Self, ControlPlaneError> {
        let parts: Vec<&str> = id.splitn(3, '.').collect();
        match parts.as_slice() {
            [consumer, producer, detail_type]
                if !consumer.is_empty() && !producer.is_empty() && !detail_type.is_empty() =>
            {
                Ok(Self {
                    consumer_domain: consumer.to_string(),
                    producer_domain: producer.to_string(),
                    detail_type: detail_type.to_string(),
                })
            }
            _ => Err(ControlPlaneError::ValidationException {
                message: format!(
                    "subscription id '{id}' must be '{{consumer}}.{{producer}}.{{detailType}}'"
                ),
                reason: ValidationExceptionReason::FieldValidationFailed,
                field_list: Some(vec![ValidationExceptionField {
                    name: "subscriptionId".to_string(),
                    message: "expected {consumer}.{producer}.{detailType}".to_string(),
                }]),
            }),
        }
    }
}

impl std::fmt::Display for SubscriptionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}.{}",
            self.consumer_domain, self.producer_domain, self.detail_type
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_status_round_trips_wire_format() {
        for (status, wire) in [
            (StreamStatus::Pending, "PENDING"),
            (StreamStatus::Active, "ACTIVE"),
            (StreamStatus::Failed, "FAILED"),
            (StreamStatus::Removed, "REMOVED"),
            (StreamStatus::AwaitingPromotion, "AWAITING_PROMOTION"),
        ] {
            assert_eq!(status.to_string(), wire);
            assert_eq!(wire.parse::<StreamStatus>(), Ok(status));
            assert_eq!(
                serde_json::to_string(&status).unwrap(),
                format!("\"{wire}\"")
            );
        }
        assert!("DISABLED".parse::<StreamStatus>().is_err());
    }

    #[test]
    fn sort_key_prefixes_schema_name_for_census_queries() {
        assert_eq!(
            EventStream::sort_key("acme-orders@OrderNotification", "checkout"),
            "acme-orders@OrderNotification#checkout"
        );
    }

    #[test]
    fn subscription_id_round_trips() {
        let id = SubscriptionId::parse("checkout.orders.OrderNotification").unwrap();
        assert_eq!(id.consumer_domain, "checkout");
        assert_eq!(id.producer_domain, "orders");
        assert_eq!(id.detail_type, "OrderNotification");
        assert_eq!(id.to_string(), "checkout.orders.OrderNotification");
    }

    #[test]
    fn subscription_id_keeps_dots_in_detail_type() {
        let id = SubscriptionId::parse("checkout.orders.order.placed.v2").unwrap();
        assert_eq!(id.consumer_domain, "checkout");
        assert_eq!(id.producer_domain, "orders");
        assert_eq!(id.detail_type, "order.placed.v2");
    }

    #[test]
    fn subscription_id_rejects_too_few_segments() {
        assert!(SubscriptionId::parse("checkout.orders").is_err());
        assert!(SubscriptionId::parse("checkout").is_err());
        assert!(SubscriptionId::parse("").is_err());
    }

    #[test]
    fn subscription_id_rejects_empty_segments() {
        assert!(SubscriptionId::parse(".orders.OrderNotification").is_err());
        assert!(SubscriptionId::parse("checkout..OrderNotification").is_err());
        assert!(SubscriptionId::parse("checkout.orders.").is_err());
    }

    #[test]
    fn pending_env_stream_omits_unresolved_version() {
        let env = EnvStream {
            pinned_version: None,
            rule_name: "checkout-OrderNotification".to_string(),
            rule_arn: None,
            enabled: true,
            status: StreamStatus::Pending,
            error_detail: None,
        };
        let value = serde_json::to_value(&env).unwrap();
        assert!(value.get("pinnedVersion").is_none());
        assert_eq!(value["status"], "PENDING");
    }

    #[test]
    fn awaiting_promotion_env_round_trips_without_materialization() {
        let env = EnvStream {
            pinned_version: None,
            rule_name: "checkout-OrderNotification".to_string(),
            rule_arn: None,
            enabled: false,
            status: StreamStatus::AwaitingPromotion,
            error_detail: None,
        };
        let value = serde_json::to_value(&env).unwrap();
        assert_eq!(value["status"], "AWAITING_PROMOTION");
        assert!(value.get("ruleArn").is_none());
        assert!(value.get("pinnedVersion").is_none());
        assert!(value.get("errorDetail").is_none());
        let back: EnvStream = serde_json::from_value(value).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn failed_env_carries_error_detail() {
        let env = EnvStream {
            pinned_version: None,
            rule_name: "checkout-OrderNotification".to_string(),
            rule_arn: None,
            enabled: true,
            status: StreamStatus::Failed,
            error_detail: Some("PutRule throttled".to_string()),
        };
        let value = serde_json::to_value(&env).unwrap();
        assert_eq!(value["errorDetail"], "PutRule throttled");
        let back: EnvStream = serde_json::from_value(value).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn env_stream_serializes_camel_case() {
        let env = EnvStream {
            pinned_version: Some(3),
            rule_name: "checkout-OrderNotification".to_string(),
            rule_arn: None,
            enabled: true,
            status: StreamStatus::Active,
            error_detail: None,
        };
        let value = serde_json::to_value(&env).unwrap();
        assert_eq!(value["pinnedVersion"], 3);
        assert_eq!(value["ruleName"], "checkout-OrderNotification");
        assert_eq!(value["status"], "ACTIVE");
        assert!(value.get("ruleArn").is_none());
    }
}
