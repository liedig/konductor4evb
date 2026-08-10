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
        ControlPlaneError::DomainNotFound { .. }
        | ControlPlaneError::RequestNotFound { .. }
        | ControlPlaneError::BindingNotFound { .. }
        | ControlPlaneError::EntitlementNotFound { .. }
        | ControlPlaneError::SchemaNotFound { .. } => error_response(
            404,
            ApiError {
                code: "ResourceNotFoundException".to_string(),
                message: err.to_string(),
                request_id: None,
                reason: None,
                field_list: None,
            },
        ),
        ControlPlaneError::DomainAlreadyExists { .. }
        | ControlPlaneError::BindingConflict { .. }
        | ControlPlaneError::EntitlementConflict { .. }
        | ControlPlaneError::SchemaAlreadyExists { .. } => error_response(
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

/// The IAM-authenticated caller's AWS account, as populated by API Gateway on
/// an `aws_proxy` integration (`requestContext.identity.accountId`).
pub fn caller_account_id(event: &Value) -> Option<String> {
    event
        .get("requestContext")
        .and_then(|rc| rc.get("identity"))
        .and_then(|i| i.get("accountId"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn cors_headers() -> HashMap<&'static str, &'static str> {
    let mut headers = HashMap::new();
    headers.insert("Content-Type", "application/json");
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ControlPlaneError;
    use serde_json::json;

    #[test]
    fn success_response_returns_status_and_body() {
        let body = json!({"id": "123"});
        let resp = success_response(200, &body);

        assert_eq!(resp["statusCode"], 200);
        assert_eq!(resp["headers"]["Content-Type"], "application/json");

        let parsed_body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(parsed_body["id"], "123");
    }

    #[test]
    fn success_response_accepts_different_status_codes() {
        let resp = success_response(201, &json!({}));
        assert_eq!(resp["statusCode"], 201);
    }

    #[test]
    fn error_mapping_domain_not_found_returns_404() {
        let err = ControlPlaneError::DomainNotFound {
            domain_name: "abc".to_string(),
        };
        let resp = from_control_plane_error(&err);
        assert_eq!(resp["statusCode"], 404);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "ResourceNotFoundException");
    }

    #[test]
    fn error_mapping_request_not_found_returns_404() {
        let err = ControlPlaneError::RequestNotFound {
            request_id: "abc".to_string(),
        };
        let resp = from_control_plane_error(&err);
        assert_eq!(resp["statusCode"], 404);
    }

    #[test]
    fn error_mapping_domain_already_exists_returns_409() {
        let err = ControlPlaneError::DomainAlreadyExists {
            domain_name: "billing".to_string(),
        };
        let resp = from_control_plane_error(&err);
        assert_eq!(resp["statusCode"], 409);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "ConflictException");
    }

    #[test]
    fn error_mapping_validation_exception_returns_400() {
        let err = validation_error("bad field");
        let resp = from_control_plane_error(&err);
        assert_eq!(resp["statusCode"], 400);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "ValidationException");
        assert_eq!(body["message"], "bad field");
    }

    #[test]
    fn error_mapping_access_denied_returns_403() {
        let err = ControlPlaneError::AccessDenied;
        let resp = from_control_plane_error(&err);
        assert_eq!(resp["statusCode"], 403);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "AccessDeniedException");
    }

    #[test]
    fn error_mapping_invalid_state_transition_returns_409() {
        let err = ControlPlaneError::InvalidStateTransition {
            current: "ACTIVE".to_string(),
            target: "APPROVED".to_string(),
        };
        let resp = from_control_plane_error(&err);
        assert_eq!(resp["statusCode"], 409);
    }

    #[test]
    fn error_mapping_schema_not_found_returns_404() {
        let err = ControlPlaneError::SchemaNotFound {
            schema_name: "acme-orders@OrderNotification".to_string(),
        };
        let resp = from_control_plane_error(&err);
        assert_eq!(resp["statusCode"], 404);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "ResourceNotFoundException");
    }

    #[test]
    fn error_mapping_schema_already_exists_returns_409() {
        let err = ControlPlaneError::SchemaAlreadyExists {
            schema_name: "acme-orders@OrderNotification".to_string(),
        };
        let resp = from_control_plane_error(&err);
        assert_eq!(resp["statusCode"], 409);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "ConflictException");
    }

    #[test]
    fn error_mapping_internal_error_returns_500() {
        let err = ControlPlaneError::DynamoDb("timeout".to_string());
        let resp = from_control_plane_error(&err);
        assert_eq!(resp["statusCode"], 500);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "InternalServerException");
        assert_eq!(body["message"], "An internal error occurred");
    }

    #[test]
    fn error_response_includes_request_id() {
        let err = ControlPlaneError::AccessDenied;
        let resp = from_control_plane_error(&err);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert!(body["requestId"].is_string());
        assert!(!body["requestId"].as_str().unwrap().is_empty());
    }

    #[test]
    fn parse_body_deserializes_valid_json() {
        let event = json!({"body": "{\"domainName\":\"billing\"}"});
        let result: Result<Value, _> = parse_body(&event);
        assert_eq!(result.unwrap()["domainName"], "billing");
    }

    #[test]
    fn parse_body_defaults_to_empty_object_when_body_missing() {
        let event = json!({});
        let result: Result<Value, _> = parse_body(&event);
        assert_eq!(result.unwrap(), json!({}));
    }

    #[test]
    fn parse_body_returns_error_for_malformed_json() {
        let event = json!({"body": "not json {"});
        let result: Result<Value, _> = parse_body(&event);
        assert!(result.is_err());
        match result.unwrap_err() {
            ControlPlaneError::ValidationException { reason, .. } => {
                assert_eq!(reason, crate::error::ValidationExceptionReason::CannotParse);
            }
            other => panic!("expected ValidationException, got: {other:?}"),
        }
    }

    #[test]
    fn path_param_extracts_value() {
        let event = json!({"pathParameters": {"domainName": "billing"}});
        assert_eq!(
            path_param(&event, "domainName"),
            Some("billing".to_string())
        );
    }

    #[test]
    fn path_param_returns_none_when_missing() {
        let event = json!({});
        assert_eq!(path_param(&event, "domainName"), None);
    }

    #[test]
    fn path_param_returns_none_for_unknown_param() {
        let event = json!({"pathParameters": {"other": "val"}});
        assert_eq!(path_param(&event, "domainName"), None);
    }

    #[test]
    fn query_param_extracts_value() {
        let event = json!({"queryStringParameters": {"status": "ACTIVE"}});
        assert_eq!(query_param(&event, "status"), Some("ACTIVE".to_string()));
    }

    #[test]
    fn query_param_returns_none_when_missing() {
        let event = json!({});
        assert_eq!(query_param(&event, "status"), None);
    }

    #[test]
    fn query_param_returns_none_for_unknown_param() {
        let event = json!({"queryStringParameters": {"other": "val"}});
        assert_eq!(query_param(&event, "status"), None);
    }
}
