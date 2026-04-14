use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

use crate::error::{ControlPlaneError, ValidationExceptionReason};
use crate::models::ApiError;

pub fn success_response(status_code: u16, body: &impl Serialize) -> Value {
    json!({
        "statusCode": status_code,
        "headers": cors_headers(),
        "body": serde_json::to_string(body).unwrap_or_default()
    })
}

pub fn error_response(status_code: u16, mut error: ApiError) -> Value {
    if error.request_id.is_none() {
        error.request_id = Some(Uuid::new_v4().to_string());
    }
    json!({
        "statusCode": status_code,
        "headers": cors_headers(),
        "body": serde_json::to_string(&error).unwrap_or_default()
    })
}

pub fn from_control_plane_error(err: &ControlPlaneError) -> Value {
    match err {
        ControlPlaneError::DomainNotFound { .. } | ControlPlaneError::RequestNotFound { .. } => {
            error_response(
                404,
                ApiError {
                    code: "ResourceNotFoundException".to_string(),
                    message: err.to_string(),
                    request_id: None,
                    reason: None,
                    field_list: None,
                },
            )
        }
        ControlPlaneError::DomainAlreadyExists { .. } => error_response(
            409,
            ApiError {
                code: "ConflictException".to_string(),
                message: err.to_string(),
                request_id: None,
                reason: None,
                field_list: None,
            },
        ),
        ControlPlaneError::ValidationException {
            message,
            reason,
            field_list,
        } => error_response(
            400,
            ApiError {
                code: "ValidationException".to_string(),
                message: message.clone(),
                request_id: None,
                reason: Some(reason.clone()),
                field_list: field_list.clone(),
            },
        ),
        ControlPlaneError::AccessDenied => error_response(
            403,
            ApiError {
                code: "AccessDeniedException".to_string(),
                message: "Access denied".to_string(),
                request_id: None,
                reason: None,
                field_list: None,
            },
        ),
        ControlPlaneError::InvalidStateTransition { .. } => error_response(
            409,
            ApiError {
                code: "ConflictException".to_string(),
                message: err.to_string(),
                request_id: None,
                reason: None,
                field_list: None,
            },
        ),
        _ => error_response(
            500,
            ApiError {
                code: "InternalServerException".to_string(),
                message: "An internal error occurred".to_string(),
                request_id: None,
                reason: None,
                field_list: None,
            },
        ),
    }
}

pub fn validation_error(message: impl Into<String>) -> ControlPlaneError {
    ControlPlaneError::ValidationException {
        message: message.into(),
        reason: ValidationExceptionReason::FieldValidationFailed,
        field_list: None,
    }
}

pub fn parse_error(message: impl Into<String>) -> ControlPlaneError {
    ControlPlaneError::ValidationException {
        message: message.into(),
        reason: ValidationExceptionReason::CannotParse,
        field_list: None,
    }
}

pub fn parse_body<T: serde::de::DeserializeOwned>(event: &Value) -> Result<T, ControlPlaneError> {
    let body_str = event.get("body").and_then(|b| b.as_str()).unwrap_or("{}");

    serde_json::from_str(body_str).map_err(|e| parse_error(format!("Invalid request body: {e}")))
}

pub fn path_param(event: &Value, name: &str) -> Option<String> {
    event
        .get("pathParameters")
        .and_then(|p| p.get(name))
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub fn query_param(event: &Value, name: &str) -> Option<String> {
    event
        .get("queryStringParameters")
        .and_then(|p| p.get(name))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn cors_headers() -> HashMap<&'static str, &'static str> {
    let mut headers = HashMap::new();
    headers.insert("Content-Type", "application/json");
    headers
}
