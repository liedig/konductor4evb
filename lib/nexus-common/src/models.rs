use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{ValidationExceptionField, ValidationExceptionReason};

fn deserialize_non_empty_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let vec = Vec::<String>::deserialize(deserializer)?;
    if vec.is_empty() {
        return Err(serde::de::Error::custom(
            "secondaryOwnerEmails must contain at least one email address",
        ));
    }
    Ok(vec)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DomainStatus {
    PendingApproval,
    Approved,
    Rejected,
    ApprovalNotificationFailed,
    Provisioning,
    Active,
    ProvisioningFailed,
    Inactive,
}

impl DomainStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PendingApproval => "PENDING_APPROVAL",
            Self::Approved => "APPROVED",
            Self::Rejected => "REJECTED",
            Self::ApprovalNotificationFailed => "APPROVAL_NOTIFICATION_FAILED",
            Self::Provisioning => "PROVISIONING",
            Self::Active => "ACTIVE",
            Self::ProvisioningFailed => "PROVISIONING_FAILED",
            Self::Inactive => "INACTIVE",
        }
    }
}

impl std::fmt::Display for DomainStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for DomainStatus {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "PENDING_APPROVAL" => Ok(Self::PendingApproval),
            "APPROVED" => Ok(Self::Approved),
            "REJECTED" => Ok(Self::Rejected),
            "APPROVAL_NOTIFICATION_FAILED" => Ok(Self::ApprovalNotificationFailed),
            "PROVISIONING" => Ok(Self::Provisioning),
            "ACTIVE" => Ok(Self::Active),
            "PROVISIONING_FAILED" => Ok(Self::ProvisioningFailed),
            "INACTIVE" => Ok(Self::Inactive),
            _ => Err(()),
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
    pub domain_name: String,
    pub description: Option<String>,
    pub owner_account_id: String,
    pub owner_email: String,
    pub secondary_owner_emails: Option<Vec<String>>,
    pub status: DomainStatus,
    pub environments: Vec<String>,
    pub tags: Option<std::collections::HashMap<String, String>>,
    pub resource_arns: Option<std::collections::HashMap<String, PerEnvResourceArns>>,
    pub namespace_parameter: Option<String>,
    pub ram_share_arn: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub task_token: Option<String>,
    pub decided_by: Option<String>,
    pub justification: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
    pub rejected_at: Option<DateTime<Utc>>,
    pub error_detail: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PerEnvResourceArns {
    pub platform_event_bus_arn: Option<String>,
    pub app_event_bus_arn: Option<String>,
    pub schema_registry_arn: Option<String>,
    pub subscription_dlq_arn: Option<String>,
    pub integration_dlq_arn: Option<String>,
    pub dashboard_arn: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct RegisterDomainRequest {
    pub domain_name: String,
    pub description: Option<String>,
    pub owner_account_id: String,
    pub owner_email: String,
    #[serde(deserialize_with = "deserialize_non_empty_vec")]
    pub secondary_owner_emails: Vec<String>,
    pub tags: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct DecideRequest {
    pub task_token: String,
    pub decision: Decision,
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
            (
                DomainStatus::ApprovalNotificationFailed,
                "\"APPROVAL_NOTIFICATION_FAILED\"",
            ),
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
            (
                "\"APPROVAL_NOTIFICATION_FAILED\"",
                DomainStatus::ApprovalNotificationFailed,
            ),
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
        assert_eq!(
            DomainStatus::ApprovalNotificationFailed.to_string(),
            "APPROVAL_NOTIFICATION_FAILED"
        );
        assert_eq!(DomainStatus::Provisioning.to_string(), "PROVISIONING");
        assert_eq!(DomainStatus::Active.to_string(), "ACTIVE");
        assert_eq!(
            DomainStatus::ProvisioningFailed.to_string(),
            "PROVISIONING_FAILED"
        );
        assert_eq!(DomainStatus::Inactive.to_string(), "INACTIVE");
    }

    #[test]
    fn domain_status_from_str_matches_wire_format() {
        assert_eq!(
            "PENDING_APPROVAL".parse::<DomainStatus>(),
            Ok(DomainStatus::PendingApproval)
        );
        assert_eq!(
            "APPROVED".parse::<DomainStatus>(),
            Ok(DomainStatus::Approved)
        );
        assert_eq!(
            "REJECTED".parse::<DomainStatus>(),
            Ok(DomainStatus::Rejected)
        );
        assert_eq!(
            "APPROVAL_NOTIFICATION_FAILED".parse::<DomainStatus>(),
            Ok(DomainStatus::ApprovalNotificationFailed)
        );
        assert_eq!(
            "PROVISIONING".parse::<DomainStatus>(),
            Ok(DomainStatus::Provisioning)
        );
        assert_eq!("ACTIVE".parse::<DomainStatus>(), Ok(DomainStatus::Active));
        assert_eq!(
            "PROVISIONING_FAILED".parse::<DomainStatus>(),
            Ok(DomainStatus::ProvisioningFailed)
        );
        assert_eq!(
            "INACTIVE".parse::<DomainStatus>(),
            Ok(DomainStatus::Inactive)
        );
    }

    #[test]
    fn domain_status_from_str_rejects_invalid_values() {
        assert!("DEACTIVATED".parse::<DomainStatus>().is_err());
        assert!("".parse::<DomainStatus>().is_err());
    }

    #[test]
    fn register_domain_request_rejects_unknown_fields() {
        let value = serde_json::json!({
            "domainName": "billing",
            "ownerAccountId": "111122223333",
            "ownerEmail": "owner@example.com",
            "secondaryOwnerEmails": ["backup@example.com"],
            "unexpected": "value"
        });
        let parsed: Result<RegisterDomainRequest, _> = serde_json::from_value(value);
        assert!(parsed.is_err());
    }

    #[test]
    fn register_domain_request_rejects_empty_secondary_owner_emails() {
        let value = serde_json::json!({
            "domainName": "billing",
            "ownerAccountId": "111122223333",
            "ownerEmail": "owner@example.com",
            "secondaryOwnerEmails": []
        });
        let parsed: Result<RegisterDomainRequest, _> = serde_json::from_value(value);
        assert!(parsed.is_err());
    }

    #[test]
    fn register_domain_request_rejects_missing_secondary_owner_emails() {
        let value = serde_json::json!({
            "domainName": "billing",
            "ownerAccountId": "111122223333",
            "ownerEmail": "owner@example.com"
        });
        let parsed: Result<RegisterDomainRequest, _> = serde_json::from_value(value);
        assert!(parsed.is_err());
    }

    #[test]
    fn decide_request_rejects_unknown_fields() {
        let value = serde_json::json!({
            "taskToken": "token",
            "decision": "ACCEPT",
            "decidedBy": "admin@example.com",
            "unexpected": "value"
        });
        let parsed: Result<DecideRequest, _> = serde_json::from_value(value);
        assert!(parsed.is_err());
    }

    #[test]
    fn decide_request_parses_without_decided_by() {
        let value = serde_json::json!({
            "taskToken": "token",
            "decision": "ACCEPT",
        });
        let parsed: DecideRequest = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.task_token, "token");
        assert_eq!(parsed.decision, Decision::Accept);
    }

    #[test]
    fn decide_request_rejects_caller_supplied_decided_by() {
        let value = serde_json::json!({
            "taskToken": "token",
            "decision": "ACCEPT",
            "decidedBy": "attacker",
        });
        let parsed: Result<DecideRequest, _> = serde_json::from_value(value);
        assert!(parsed.is_err());
    }
}
