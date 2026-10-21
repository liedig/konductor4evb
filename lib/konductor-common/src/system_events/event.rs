//! The fully constructed public system event: validation, PutEvents entry
//! rendering, and SNS attribute projection for the draft-4 contracts.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::envelope::PUT_EVENTS_ENTRY_MAX_BYTES;
use crate::error::{ControlPlaneError, ValidationExceptionField, ValidationExceptionReason};

use super::models::{
    SystemEventData, SystemEventDetail, SystemEventMetadata, SystemEventType, UpgradeOutcome,
    SYSTEM_EVENT_SOURCE,
};

/// A fully constructed system event ready for PutEvents on the system bus.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemEvent {
    pub source: String,
    pub detail_type: String,
    pub event_bus_name: String,
    pub detail: SystemEventDetail,
}

/// One SNS message attribute: value plus wire data type (`String` or
/// `Number`).
#[derive(Debug, Clone, PartialEq)]
pub struct SnsAttribute {
    pub value: String,
    pub data_type: SnsAttributeDataType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnsAttributeDataType {
    String,
    Number,
}

impl SnsAttributeDataType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::String => "String",
            Self::Number => "Number",
        }
    }
}

impl SystemEvent {
    /// Builds and validates a system event. The data shape must match the
    /// event type's phase: start events carry no outcome or resulting
    /// contract; terminal events carry both, plus a safe reason when partial.
    pub fn new(
        event_type: SystemEventType,
        event_bus_name: String,
        metadata: SystemEventMetadata,
        data: SystemEventData,
    ) -> Result<Self, ControlPlaneError> {
        let event = Self {
            source: SYSTEM_EVENT_SOURCE.to_string(),
            detail_type: event_type.detail_type().to_string(),
            event_bus_name,
            detail: SystemEventDetail { metadata, data },
        };
        event.validate(event_type)?;
        Ok(event)
    }

    fn validate(&self, event_type: SystemEventType) -> Result<(), ControlPlaneError> {
        let metadata = &self.detail.metadata;
        let data = &self.detail.data;
        if metadata.schema_version <= 0 {
            return Err(event_error(
                "schemaVersion",
                "contract version must be positive",
            ));
        }
        if metadata.idempotency_key.is_empty() {
            return Err(event_error(
                "idempotencyKey",
                "idempotency key must not be empty",
            ));
        }
        if metadata.correlation_id.is_empty() {
            return Err(event_error(
                "correlationId",
                "correlation id must not be empty",
            ));
        }
        if metadata.resource_revision == 0 {
            return Err(event_error(
                "resourceRevision",
                "resource revision must be positive",
            ));
        }
        if data.domain.is_empty() || data.producer_domain.is_empty() {
            return Err(event_error("domain", "domain names must not be empty"));
        }
        if data.from_version <= 0 || data.requested_version <= 0 {
            return Err(event_error(
                "fromVersion",
                "business versions must be positive",
            ));
        }
        if let Some(range) = &data.intermediate_versions {
            if range.from_version <= data.from_version
                || range.to_version < range.from_version
                || range.to_version >= data.requested_version
            {
                return Err(event_error(
                    "intermediateVersions",
                    "range must lie strictly between from and requested versions",
                ));
            }
        }
        if event_type.is_terminal() {
            validate_terminal(data)?;
        } else if data.resulting_version.is_some()
            || data.contracts.resulting.is_some()
            || data.outcome.is_some()
            || data.reason.is_some()
        {
            return Err(event_error(
                "outcome",
                "start events carry no outcome, reason, or resulting contract",
            ));
        }
        let entry_bytes = serde_json::to_vec(&self.entry_json())
            .map_err(|_| event_error("detail", "event is not serializable"))?;
        if entry_bytes.len() > PUT_EVENTS_ENTRY_MAX_BYTES {
            return Err(event_error(
                "detail",
                "event exceeds the PutEvents entry limit",
            ));
        }
        Ok(())
    }

    /// The PutEvents entry shape for the system bus.
    pub fn entry_json(&self) -> Value {
        json!({
            "Source": self.source,
            "DetailType": self.detail_type,
            "EventBusName": self.event_bus_name,
            "Detail": serde_json::to_string(&self.detail).unwrap_or_default(),
        })
    }

    /// The full EventBridge event as delivered to consumers: this is the
    /// SNS message body the router publishes. Transport `id` is routing
    /// metadata, never part of the identity contract.
    pub fn full_event_json(
        &self,
        id: &str,
        account: &str,
        region: &str,
        time: DateTime<Utc>,
    ) -> Value {
        json!({
            "version": "0",
            "id": id,
            "detail-type": self.detail_type,
            "source": self.source,
            "account": account,
            "time": time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "region": region,
            "resources": [],
            "detail": serde_json::to_value(&self.detail).unwrap_or_default(),
        })
    }

