use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{ValidationExceptionField, ValidationExceptionReason};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DomainStatus {
    PendingApproval,
    Approved,
    Creating,
    Active,
    Suspended,
    Failed,
    Rejected,
    Deactivated,
}

impl std::fmt::Display for DomainStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PendingApproval => write!(f, "PENDING_APPROVAL"),
            Self::Approved => write!(f, "APPROVED"),
            Self::Creating => write!(f, "CREATING"),
            Self::Active => write!(f, "ACTIVE"),
            Self::Suspended => write!(f, "SUSPENDED"),
            Self::Failed => write!(f, "FAILED"),
            Self::Rejected => write!(f, "REJECTED"),
            Self::Deactivated => write!(f, "DEACTIVATED"),
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
    pub status: DomainStatus,
    pub tags: Option<std::collections::HashMap<String, String>>,
    pub resource_arns: Option<DomainResourceArns>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: String,
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
pub struct RegistrationRequest {
    pub request_id: String,
    pub domain_id: String,
    pub domain_name: String,
    pub description: Option<String>,
    pub owner_account_id: String,
    pub status: DomainStatus,
    pub task_token: Option<String>,
    pub workflow_execution_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub decided_by: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub tags: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterDomainRequest {
    pub domain_name: String,
    pub description: Option<String>,
    pub owner_account_id: String,
    pub tags: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRequest {
    pub decision: Decision,
    pub reason: Option<String>,
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
