//! The admitted publish command's queue envelope.
//!
//! Two formats share the publish queue during the dual-format rollout:
//!
//! - **Legacy (format 1, implicit):** `{callerAccountId, body}` with the
//!   routing identity carried in SQS message attributes. Messages already
//!   in flight at upgrade time keep this shape forever.
//! - **Versioned (format 2):** an explicit `format: 2` field plus everything
//!   admission resolved and froze — business/schema identity, target,
//!   metadata revision/digest, the generated extension values, and the
//!   compiled metadata schema delivery validates the frozen fields against.
//!   Metadata lives in the body, not in an unbounded attribute set.
//!
//! Parsing is dual-format and fails closed: only an actual legacy wrapper
//! (no `format` field) takes the legacy path; anything else must be exactly
//! format 2 with every required field present. A message that is missing or
//! internally inconsistent is reported for SQS retry, never repaired — no
//! consumer path generates missing values or reads current metadata
//! configuration.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ControlPlaneError, ValidationExceptionField, ValidationExceptionReason};
use crate::models::PublishRequest;

/// The explicit format marker on versioned messages.
pub const QUEUE_FORMAT_VERSIONED: u8 = 2;

/// The frozen metadata identity and generated extension values for one
/// admitted event, recorded at admission and reused verbatim across every
/// delivery retry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenMetadata {
    /// The immutable metadata revision the values were generated under.
    pub metadata_revision: u32,
    /// The revision's definition digest, for audit and identity checks.
    pub definition_digest: String,
    /// The environment pointer generation at admission, scoping caches.
    pub generation: u64,
    /// The generated extension fields, exactly as they will be stamped into
    /// `detail.metadata`. Reserved built-ins are rejected here.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub values: serde_json::Map<String, Value>,
}

/// The admission-resolved identity carried in a format-2 message: what was
/// validated, where it goes, and which metadata contract stamped it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionIdentity {
    pub environment: String,
    pub schema_name: String,
    pub event_bus_name: String,
    pub schema_version: i64,
    pub idempotency_key: String,
    pub metadata: FrozenMetadata,
    /// The revision's compiled metadata schema (draft-4 JSON document).
    /// Delivery validates the frozen fields and the late-bound timestamp
    /// against it without any configuration read. It is never published to
    /// EventBridge.
    pub metadata_schema: String,
}

/// The format-2 queue envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VersionedAdmission {
    pub format: u8,
    pub caller_account_id: String,
    pub body: PublishRequest,
    pub admission: AdmissionIdentity,
}

/// The legacy queue envelope: the publisher body plus the caller account;
/// routing identity travels in the SQS message attributes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyAdmission {
    pub caller_account_id: String,
    pub body: PublishRequest,
}

/// One admitted publish command, in whichever queue format it arrived.
/// The versioned envelope is boxed: it dominates the enum's size and only
/// ever exists once per message.
#[derive(Debug, Clone, PartialEq)]
pub enum AdmittedCommand {
    Legacy(LegacyAdmission),
    Versioned(Box<VersionedAdmission>),
}

impl AdmittedCommand {
    /// Parses an SQS publish-queue message body, dispatching on the explicit
    /// `format` field. Absent `format` is the legacy wrapper — the only path
    /// to the old three-field output; `format: 2` is the versioned envelope;
    /// every other shape fails closed.
    pub fn parse(body: &str) -> Result<Self, ControlPlaneError> {
        let value: Value = serde_json::from_str(body)
            .map_err(|e| queue_parse_error(format!("queue message is not valid JSON: {e}")))?;
        match value.get("format") {
            None => {
                let legacy: LegacyAdmission = serde_json::from_value(value)
                    .map_err(|e| queue_parse_error(format!("legacy queue message is malformed: {e}")))?;
                Ok(AdmittedCommand::Legacy(legacy))
            }
            Some(format) if format.as_u64() == Some(QUEUE_FORMAT_VERSIONED as u64) => {
                let versioned: VersionedAdmission = serde_json::from_value(value).map_err(|e| {
                    queue_parse_error(format!(
                        "format-2 queue message is missing or has inconsistent fields: {e}"
                    ))
                })?;
                versioned.validate()?;
                Ok(AdmittedCommand::Versioned(Box::new(versioned)))
            }
            Some(other) => Err(queue_parse_error(format!(
                "unsupported queue format {other}; only the legacy wrapper and format 2 are accepted"
            ))),
        }
    }
}