    /// The stable SNS filter attributes. Body and attribute values agree by
    /// construction; a null environment maps to the attribute value `ALL`.
    /// Terminal events additionally carry `outcome`.
    pub fn sns_attributes(&self) -> BTreeMap<String, SnsAttribute> {
        let data = &self.detail.data;
        let string = |value: &str| SnsAttribute {
            value: value.to_string(),
            data_type: SnsAttributeDataType::String,
        };
        let mut attributes = BTreeMap::new();
        attributes.insert("eventType".to_string(), string(&self.detail_type));
        attributes.insert(
            "schemaVersion".to_string(),
            SnsAttribute {
                value: self.detail.metadata.schema_version.to_string(),
                data_type: SnsAttributeDataType::Number,
            },
        );
        attributes.insert("audience".to_string(), string(data.audience.as_str()));
        attributes.insert("domain".to_string(), string(&data.domain));
        attributes.insert(
            "environment".to_string(),
            string(data.environment.as_deref().unwrap_or("ALL")),
        );
        attributes.insert(
            "resourceType".to_string(),
            string(data.resource.resource_type.as_str()),
        );
        attributes.insert("trigger".to_string(), string(data.trigger.as_str()));
        if let Some(outcome) = &data.outcome {
            attributes.insert("outcome".to_string(), string(outcome.as_str()));
        }
        attributes
    }
}

fn validate_terminal(data: &SystemEventData) -> Result<(), ControlPlaneError> {
    match data.resulting_version {
        Some(version) if version > 0 => {}
        _ => {
            return Err(event_error(
                "resultingVersion",
                "terminal events need a positive resulting version",
            ))
        }
    }
    if data.contracts.resulting.is_none() {
        return Err(event_error(
            "contracts",
            "terminal events need the resulting contract selector",
        ));
    }
    match (data.outcome, data.reason) {
        (Some(UpgradeOutcome::Completed), None) => Ok(()),
        (Some(UpgradeOutcome::Partial), Some(_)) => Ok(()),
        (Some(UpgradeOutcome::Partial), None) => {
            Err(event_error("reason", "partial outcomes need a safe reason"))
        }
        (Some(UpgradeOutcome::Completed), Some(_)) => {
            Err(event_error("reason", "completed outcomes carry no reason"))
        }
        (None, _) => Err(event_error("outcome", "terminal events need an outcome")),
    }
}

