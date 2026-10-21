//! Typed mirror of the `api/system-events/` draft-4 contracts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::streams::StreamStatus;

/// EventBridge `source` shared by every public system event.
pub const SYSTEM_EVENT_SOURCE: &str = "konductor.system";

/// Current public event contract version, stamped as
/// `detail.metadata.schemaVersion`.
pub const SYSTEM_EVENT_CONTRACT_VERSION: i64 = 1;

/// The two public system event types of the upgrade family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemEventType {
    SubscriptionUpgradeStarted,
    SubscriptionUpgraded,
}

impl SystemEventType {
    pub fn detail_type(&self) -> &'static str {
        match self {
            Self::SubscriptionUpgradeStarted => "SubscriptionUpgradeStarted",
            Self::SubscriptionUpgraded => "SubscriptionUpgraded",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::SubscriptionUpgraded)
    }

    /// Parses a public detail type back into its event type.
    pub fn from_detail_type(detail_type: &str) -> Option<Self> {
        match detail_type {
            "SubscriptionUpgradeStarted" => Some(Self::SubscriptionUpgradeStarted),
            "SubscriptionUpgraded" => Some(Self::SubscriptionUpgraded),
            _ => None,
        }
    }
}

/// What started the upgrade: a schema promotion or an authoring append.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UpgradeTrigger {
    Promotion,
    Authoring,
}

impl UpgradeTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Promotion => "PROMOTION",
            Self::Authoring => "AUTHORING",
        }
    }
}

/// The truthful terminal result of an upgrade phase.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UpgradeOutcome {
    Completed,
    Partial,
}

impl UpgradeOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "COMPLETED",
            Self::Partial => "PARTIAL",
        }
    }
}

/// Safe, public classification for a partial outcome. Raw AWS errors are
/// never part of the public payload.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PartialReason {
    ReplayPartial,
    RepinFailed,
}

impl PartialReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReplayPartial => "REPLAY_PARTIAL",
            Self::RepinFailed => "REPIN_FAILED",
        }
    }
}

/// Who an event is addressed to. The MVP publishes only consumer events.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SystemEventAudience {
    Consumer,
}

impl SystemEventAudience {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Consumer => "CONSUMER",
        }
    }
}

/// The kind of resource an event describes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SystemResourceType {
    Subscription,
}

impl SystemResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Subscription => "SUBSCRIPTION",
        }
    }
}

/// The resource an event describes, identified by its stable API id
/// (`{consumer}.{producer}.{detailType}`), never a transport id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemResource {
    #[serde(rename = "type")]
    pub resource_type: SystemResourceType,
    pub id: String,
}

/// One exact composed contract: producer, canonical registry schema name,
/// environment, business schema version, and metadata revision. Metadata
/// revision 0 is valid for legacy subscriptions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContractSelector {
    pub producer_domain: String,
    pub schema_name: String,
    pub environment: String,
    pub schema_version: i64,
    pub metadata_revision: u32,
}

/// The contract references of one phase. `resulting` is null at start and
/// present in terminal events; `from` and `requested` are retained across
/// both phases.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContractSelectors {
    pub from: ContractSelector,
    pub requested: ContractSelector,
    pub resulting: Option<ContractSelector>,
}

/// A compact inclusive range of intermediate business versions replayed
/// between the original and requested versions. Never includes the
/// lookahead slot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VersionRange {
    pub from_version: i64,
    pub to_version: i64,
}

/// Platform-owned identity block of a public system event. All values are
/// fixed when the phase snapshot is committed; retries republish them
/// unchanged.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemEventMetadata {
    pub schema_version: i64,
    pub idempotency_key: String,
    #[serde(with = "crate::envelope::iso8601_millis")]
    pub published_at: DateTime<Utc>,
    pub correlation_id: String,
    pub resource_revision: u64,
}

/// The event-specific payload of one upgrade phase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemEventData {
    pub audience: SystemEventAudience,
    pub domain: String,
    pub environment: Option<String>,
    pub resource: SystemResource,
    pub producer_domain: String,
    pub schema_name: String,
    pub trigger: UpgradeTrigger,
    pub from_version: i64,
    pub requested_version: i64,
    pub resulting_version: Option<i64>,
    pub intermediate_versions: Option<VersionRange>,
    pub enabled: bool,
    pub subscription_status: StreamStatus,
    pub expected_repin_at: Option<DateTime<Utc>>,
    pub contracts: ContractSelectors,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<UpgradeOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<PartialReason>,
}

/// The canonical `detail` of a system event: platform metadata plus data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemEventDetail {
    pub metadata: SystemEventMetadata,
    pub data: SystemEventData,
}
