//! Stable identity and ordering for projected upgrade phases, derived from
//! the existing re-pin generation so no separate counter service is needed.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ControlPlaneError, ValidationExceptionField, ValidationExceptionReason};

use super::models::SystemEventType;

/// One committed upgrade phase for a subscription entry in one environment.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UpgradePhase {
    /// Re-pinning has started; the outcome is not yet committed.
    Started,
    /// The upgrade reached a committed terminal outcome.
    Terminal,
}

impl UpgradePhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Started => "STARTED",
            Self::Terminal => "TERMINAL",
        }
    }

    /// The public event type projecting this phase.
    pub fn event_type(&self) -> SystemEventType {
        match self {
            Self::Started => SystemEventType::SubscriptionUpgradeStarted,
            Self::Terminal => SystemEventType::SubscriptionUpgraded,
        }
    }

    /// Derives the public resource revision from the re-pin generation:
    /// `2*g - 1` for the start phase, `2*g` for the terminal phase.
    /// Revisions are ordered only within one subscription and environment;
    /// gaps are valid.
    pub fn resource_revision(&self, generation: u64) -> Result<u64, ControlPlaneError> {
        if generation == 0 {
            return Err(identity_error(
                "generation",
                "re-pin generation must be a positive number".to_string(),
            ));
        }
        let doubled = generation.checked_mul(2).ok_or_else(|| {
            identity_error(
                "generation",
                format!("generation {generation} overflows a revision"),
            )
        })?;
        Ok(match self {
            Self::Started => doubled - 1,
            Self::Terminal => doubled,
        })
    }
}

/// Versioned deterministic idempotency key for one published phase. The
/// same committed phase always hashes to the same key across delivery
/// retries; any change of scope, operation, or phase yields a different key.
pub fn idempotency_key(
    organization: &str,
    subscription_id: &str,
    environment: &str,
    operation_id: &str,
    phase: UpgradePhase,
) -> String {
    let material = [
        organization,
        subscription_id,
        environment,
        operation_id,
        phase.as_str(),
    ]
    .join("\u{1f}");
    let digest = Sha256::digest(format!("system-event/v1\u{1f}{material}").as_bytes());
    let mut key = String::with_capacity("system-event/v1/".len() + digest.len() * 2);
    key.push_str("system-event/v1/");
    for byte in digest {
        key.push_str(&format!("{byte:02x}"));
    }
    key
}

fn identity_error(field: &str, message: String) -> ControlPlaneError {
    ControlPlaneError::ValidationException {
        message: message.clone(),
        reason: ValidationExceptionReason::FieldValidationFailed,
        field_list: Some(vec![ValidationExceptionField {
            name: field.to_string(),
            message,
        }]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_revision_precedes_terminal_within_a_generation() {
        let start = UpgradePhase::Started.resource_revision(3).unwrap();
        let terminal = UpgradePhase::Terminal.resource_revision(3).unwrap();
        assert_eq!(start, 5);
        assert_eq!(terminal, 6);
    }

    #[test]
    fn newer_generations_sort_above_earlier_terminals() {
        let previous_terminal = UpgradePhase::Terminal.resource_revision(3).unwrap();
        let next_start = UpgradePhase::Started.resource_revision(4).unwrap();
        assert!(next_start > previous_terminal);
    }

    #[test]
    fn generation_zero_is_rejected() {
        assert!(UpgradePhase::Started.resource_revision(0).is_err());
        assert!(UpgradePhase::Terminal.resource_revision(0).is_err());
    }

    #[test]
    fn idempotency_key_is_deterministic_for_one_phase() {
        let first = idempotency_key(
            "acme",
            "checkout.orders.OrderNotification",
            "prod",
            "op-1",
            UpgradePhase::Started,
        );
        let second = idempotency_key(
            "acme",
            "checkout.orders.OrderNotification",
            "prod",
            "op-1",
            UpgradePhase::Started,
        );
        assert_eq!(first, second);
        assert!(first.starts_with("system-event/v1/"));
        assert_eq!(first.len(), "system-event/v1/".len() + 64);
    }

    #[test]
    fn idempotency_key_changes_with_any_scope_component() {
        let base = idempotency_key(
            "acme",
            "checkout.orders.OrderNotification",
            "prod",
            "op-1",
            UpgradePhase::Started,
        );
        let variants = [
            idempotency_key(
                "other-org",
                "checkout.orders.OrderNotification",
                "prod",
                "op-1",
                UpgradePhase::Started,
            ),
            idempotency_key(
                "acme",
                "checkout.orders.OtherEvent",
                "prod",
                "op-1",
                UpgradePhase::Started,
            ),
            idempotency_key(
                "acme",
                "checkout.orders.OrderNotification",
                "dev",
                "op-1",
                UpgradePhase::Started,
            ),
            idempotency_key(
                "acme",
                "checkout.orders.OrderNotification",
                "prod",
                "op-2",
                UpgradePhase::Started,
            ),
            idempotency_key(
                "acme",
                "checkout.orders.OrderNotification",
                "prod",
                "op-1",
                UpgradePhase::Terminal,
            ),
        ];
        for variant in variants {
            assert_ne!(base, variant);
        }
    }

    #[test]
    fn idempotency_key_separates_prefix_ambiguities() {
        let joined = idempotency_key("acmeorders", "x.y", "prod", "op", UpgradePhase::Started);
        let split = idempotency_key("acme", "orders.x.y", "prod", "op", UpgradePhase::Started);
        assert_ne!(joined, split);
    }
}
