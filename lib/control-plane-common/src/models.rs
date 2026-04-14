use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{ValidationExceptionField, ValidationExceptionReason};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DomainStatus {
    PendingApproval,
    Approved,
    Rejected,
    Provisioning,
    Active,
    ProvisioningFailed,
    Inactive,
}

impl std::fmt::Display for DomainStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PendingApproval => write!(f, "PENDING_APPROVAL"),
            Self::Approved => write!(f, "APPROVED"),
            Self::Rejected => write!(f, "REJECTED"),
            Self::Provisioning => write!(f, "PROVISIONING"),
            Self::Active => write!(f, "ACTIVE"),
            Self::ProvisioningFailed => write!(f, "PROVISIONING_FAILED"),
            Self::Inactive => write!(f, "INACTIVE"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Decision {
    Accept,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Domain {
    pub domain_id: String,
    pub domain_name: String,
    pub description: Option<String>,
    pub owner_account_id: String,
    pub owner_email: String,
    pub status: DomainStatus,
    pub tags: Option<std::collections::HashMap<String, String>>,
    pub resource_arns: Option<DomainResourceArns>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub task_token: Option<String>,
    pub decided_by: Option<String>,
    pub justification: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
    pub rejected_at: Option<DateTime<Utc>>,
    pub error_detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainResourceArns {
    pub platform_event_bus_arn: Option<String>,
    pub app_event_bus_arn: Option<String>,
    pub schema_registry_arn: Option<String>,
    pub subscription_dlq_arn: Option<String>,
    pub integration_dlq_arn: Option<String>,
    pub dashboard_arn: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterDomainRequest {
    pub domain_name: String,
    pub description: Option<String>,
    pub owner_account_id: String,
    pub owner_email: String,
    pub tags: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecideRequest {
    pub task_token: String,
    pub decision: Decision,
    pub decided_by: String,
    pub justification: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse<T: Serialize> {
    pub status_code: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<ValidationExceptionReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_list: Option<Vec<ValidationExceptionField>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedResponse<T: Serialize> {
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_status_serializes_to_screaming_snake_case() {
        let cases = vec![
            (DomainStatus::PendingApproval, "\"PENDING_APPROVAL\""),
            (DomainStatus::Approved, "\"APPROVED\""),
            (DomainStatus::Rejected, "\"REJECTED\""),
            (DomainStatus::Provisioning, "\"PROVISIONING\""),
            (DomainStatus::Active, "\"ACTIVE\""),
            (DomainStatus::ProvisioningFailed, "\"PROVISIONING_FAILED\""),
            (DomainStatus::Inactive, "\"INACTIVE\""),
        ];
        for (status, expected) in cases {
            assert_eq!(serde_json::to_string(&status).unwrap(), expected);
        }
    }

    #[test]
    fn domain_status_deserializes_from_screaming_snake_case() {
        let cases = vec![
            ("\"PENDING_APPROVAL\"", DomainStatus::PendingApproval),
            ("\"APPROVED\"", DomainStatus::Approved),
            ("\"REJECTED\"", DomainStatus::Rejected),
            ("\"PROVISIONING\"", DomainStatus::Provisioning),
            ("\"ACTIVE\"", DomainStatus::Active),
            ("\"PROVISIONING_FAILED\"", DomainStatus::ProvisioningFailed),
            ("\"INACTIVE\"", DomainStatus::Inactive),
        ];
        for (input, expected) in cases {
            let result: DomainStatus = serde_json::from_str(input).unwrap();
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn domain_status_display_matches_wire_format() {
        assert_eq!(
            DomainStatus::PendingApproval.to_string(),
            "PENDING_APPROVAL"
        );
        assert_eq!(DomainStatus::Approved.to_string(), "APPROVED");
        assert_eq!(DomainStatus::Rejected.to_string(), "REJECTED");
        assert_eq!(DomainStatus::Provisioning.to_string(), "PROVISIONING");
        assert_eq!(DomainStatus::Active.to_string(), "ACTIVE");
        assert_eq!(
            DomainStatus::ProvisioningFailed.to_string(),
            "PROVISIONING_FAILED"
        );
        assert_eq!(DomainStatus::Inactive.to_string(), "INACTIVE");
    }
}
