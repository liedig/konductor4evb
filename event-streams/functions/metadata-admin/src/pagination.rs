//! Opaque continuation tokens for the metadata administration list routes.
//! Tokens carry only DynamoDB exclusive-start keys — cursors, never
//! authorization state — and are validated against the partition they
//! claim to continue.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use base64::Engine;

use konductor_common::api;
use konductor_common::dynamodb::{get_s, s};
use konductor_common::error::ControlPlaneError;
use konductor_common::metadata::store::REVISION_PARTITION;

/// Encodes a last-evaluated key as an opaque continuation token.
pub fn encode_cursor(key: &HashMap<String, AttributeValue>) -> Option<String> {
    let pk = get_s(key, "PK")?;
    let sk = get_s(key, "SK")?;
    let payload = serde_json::json!({ "pk": pk, "sk": sk });
    Some(base64::engine::general_purpose::URL_SAFE.encode(payload.to_string()))
}

/// Decodes a continuation token into a revision-history exclusive-start
/// key. Tokens naming any other partition are rejected so a token from one
/// list route can never steer another.
pub fn decode_revision_cursor(
    raw: &str,
) -> Result<HashMap<String, AttributeValue>, ControlPlaneError> {
    let decoded = base64::engine::general_purpose::URL_SAFE
        .decode(raw)
        .map_err(|_| api::validation_error("nextToken is not a valid continuation token"))
        .and_then(|bytes| {
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .map_err(|_| api::validation_error("nextToken is not a valid continuation token"))
        })?;
    let pk = decoded["pk"].as_str().unwrap_or_default();
    let sk = decoded["sk"].as_str().unwrap_or_default();
    if pk != REVISION_PARTITION || !sk.starts_with('r') {
        return Err(api::validation_error(
            "nextToken does not continue the revision history",
        ));
    }
    Ok(HashMap::from([
        ("PK".to_string(), s(pk)),
        ("SK".to_string(), s(sk)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use konductor_common::metadata::store::revision_sort_key;

    #[test]
    fn cursors_round_trip_for_revision_keys() {
        let mut key = HashMap::new();
        key.insert("PK".to_string(), s(REVISION_PARTITION));
        key.insert("SK".to_string(), s(&revision_sort_key(7)));
        let token = encode_cursor(&key).unwrap();
        assert_eq!(decode_revision_cursor(&token).unwrap(), key);
    }

    #[test]
    fn foreign_partition_tokens_are_rejected() {
        let mut key = HashMap::new();
        key.insert("PK".to_string(), s("ENV"));
        key.insert("SK".to_string(), s("dev"));
        let token = encode_cursor(&key).unwrap();
        assert!(decode_revision_cursor(&token).is_err());
    }

    #[test]
    fn garbage_tokens_are_rejected() {
        assert!(decode_revision_cursor("not-a-token!").is_err());
    }
}
