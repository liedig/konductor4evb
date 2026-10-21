//! The durable per-entry upgrade phase snapshot committed to the
//! event-streams record. A snapshot is self-contained: the stream projector
//! turns it into a public system event without reading current subscription,
//! schema, or metadata state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::ControlPlaneError;
use crate::streams::StreamStatus;

use super::event::SystemEvent;
use super::identity::{idempotency_key, UpgradePhase};
use super::models::{
    ContractSelectors, PartialReason, SystemEventAudience, SystemEventData, SystemEventMetadata,
    SystemResource, SystemResourceType, UpgradeOutcome, UpgradeTrigger, VersionRange,
    SYSTEM_EVENT_CONTRACT_VERSION,
};

/// All facts needed to project one committed upgrade phase, stored at
/// `upgradeEvents.<environment>` on the event-stream record. Kept outside
/// `environments.<env>` so repair and materialization writes cannot erase it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeEventSnapshot {
    pub phase: UpgradePhase,
    pub operation_id: String,
    pub trigger: UpgradeTrigger,
    pub generation: u64,
    pub resource_revision: u64,
    pub idempotency_key: String,
    #[serde(with = "crate::envelope::iso8601_millis")]
    pub published_at: DateTime<Utc>,
    pub from_version: i64,
    pub requested_version: i64,
    pub resulting_version: Option<i64>,
    pub intermediate_versions: Option<VersionRange>,
    pub enabled: bool,
    pub subscription_status: StreamStatus,
    pub expected_repin_at: Option<DateTime<Utc>>,
    pub contracts: ContractSelectors,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<UpgradeOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<PartialReason>,
}

/// The identity inputs the snapshot does not carry because they come from
/// the enclosing event-stream record.
pub struct SnapshotScope<'a> {
    pub organization: &'a str,
    pub consumer_domain: &'a str,
    pub producer_domain: &'a str,
    pub schema_name: &'a str,
    pub detail_type: &'a str,
    pub environment: &'a str,
}

