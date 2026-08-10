use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ValidationExceptionReason {
    UnknownOperation,
    CannotParse,
    FieldValidationFailed,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationExceptionField {
    pub name: String,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum ControlPlaneError {
    #[error("domain not found: {domain_name}")]
    DomainNotFound { domain_name: String },

    #[error("domain already exists: {domain_name}")]
    DomainAlreadyExists { domain_name: String },

    #[error("{message}")]
    ValidationException {
        message: String,
        reason: ValidationExceptionReason,
        field_list: Option<Vec<ValidationExceptionField>>,
    },

    #[error("access denied")]
    AccessDenied,

    #[error("registration request not found: {request_id}")]
    RequestNotFound { request_id: String },

    #[error("binding not found: {binding_id}")]
    BindingNotFound { binding_id: String },

    #[error("binding conflict: {reason}")]
    BindingConflict { reason: String },
    #[error("entitlement not found: {entitlement_id}")]
    EntitlementNotFound { entitlement_id: String },
    #[error("entitlement conflict: {reason}")]
    EntitlementConflict { reason: String },

    #[error("schema not found: {schema_name}")]
    SchemaNotFound { schema_name: String },

    #[error("schema already exists: {schema_name}")]
    SchemaAlreadyExists { schema_name: String },

    #[error("invalid state transition from {current} to {target}")]
    InvalidStateTransition { current: String, target: String },

    #[error("DynamoDB error: {0}")]
    DynamoDb(String),

    #[error("EventBridge error: {0}")]
    EventBridge(String),

    #[error("Step Functions error: {0}")]
    StepFunctions(String),

    #[error("Schemas error: {0}")]
    Schemas(String),
}
