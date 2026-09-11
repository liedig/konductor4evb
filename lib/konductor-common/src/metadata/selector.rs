use serde::{Deserialize, Serialize};

use crate::envelope::SchemaName;
use crate::error::{ControlPlaneError, ValidationExceptionField, ValidationExceptionReason};

/// Numeric metadata revision identity. Revision 0 is the read-only legacy
/// selector for events predating metadata stamping; real revisions are
/// positive integers with revision 1 as the protected-defaults baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MetadataRevision(u32);

impl MetadataRevision {
    /// The legacy selector: events and queued messages without a metadata
    /// stamp are interpreted with the legacy built-in contract.
    pub const LEGACY: Self = Self(0);

    /// Creates a real (non-legacy) revision identity.
    pub fn new(value: u32) -> Result<Self, ControlPlaneError> {
        if value == 0 {
            return Err(ControlPlaneError::ValidationException {
                message: "metadata revision must be a positive number".to_string(),
                reason: ValidationExceptionReason::FieldValidationFailed,
                field_list: Some(vec![ValidationExceptionField {
                    name: "metadataRevision".to_string(),
                    message: "must be a positive number; 0 is the reserved legacy selector"
                        .to_string(),
                }]),
            });
        }
        Ok(Self(value))
    }

    pub const fn legacy() -> Self {
        Self(0)
    }

    pub const fn is_legacy(self) -> bool {
        self.0 == 0
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for MetadataRevision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The exact identity of an event contract: organisation, environment,
/// canonical schema name, business schema version, and metadata revision.
/// `business_version` keeps its subscription-routing meaning; the metadata
/// revision is an independently versioned component of the same contract.
#[derive(Debug, Clone, PartialEq)]
pub struct ContractSelector {
    pub organisation: String,
    pub environment: String,
    pub schema_name: SchemaName,
    pub business_version: i64,
    pub metadata_revision: MetadataRevision,
}

impl ContractSelector {
    /// The legacy selector for a historical business version predating
    /// metadata stamping. Explicit, never silently substituted for an
    /// unknown revision.
    pub fn legacy(
        organisation: String,
        environment: String,
        schema_name: SchemaName,
        business_version: i64,
    ) -> Self {
        Self {
            organisation,
            environment,
            schema_name,
            business_version,
            metadata_revision: MetadataRevision::LEGACY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_revision_is_zero_and_read_only() {
        assert_eq!(MetadataRevision::LEGACY.as_u32(), 0);
        assert!(MetadataRevision::LEGACY.is_legacy());
        assert!(MetadataRevision::legacy().is_legacy());
    }

    #[test]
    fn revision_zero_cannot_be_minted() {
        assert!(MetadataRevision::new(0).is_err());
        assert!(MetadataRevision::new(1).is_ok());
        assert!(!MetadataRevision::new(1).unwrap().is_legacy());
    }

    #[test]
    fn revisions_order_numerically() {
        let m1 = MetadataRevision::new(1).unwrap();
        let m2 = MetadataRevision::new(2).unwrap();
        assert!(m1 < m2);
        assert!(MetadataRevision::LEGACY < m1);
    }

    #[test]
    fn revision_serializes_as_a_transparent_number() {
        let value = serde_json::to_value(MetadataRevision::new(7).unwrap()).unwrap();
        assert_eq!(value, serde_json::json!(7));
        let parsed: MetadataRevision = serde_json::from_value(serde_json::json!(7)).unwrap();
        assert_eq!(parsed.as_u32(), 7);
    }

    #[test]
    fn contract_selector_combines_independent_versions() {
        let schema = SchemaName::parse("acme-orders@OrderPlaced").unwrap();
        let selector = ContractSelector {
            organisation: "acme".to_string(),
            environment: "staging".to_string(),
            schema_name: schema.clone(),
            business_version: 3,
            metadata_revision: MetadataRevision::new(2).unwrap(),
        };
        assert_eq!(selector.business_version, 3);
        assert_eq!(selector.metadata_revision.as_u32(), 2);

        let legacy = ContractSelector::legacy("acme".to_string(), "staging".to_string(), schema, 2);
        assert!(legacy.metadata_revision.is_legacy());
        assert_eq!(legacy.business_version, 2);
    }
}