fn event_error(field: &str, message: &str) -> ControlPlaneError {
    ControlPlaneError::ValidationException {
        message: message.to_string(),
        reason: ValidationExceptionReason::FieldValidationFailed,
        field_list: Some(vec![ValidationExceptionField {
            name: field.to_string(),
            message: message.to_string(),
        }]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streams::StreamStatus;
    use crate::system_events::models::*;

    const STARTED_SCHEMA: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../api/system-events/subscription-upgrade-started.schema.json"
    ));
    const UPGRADED_SCHEMA: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../api/system-events/subscription-upgraded.schema.json"
    ));

    fn compile(source: &str) -> jsonschema::Validator {
        let document: Value = serde_json::from_str(source).unwrap();
        jsonschema::options()
            .with_draft(jsonschema::Draft::Draft4)
            .build(&document)
            .unwrap()
    }

    fn assert_valid(schema_source: &str, event: &Value) {
        let validator = compile(schema_source);
        let errors: Vec<String> = validator
            .iter_errors(event)
            .map(|e| e.to_string())
            .collect();
        assert!(
            errors.is_empty(),
            "event failed schema validation: {errors:?}"
        );
    }

    fn selector(version: i64, metadata_revision: u32) -> ContractSelector {
        ContractSelector {
            producer_domain: "orders".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            environment: "prod".to_string(),
            schema_version: version,
            metadata_revision,
        }
    }

    fn metadata(phase_revision: u64) -> SystemEventMetadata {
        SystemEventMetadata {
            schema_version: SYSTEM_EVENT_CONTRACT_VERSION,
            idempotency_key: "system-event/v1/abc123".to_string(),
            published_at: DateTime::from_timestamp_millis(1785407400123).unwrap(),
            correlation_id: "promotion-42".to_string(),
            resource_revision: phase_revision,
        }
    }

    fn data() -> SystemEventData {
        SystemEventData {
            audience: SystemEventAudience::Consumer,
            domain: "checkout".to_string(),
            environment: Some("prod".to_string()),
            resource: SystemResource {
                resource_type: SystemResourceType::Subscription,
                id: "checkout.orders.OrderNotification".to_string(),
            },
            producer_domain: "orders".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            trigger: UpgradeTrigger::Promotion,
            from_version: 3,
            requested_version: 6,
            resulting_version: None,
            intermediate_versions: Some(VersionRange {
                from_version: 4,
                to_version: 5,
            }),
            enabled: true,
            subscription_status: StreamStatus::Active,
            expected_repin_at: Some(DateTime::from_timestamp_millis(1785407700123).unwrap()),
            contracts: ContractSelectors {
                from: selector(3, 0),
                requested: selector(6, 2),
                resulting: None,
            },
            outcome: None,
            reason: None,
        }
    }

    fn started_event() -> SystemEvent {
        SystemEvent::new(
            SystemEventType::SubscriptionUpgradeStarted,
            "acme-konductor-system-bus".to_string(),
            metadata(5),
            data(),
        )
        .unwrap()
    }

    fn upgraded_event(outcome: UpgradeOutcome, reason: Option<PartialReason>) -> SystemEvent {
        let mut upgraded = data();
        upgraded.resulting_version = Some(6);
        upgraded.contracts.resulting = Some(selector(6, 2));
        upgraded.outcome = Some(outcome);
        upgraded.reason = reason;
        SystemEvent::new(
            SystemEventType::SubscriptionUpgraded,
            "acme-konductor-system-bus".to_string(),
            metadata(6),
            upgraded,
        )
        .unwrap()
    }

    fn wire(event: &SystemEvent) -> Value {
        event.full_event_json(
            "evt-123",
            "123456789012",
            "eu-central-1",
            DateTime::from_timestamp_millis(1785407400500).unwrap(),
        )
    }

    #[test]
    fn started_fixture_validates_against_its_schema() {
        assert_valid(STARTED_SCHEMA, &wire(&started_event()));
    }

    #[test]
    fn started_fixture_supports_authoring_trigger() {
        let mut authoring = data();
        authoring.trigger = UpgradeTrigger::Authoring;
        authoring.intermediate_versions = None;
        let event = SystemEvent::new(
            SystemEventType::SubscriptionUpgradeStarted,
            "acme-konductor-system-bus".to_string(),
            metadata(1),
            authoring,
        )
        .unwrap();
        assert_valid(STARTED_SCHEMA, &wire(&event));
    }

    #[test]
    fn start_results_are_null() {
        let detail = serde_json::to_value(&started_event().detail).unwrap();
        let payload = &detail["data"];
        assert_eq!(payload["resultingVersion"], Value::Null);
        assert_eq!(payload["contracts"]["resulting"], Value::Null);
        assert!(payload.get("outcome").is_none());
        assert!(payload.get("reason").is_none());
        // ...and a start payload must not pass as a terminal event.
        assert!(!compile(UPGRADED_SCHEMA).is_valid(&wire(&started_event())));
    }

    #[test]
    fn upgraded_completed_fixture_validates() {
        assert_valid(
            UPGRADED_SCHEMA,
            &wire(&upgraded_event(UpgradeOutcome::Completed, None)),
        );
    }

    #[test]
    fn upgraded_partial_fixture_validates_with_safe_reason() {
        let event = upgraded_event(UpgradeOutcome::Partial, Some(PartialReason::ReplayPartial));
        assert_valid(UPGRADED_SCHEMA, &wire(&event));
        let detail = serde_json::to_value(&event.detail).unwrap();
        assert_eq!(detail["data"]["outcome"], json!("PARTIAL"));
        assert_eq!(detail["data"]["reason"], json!("REPLAY_PARTIAL"));
    }

    #[test]
    fn legacy_metadata_revision_zero_selector_validates() {
        let event = started_event();
        let payload = serde_json::to_value(&event.detail).unwrap();
        assert_eq!(
            payload["data"]["contracts"]["from"]["metadataRevision"],
            json!(0)
        );
        assert_valid(STARTED_SCHEMA, &wire(&event));
    }

    #[test]
    fn malformed_payloads_are_rejected_by_schema() {
        let mut missing_outcome = wire(&upgraded_event(UpgradeOutcome::Completed, None));
        missing_outcome["detail"]["data"]
            .as_object_mut()
            .unwrap()
            .remove("outcome");
        assert!(!compile(UPGRADED_SCHEMA).is_valid(&missing_outcome));

        let mut bad_trigger = wire(&started_event());
        bad_trigger["detail"]["data"]["trigger"] = json!("DEPLOYMENT");
        assert!(!compile(STARTED_SCHEMA).is_valid(&bad_trigger));

        let mut extra_field = wire(&started_event());
        extra_field["detail"]["data"]["rawAwsError"] = json!("ThrottlingException");
        assert!(!compile(STARTED_SCHEMA).is_valid(&extra_field));
    }

    #[test]
    fn builder_rejects_terminal_fields_on_start_events() {
        let mut invalid = data();
        invalid.outcome = Some(UpgradeOutcome::Completed);
        assert!(SystemEvent::new(
            SystemEventType::SubscriptionUpgradeStarted,
            "bus".to_string(),
            metadata(5),
            invalid,
        )
        .is_err());
    }

    #[test]
    fn builder_enforces_outcome_reason_pairing() {
        assert!(SystemEvent::new(
            SystemEventType::SubscriptionUpgraded,
            "bus".to_string(),
            metadata(6),
            {
                let mut partial_without_reason = data();
                partial_without_reason.resulting_version = Some(6);
                partial_without_reason.contracts.resulting = Some(selector(6, 2));
                partial_without_reason.outcome = Some(UpgradeOutcome::Partial);
                partial_without_reason
            },
        )
        .is_err());

        assert!(SystemEvent::new(
            SystemEventType::SubscriptionUpgraded,
            "bus".to_string(),
            metadata(6),
            {
                let mut completed_with_reason = data();
                completed_with_reason.resulting_version = Some(6);
                completed_with_reason.contracts.resulting = Some(selector(6, 2));
                completed_with_reason.outcome = Some(UpgradeOutcome::Completed);
                completed_with_reason.reason = Some(PartialReason::RepinFailed);
                completed_with_reason
            },
        )
        .is_err());
    }

    #[test]
    fn builder_enforces_intermediate_range_bounds() {
        for (from, to) in [(3, 5), (4, 6), (5, 4)] {
            let mut invalid = data();
            invalid.intermediate_versions = Some(VersionRange {
                from_version: from,
                to_version: to,
            });
            assert!(
                SystemEvent::new(
                    SystemEventType::SubscriptionUpgradeStarted,
                    "bus".to_string(),
                    metadata(5),
                    invalid,
                )
                .is_err(),
                "range {from}..={to} must be rejected"
            );
        }
    }

    #[test]
    fn sns_attributes_agree_with_body_and_stay_under_limit() {
        let started = started_event();
        let attributes = started.sns_attributes();
        assert_eq!(attributes.len(), 7);
        assert_eq!(attributes["eventType"].value, "SubscriptionUpgradeStarted");
        assert_eq!(attributes["schemaVersion"].value, "1");
        assert_eq!(
            attributes["schemaVersion"].data_type,
            SnsAttributeDataType::Number
        );
        assert_eq!(attributes["audience"].value, "CONSUMER");
        assert_eq!(attributes["domain"].value, "checkout");
        assert_eq!(attributes["environment"].value, "prod");
        assert_eq!(attributes["resourceType"].value, "SUBSCRIPTION");
        assert_eq!(attributes["trigger"].value, "PROMOTION");
        assert!(!attributes.contains_key("outcome"));

        let terminal = upgraded_event(UpgradeOutcome::Partial, Some(PartialReason::RepinFailed));
        let terminal_attributes = terminal.sns_attributes();
        assert_eq!(terminal_attributes.len(), 8);
        assert_eq!(terminal_attributes["outcome"].value, "PARTIAL");
    }

    #[test]
    fn null_environment_maps_to_all_attribute() {
        let mut domain_wide = data();
        domain_wide.environment = None;
        let event = SystemEvent::new(
            SystemEventType::SubscriptionUpgradeStarted,
            "bus".to_string(),
            metadata(5),
            domain_wide,
        )
        .unwrap();
        assert_eq!(event.sns_attributes()["environment"].value, "ALL");
        assert_valid(STARTED_SCHEMA, &wire(&event));
    }

    #[test]
    fn entry_json_carries_canonical_system_identity() {
        let entry = started_event().entry_json();
        assert_eq!(entry["Source"], json!("konductor.system"));
        assert_eq!(entry["DetailType"], json!("SubscriptionUpgradeStarted"));
        assert_eq!(entry["EventBusName"], json!("acme-konductor-system-bus"));
    }

    #[test]
    fn detail_round_trips_through_serde() {
        let event = upgraded_event(UpgradeOutcome::Partial, Some(PartialReason::ReplayPartial));
        let value = serde_json::to_value(&event.detail).unwrap();
        let back: SystemEventDetail = serde_json::from_value(value).unwrap();
        assert_eq!(back, event.detail);
    }
}
