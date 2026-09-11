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
use konductor_common::metadata::store::{AUDIT_PARTITION, REVISION_PARTITION};

/// Encodes a last-evaluated key as an opaque continuation token.
pub fn encode_cursor(key: &HashMap<String, AttributeValue>) -> Option<String> {
    let pk = get_s(key, "PK")?;
    let sk = get_s(key, "SK")?;
    let payload = serde_json::json!({ "pk": pk, "sk": sk });
    Some(base64::engine::general_purpose::URL_SAFE.encode(payload.to_string()))
}

/// Decodes a continuation token into a revision-history exclusive-start key.
pub fn decode_revision_cursor(
    raw: &str,
) -> Result<HashMap<String, AttributeValue>, ControlPlaneError> {
    decode_cursor(raw, REVISION_PARTITION, |sk| sk.starts_with('r'))
}

/// Decodes a continuation token into an audit-history exclusive-start key.
pub fn decode_audit_cursor(
    raw: &str,
) -> Result<HashMap<String, AttributeValue>, ControlPlaneError> {
    decode_cursor(raw, AUDIT_PARTITION, |sk| {
        sk.split('#')
            .next()
            .is_some_and(|millis| !millis.is_empty() && millis.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Tokens are validated against the partition and sort-key shape they
/// claim to continue, so a token from one list route can never steer
/// another.
fn decode_cursor(
    raw: &str,
    partition: &str,
    sk_valid: impl Fn(&str) -> bool,
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
    if pk != partition || !sk_valid(sk) {
        return Err(api::validation_error(
            "nextToken does not continue this history",
        ));
    }
    Ok(HashMap::from([
        ("PK".to_string(), s(pk)),
        ("SK".to_string(), s(sk)),
    ]))
}

/// Parses and validates the shared list-route paging parameters: a bounded
/// page size plus an opaque, partition-checked continuation token.
/// `Err` carries the ready-to-send error response.
pub fn page_request(
    payload: &serde_json::Value,
    decode: fn(&str) -> Result<HashMap<String, AttributeValue>, ControlPlaneError>,
) -> Result<(i32, Option<HashMap<String, AttributeValue>>), serde_json::Value> {
    let limit = match api::query_param(payload, "maxResults") {
        None => crate::handler::DEFAULT_PAGE_SIZE,
        Some(raw) => match raw.parse::<i32>() {
            Ok(v) if (1..=crate::handler::MAX_PAGE_SIZE).contains(&v) => v,
            _ => {
                return Err(api::from_control_plane_error(&api::validation_error(
                    format!(
                        "query parameter 'maxResults' must be between 1 and {}",
                        crate::handler::MAX_PAGE_SIZE
                    ),
                )))
            }
        },
    };
    let exclusive_start = match api::query_param(payload, "nextToken") {
        None => None,
        Some(raw) => match decode(&raw) {
            Ok(key) => Some(key),
            Err(e) => return Err(api::from_control_plane_error(&e)),
        },
    };
    Ok((limit, exclusive_start))
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
        assert!(decode_audit_cursor(&token).is_err());
    }

    #[test]
    fn audit_cursors_round_trip_and_reject_revision_keys() {
        let mut key = HashMap::new();
        key.insert("PK".to_string(), s(AUDIT_PARTITION));
        key.insert("SK".to_string(), s("1767225600000#op-act-1"));
        let token = encode_cursor(&key).unwrap();
        assert_eq!(decode_audit_cursor(&token).unwrap(), key);

        let mut revision_key = HashMap::new();
        revision_key.insert("PK".to_string(), s(REVISION_PARTITION));
        revision_key.insert("SK".to_string(), s("r0000000002"));
        let token = encode_cursor(&revision_key).unwrap();
        assert!(decode_audit_cursor(&token).is_err());
    }

    #[test]
    fn garbage_tokens_are_rejected() {
        assert!(decode_revision_cursor("not-a-token!").is_err());
    }
}