impl VersionedAdmission {
    /// Format-level integrity beyond serde's shape checks: the metadata
    /// revision must be real and the frozen values must not shadow reserved
    /// built-ins.
    fn validate(&self) -> Result<(), ControlPlaneError> {
        if self.admission.metadata.metadata_revision == 0 {
            return Err(queue_parse_error(
                "metadata revision must be a positive number".to_string(),
            ));
        }
        for reserved in crate::metadata::definition::RESERVED_ROOT_FIELDS {
            if self.admission.metadata.values.contains_key(reserved) {
                return Err(queue_parse_error(format!(
                    "frozen metadata value '{reserved}' shadows a reserved built-in"
                )));
            }
        }
        if self.admission.metadata_schema.trim().is_empty() {
            return Err(queue_parse_error(
                "format-2 messages must carry the compiled metadata schema".to_string(),
            ));
        }
        Ok(())
    }
}

fn queue_parse_error(message: String) -> ControlPlaneError {
    ControlPlaneError::ValidationException {
        message: message.clone(),
        reason: ValidationExceptionReason::FieldValidationFailed,
        field_list: Some(vec![ValidationExceptionField {
            name: "body".to_string(),
            message,
        }]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versioned_body() -> Value {
        json_body(QUEUE_FORMAT_VERSIONED)
    }

    fn json_body(format: u8) -> Value {
        serde_json::json!({
            "format": format,
            "callerAccountId": "111122223333",
            "body": {"domain": "orders", "eventName": "OrderNotification", "data": {"orderId": 1}},
            "admission": {
                "environment": "prod",
                "schemaName": "acme-orders@OrderNotification",
                "eventBusName": "acme-prod-orders-event-bus",
                "schemaVersion": 4,
                "idempotencyKey": "key-1",
                "metadata": {
                    "metadataRevision": 2,
                    "definitionDigest": "digest",
                    "generation": 1,
                    "values": {"traceId": "trace-abc"}
                },
                "metadataSchema": "{\"type\": \"object\"}"
            }
        })
    }

    fn legacy_body() -> String {
        serde_json::json!({
            "callerAccountId": "111122223333",
            "body": {"domain": "orders", "eventName": "OrderNotification", "data": {"orderId": 1}}
        })
        .to_string()
    }

    #[test]
    fn a_legacy_wrapper_parses_as_legacy() {
        match AdmittedCommand::parse(&legacy_body()).unwrap() {
            AdmittedCommand::Legacy(legacy) => {
                assert_eq!(legacy.caller_account_id, "111122223333");
                assert_eq!(legacy.body.domain, "orders");
            }
            other => panic!("legacy wrapper parsed as {other:?}"),
        }
    }

    #[test]
    fn a_format_two_message_parses_with_every_frozen_field() {
        match AdmittedCommand::parse(&versioned_body().to_string()).unwrap() {
            AdmittedCommand::Versioned(versioned) => {
                assert_eq!(versioned.format, QUEUE_FORMAT_VERSIONED);
                assert_eq!(versioned.admission.schema_version, 4);
                assert_eq!(versioned.admission.metadata.metadata_revision, 2);
                assert_eq!(
                    versioned.admission.metadata.values.get("traceId"),
                    Some(&serde_json::json!("trace-abc"))
                );
            }
            other => panic!("versioned message parsed as {other:?}"),
        }
    }

    #[test]
    fn an_explicit_unsupported_format_fails_closed() {
        for bad in [json_body(1), json_body(3)] {
            let result = AdmittedCommand::parse(&bad.to_string());
            assert!(result.is_err(), "format {} must fail", bad["format"]);
            assert!(
                result.unwrap_err().to_string().contains("unsupported"),
                "wrong error"
            );
        }
    }

    #[test]
    fn a_missing_admission_block_fails_closed() {
        let mut body = versioned_body();
        body.as_object_mut().unwrap().remove("admission");
        assert!(AdmittedCommand::parse(&body.to_string()).is_err());
    }

    #[test]
    fn a_missing_metadata_revision_fails_closed() {
        let mut body = versioned_body();
        body["admission"]["metadata"]
            .as_object_mut()
            .unwrap()
            .remove("metadataRevision");
        assert!(AdmittedCommand::parse(&body.to_string()).is_err());
    }

    #[test]
    fn reserved_shadows_in_frozen_values_fail_closed() {
        let mut body = versioned_body();
        body["admission"]["metadata"]["values"] = serde_json::json!({"schemaVersion": 99});
        let err = AdmittedCommand::parse(&body.to_string())
            .unwrap_err()
            .to_string();
        assert!(err.contains("reserved"), "unexpected error: {err}");
    }

    #[test]
    fn an_empty_carried_schema_fails_closed() {
        let mut body = versioned_body();
        body["admission"]["metadataSchema"] = serde_json::json!("  ");
        let err = AdmittedCommand::parse(&body.to_string())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("compiled metadata schema"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn an_unknown_field_fails_closed() {
        let mut body = versioned_body();
        body["admission"]["unexpected"] = serde_json::json!(true);
        assert!(AdmittedCommand::parse(&body.to_string()).is_err());
    }

    #[test]
    fn malformed_json_fails_closed() {
        assert!(AdmittedCommand::parse("not json").is_err());
    }
}
