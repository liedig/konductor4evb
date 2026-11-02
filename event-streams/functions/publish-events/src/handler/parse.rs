//! Queue message parsing: a raw SQS record becomes a [`ParsedMessage`]
//! carrying the SQS routing fields delivery needs (message id, receipt
//! handle, receive count, sent timestamp) alongside the admission-stamped
//! command.
//!
//! Dual-format queue: messages without a `format` field are the legacy
//! wrapper (identity in SQS attributes, three-field metadata output);
//! `format: 2` messages carry the admission-resolved identity, frozen
//! metadata values, and the compiled metadata schema in the body.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;

use konductor_common::envelope::published_at_from_sent_timestamp;
use konductor_common::models::PublishRequest;
use konductor_common::principal_context::sqs_attribute_map;
use konductor_common::publish_queue::{AdmittedCommand, VersionedAdmission};

/// The legacy delivery: identity travels in SQS attributes, metadata
/// output is the original three built-in fields.
pub(crate) struct LegacyDelivery {
    pub(crate) environment: String,
    pub(crate) idempotency_key: String,
    pub(crate) schema_version: i64,
    pub(crate) request: PublishRequest,
}

/// The command half of a parsed message, in either queue format.
pub(crate) enum ParsedRecord {
    Legacy(LegacyDelivery),
    Versioned { command: Box<VersionedAdmission> },
}

/// A queue message with its SQS routing fields and parsed command.
pub(crate) struct ParsedMessage {
    pub(crate) message_id: String,
    pub(crate) receipt_handle: Option<String>,
    pub(crate) receive_count: u32,
    pub(crate) sent_timestamp_ms: i64,
    pub(crate) published_at: DateTime<Utc>,
    pub(crate) record: ParsedRecord,
}

pub(crate) fn parse_message(record: &Value) -> Result<ParsedMessage> {
    let message_id = record
        .get("messageId")
        .and_then(|v| v.as_str())
        .context("missing messageId")?
        .to_string();
    let receipt_handle = record
        .get("receiptHandle")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let attributes_block = record.get("attributes");
    let sent_timestamp = attributes_block
        .and_then(|a| a.get("SentTimestamp"))
        .and_then(|v| v.as_str())
        .context("missing SQS SentTimestamp attribute")?;
    let published_at = published_at_from_sent_timestamp(sent_timestamp)?;
    let receive_count = attributes_block
        .and_then(|a| a.get("ApproximateReceiveCount"))
        .and_then(|v| v.as_str())
        .map(str::parse::<u32>)
        .transpose()
        .context("ApproximateReceiveCount attribute is not a number")?
        .unwrap_or(1);

    let attributes = sqs_attribute_map(record);
    let body_str = record
        .get("body")
        .and_then(|b| b.as_str())
        .context("missing SQS record body")?;
    let parsed = match AdmittedCommand::parse(body_str) {
        Ok(AdmittedCommand::Legacy(message)) => ParsedRecord::Legacy(LegacyDelivery {
            environment: required_attribute(&attributes, "Environment")?,
            idempotency_key: required_attribute(&attributes, "IdempotencyKey")?,
            schema_version: required_attribute(&attributes, "SchemaVersion")?
                .parse::<i64>()
                .context("SchemaVersion attribute is not a number")?,
            request: message.body,
        }),
        Ok(AdmittedCommand::Versioned(command)) => {
            check_immutable_consistency(&attributes, &command)?;
            ParsedRecord::Versioned { command }
        }
        Err(e) => return Err(anyhow::anyhow!(e.to_string())),
    };

    Ok(ParsedMessage {
        message_id,
        receipt_handle,
        receive_count,
        sent_timestamp_ms: published_at.timestamp_millis(),
        published_at,
        record: parsed,
    })
}

/// The SQS routing attributes are stamped at admission alongside the body.
/// A format-2 message whose attributes disagree with its recorded identity
/// fails closed: the message is internally inconsistent and no path may
/// guess which half is right.
fn check_immutable_consistency(
    attributes: &std::collections::HashMap<String, String>,
    command: &VersionedAdmission,
) -> Result<()> {
    let pairs = [
        ("Environment", command.admission.environment.clone()),
        ("IdempotencyKey", command.admission.idempotency_key.clone()),
        (
            "SchemaVersion",
            command.admission.schema_version.to_string(),
        ),
    ];
    for (name, expected) in pairs {
        let actual = required_attribute(attributes, name)?;
        if actual != expected {
            anyhow::bail!(
                "message attribute {name} '{actual}' disagrees with the recorded admission value '{expected}'"
            );
        }
    }
    Ok(())
}

fn required_attribute(
    attributes: &std::collections::HashMap<String, String>,
    name: &'static str,
) -> Result<String> {
    attributes
        .get(name)
        .filter(|v| !v.is_empty())
        .cloned()
        .with_context(|| format!("missing message attribute {name}"))
}
