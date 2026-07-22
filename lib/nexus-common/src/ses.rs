//! SES error-code classification for notification Lambdas.
//!
//! Notification Lambdas share one classification rule: deterministic SES
//! rejections (unverified identity in sandbox mode, malformed address) are
//! permanent and must not be retried, because retrying a deterministic
//! rejection cannot succeed. Transient failures (throttling, service errors)
//! propagate so the asynchronous invocation retry and failure-DLQ behavior
//! applies unchanged.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use serde_json::Value;

/// Returns `true` when the SES `SendEmail` error code is permanent.
///
/// All other service errors and errors without service metadata are transient.
pub fn is_permanent_failure(error_code: &str) -> bool {
    matches!(error_code, "MessageRejected" | "BadRequestException")
}

/// Owner-facing recipients from an EventBridge event detail: `ownerEmail`
/// plus any `secondaryOwnerEmails`, deduplicated. Returns `None` when
/// `ownerEmail` is missing; absent or empty secondaries degrade to
/// owner-only addressing.
pub fn recipients_from_detail(detail: &Value) -> Option<Vec<String>> {
    let owner = detail.get("ownerEmail")?.as_str()?;
    let secondaries = detail
        .get("secondaryOwnerEmails")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str))
        .into_iter()
        .flatten();
    Some(collect_recipients(owner, secondaries))
}

/// Owner-facing recipients from a domain record: `ownerEmail` plus any
/// `secondaryOwnerEmails` string set, deduplicated. Returns `None` when
/// `ownerEmail` is missing; absent or empty secondaries degrade to
/// owner-only addressing.
pub fn recipients_from_item(item: &HashMap<String, AttributeValue>) -> Option<Vec<String>> {
    let owner = item.get("ownerEmail").and_then(|v| v.as_s().ok())?;
    let secondaries = item
        .get("secondaryOwnerEmails")
        .and_then(|v| v.as_ss().ok())
        .map(|list| list.iter().map(String::as_str))
        .into_iter()
        .flatten();
    Some(collect_recipients(owner, secondaries))
}

fn collect_recipients<'a>(owner: &str, secondaries: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut recipients = vec![owner.to_string()];
    for email in secondaries {
        if !recipients.iter().any(|r| r == email) {
            recipients.push(email.to_string());
        }
    }
    recipients
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_rejected_is_permanent() {
        assert!(is_permanent_failure("MessageRejected"));
    }

    #[test]
    fn bad_request_is_permanent() {
        assert!(is_permanent_failure("BadRequestException"));
    }

    #[test]
    fn other_service_error_is_transient() {
        assert!(!is_permanent_failure("InternalServiceError"));
    }

    #[test]
    fn detail_recipients_include_owner_and_secondaries() {
        let detail = json!({
            "ownerEmail": "owner@example.com",
            "secondaryOwnerEmails": ["a@example.com", "b@example.com"]
        });
        assert_eq!(
            recipients_from_detail(&detail).unwrap(),
            vec!["owner@example.com", "a@example.com", "b@example.com"]
        );
    }

    #[test]
    fn detail_recipients_degrade_to_owner_only_without_secondaries() {
        let detail = json!({"ownerEmail": "owner@example.com"});
        assert_eq!(
            recipients_from_detail(&detail).unwrap(),
            vec!["owner@example.com"]
        );

        let empty = json!({"ownerEmail": "owner@example.com", "secondaryOwnerEmails": []});
        assert_eq!(
            recipients_from_detail(&empty).unwrap(),
            vec!["owner@example.com"]
        );
    }

    #[test]
    fn detail_recipients_none_when_owner_missing() {
        assert!(
            recipients_from_detail(&json!({"secondaryOwnerEmails": ["a@example.com"]})).is_none()
        );
    }

    #[test]
    fn detail_recipients_dedupe_owner_repeated_as_secondary() {
        let detail = json!({
            "ownerEmail": "owner@example.com",
            "secondaryOwnerEmails": ["owner@example.com", "a@example.com", "a@example.com"]
        });
        assert_eq!(
            recipients_from_detail(&detail).unwrap(),
            vec!["owner@example.com", "a@example.com"]
        );
    }

    #[test]
    fn item_recipients_include_owner_and_secondaries() {
        let mut item = HashMap::new();
        item.insert(
            "ownerEmail".to_string(),
            AttributeValue::S("owner@example.com".to_string()),
        );
        item.insert(
            "secondaryOwnerEmails".to_string(),
            AttributeValue::Ss(vec!["a@example.com".to_string()]),
        );
        assert_eq!(
            recipients_from_item(&item).unwrap(),
            vec!["owner@example.com", "a@example.com"]
        );
    }

    #[test]
    fn item_recipients_degrade_to_owner_only_without_secondaries() {
        let mut item = HashMap::new();
        item.insert(
            "ownerEmail".to_string(),
            AttributeValue::S("owner@example.com".to_string()),
        );
        assert_eq!(
            recipients_from_item(&item).unwrap(),
            vec!["owner@example.com"]
        );
    }

    #[test]
    fn item_recipients_none_when_owner_missing() {
        assert!(recipients_from_item(&HashMap::new()).is_none());
    }
}