impl UpgradeEventSnapshot {
    /// Builds a phase snapshot, deriving the resource revision from the
    /// re-pin generation and the idempotency key from the full scope. The
    /// caller passes `published_at` once; the committed snapshot then fixes
    /// it for every delivery attempt.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scope: &SnapshotScope<'_>,
        phase: UpgradePhase,
        operation_id: &str,
        trigger: UpgradeTrigger,
        generation: u64,
        published_at: DateTime<Utc>,
        from_version: i64,
        requested_version: i64,
        resulting_version: Option<i64>,
        intermediate_versions: Option<VersionRange>,
        enabled: bool,
        subscription_status: StreamStatus,
        expected_repin_at: Option<DateTime<Utc>>,
        contracts: ContractSelectors,
        outcome: Option<UpgradeOutcome>,
        reason: Option<PartialReason>,
    ) -> Result<Self, ControlPlaneError> {
        let subscription_id = format!(
            "{}.{}.{}",
            scope.consumer_domain, scope.producer_domain, scope.detail_type
        );
        Ok(Self {
            phase,
            operation_id: operation_id.to_string(),
            trigger,
            generation,
            resource_revision: phase.resource_revision(generation)?,
            idempotency_key: idempotency_key(
                scope.organization,
                &subscription_id,
                scope.environment,
                operation_id,
                phase,
            ),
            published_at,
            from_version,
            requested_version,
            resulting_version,
            intermediate_versions,
            enabled,
            subscription_status,
            expected_repin_at,
            contracts,
            outcome,
            reason,
        })
    }

    /// Projects the snapshot into a validated public system event. The
    /// consumer-facing identity comes from the enclosing record; every
    /// phase fact comes from the snapshot.
    pub fn to_system_event(
        &self,
        scope: &SnapshotScope<'_>,
        event_bus_name: &str,
    ) -> Result<SystemEvent, ControlPlaneError> {
        let event_type = self.phase.event_type();
        SystemEvent::new(
            event_type,
            event_bus_name.to_string(),
            SystemEventMetadata {
                schema_version: SYSTEM_EVENT_CONTRACT_VERSION,
                idempotency_key: self.idempotency_key.clone(),
                published_at: self.published_at,
                correlation_id: self.operation_id.clone(),
                resource_revision: self.resource_revision,
            },
            SystemEventData {
                audience: SystemEventAudience::Consumer,
                domain: scope.consumer_domain.to_string(),
                environment: Some(scope.environment.to_string()),
                resource: SystemResource {
                    resource_type: SystemResourceType::Subscription,
                    id: format!(
                        "{}.{}.{}",
                        scope.consumer_domain, scope.producer_domain, scope.detail_type
                    ),
                },
                producer_domain: scope.producer_domain.to_string(),
                schema_name: scope.schema_name.to_string(),
                trigger: self.trigger,
                from_version: self.from_version,
                requested_version: self.requested_version,
                resulting_version: self.resulting_version,
                intermediate_versions: self.intermediate_versions.clone(),
                enabled: self.enabled,
                subscription_status: self.subscription_status,
                expected_repin_at: self.expected_repin_at,
                contracts: self.contracts.clone(),
                outcome: self.outcome,
                reason: self.reason,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_events::{ContractSelector, SystemEventType};

    fn scope() -> SnapshotScope<'static> {
        SnapshotScope {
            organization: "acme",
            consumer_domain: "checkout",
            producer_domain: "orders",
            schema_name: "acme-orders@OrderNotification",
            detail_type: "OrderNotification",
            environment: "prod",
        }
    }

    fn selectors() -> ContractSelectors {
        let selector = |version| ContractSelector {
            producer_domain: "orders".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            environment: "prod".to_string(),
            schema_version: version,
            metadata_revision: 0,
        };
        ContractSelectors {
            from: selector(3),
            requested: selector(4),
            resulting: None,
        }
    }

    fn start_snapshot() -> UpgradeEventSnapshot {
        UpgradeEventSnapshot::new(
            &scope(),
            UpgradePhase::Started,
            "promotion-42",
            UpgradeTrigger::Promotion,
            3,
            DateTime::from_timestamp_millis(1785407400123).unwrap(),
            3,
            4,
            None,
            None,
            true,
            StreamStatus::Active,
            Some(DateTime::from_timestamp_millis(1785407700123).unwrap()),
            selectors(),
            None,
            None,
        )
        .unwrap()
    }

    #[test]
    fn snapshot_derives_identity_from_generation_and_scope() {
        let snapshot = start_snapshot();
        assert_eq!(snapshot.resource_revision, 5);
        assert!(snapshot.idempotency_key.starts_with("system-event/v1/"));
    }

    #[test]
    fn start_snapshot_projects_a_valid_started_event() {
        let event = start_snapshot()
            .to_system_event(&scope(), "acme-konductor-system-bus")
            .unwrap();
        assert_eq!(
            event.detail_type,
            SystemEventType::SubscriptionUpgradeStarted.detail_type()
        );
        assert_eq!(event.detail.metadata.correlation_id, "promotion-42");
        assert_eq!(event.detail.metadata.resource_revision, 5);
        assert_eq!(event.detail.data.domain, "checkout");
        assert_eq!(
            event.detail.data.resource.id,
            "checkout.orders.OrderNotification"
        );
        assert_eq!(event.detail.data.resulting_version, None);
    }

    #[test]
    fn terminal_snapshot_projects_a_truthful_partial_event() {
        let mut terminal = start_snapshot();
        terminal.phase = UpgradePhase::Terminal;
        terminal.resource_revision = 6;
        terminal.resulting_version = Some(4);
        terminal.subscription_status = StreamStatus::Failed;
        terminal.outcome = Some(UpgradeOutcome::Partial);
        terminal.reason = Some(PartialReason::RepinFailed);
        terminal.contracts.resulting = Some(ContractSelector {
            producer_domain: "orders".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            environment: "prod".to_string(),
            schema_version: 4,
            metadata_revision: 0,
        });
        let event = terminal
            .to_system_event(&scope(), "acme-konductor-system-bus")
            .unwrap();
        assert_eq!(event.detail.data.subscription_status, StreamStatus::Failed);
        assert_eq!(event.detail.data.outcome, Some(UpgradeOutcome::Partial));
        assert_eq!(event.detail.data.reason, Some(PartialReason::RepinFailed));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snapshot = start_snapshot();
        let value = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(value["phase"], serde_json::json!("STARTED"));
        assert_eq!(value["trigger"], serde_json::json!("PROMOTION"));
        assert!(value.get("outcome").is_none());
        let back: UpgradeEventSnapshot = serde_json::from_value(value).unwrap();
        assert_eq!(back, snapshot);
    }
}
