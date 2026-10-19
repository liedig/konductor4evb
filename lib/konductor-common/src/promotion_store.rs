//! DynamoDB item mappings for the promotion records (`promotion` module).
//! Attributes are camelCase, matching the serde field names; timestamps are
//! RFC 3339 strings and `expiresAt` is epoch seconds for the TTL.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use chrono::{DateTime, Utc};

use crate::promotion::{Promotion, PromotionFailureReason, RepinState, SchemaVersionContent};

fn s(value: &str) -> AttributeValue {
    AttributeValue::S(value.to_string())
}

fn n(value: i64) -> AttributeValue {
    AttributeValue::N(value.to_string())
}

fn get_s<'a>(item: &'a HashMap<String, AttributeValue>, key: &str) -> Option<&'a str> {
    item.get(key)?.as_s().ok().map(String::as_str)
}

fn get_n(item: &HashMap<String, AttributeValue>, key: &str) -> Option<i64> {
    item.get(key)?.as_n().ok()?.parse().ok()
}

fn get_ts(item: &HashMap<String, AttributeValue>, key: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(get_s(item, key)?)
        .ok()
        .map(|t| t.into())
}

fn rfc3339(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// The full head-item form; used for the initial conditional put. Later
/// transitions are `UpdateItem` expressions so concurrent writers merge.
pub fn promotion_to_item(p: &Promotion) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::from([
        ("promotionId".to_string(), s(&p.promotion_id)),
        ("domain".to_string(), s(&p.domain)),
        ("schemaName".to_string(), s(&p.schema_name)),
        ("sourceEnvironment".to_string(), s(&p.source_environment)),
        ("targetEnvironment".to_string(), s(&p.target_environment)),
        ("requestedVersion".to_string(), n(p.requested_version)),
        ("phase".to_string(), s(p.phase.as_str())),
        ("actor".to_string(), s(&p.actor)),
        ("createdAt".to_string(), s(&rfc3339(&p.created_at))),
        ("updatedAt".to_string(), s(&rfc3339(&p.updated_at))),
    ]);
    if let Some(v) = p.from_version {
        item.insert("fromVersion".to_string(), n(v));
    }
    if let Some(v) = p.last_written_version {
        item.insert("lastWrittenVersion".to_string(), n(v));
    }
    if let Some(reason) = p.reason {
        let code = match reason {
            PromotionFailureReason::Diverged => "DIVERGED",
            PromotionFailureReason::ReplayFailed => "REPLAY_FAILED",
        };
        item.insert("reason".to_string(), s(code));
    }
    if let Some(v) = p.failed_version {
        item.insert("failedVersion".to_string(), n(v));
    }
    if let Some(v) = p.repin_matched {
        item.insert("repinMatched".to_string(), n(v));
    }
    if let Some(v) = p.repin_repinned {
        item.insert("repinRepinned".to_string(), n(v));
    }
    if let Some(v) = p.repin_failed {
        item.insert("repinFailed".to_string(), n(v));
    }
    if let Some(t) = p.drain_until {
        item.insert("drainUntil".to_string(), s(&rfc3339(&t)));
    }
    if let Some(ttl) = p.expires_at {
        item.insert("expiresAt".to_string(), n(ttl));
    }
    item
}

/// Reads a head item back; returns `None` when required attributes are
/// missing or malformed.
pub fn promotion_from_item(item: &HashMap<String, AttributeValue>) -> Option<Promotion> {
    let reason = match get_s(item, "reason") {
        Some("DIVERGED") => Some(PromotionFailureReason::Diverged),
        Some("REPLAY_FAILED") => Some(PromotionFailureReason::ReplayFailed),
        _ => None,
    };
    Some(Promotion {
        promotion_id: get_s(item, "promotionId")?.to_string(),
        domain: get_s(item, "domain")?.to_string(),
        schema_name: get_s(item, "schemaName")?.to_string(),
        source_environment: get_s(item, "sourceEnvironment")?.to_string(),
        target_environment: get_s(item, "targetEnvironment")?.to_string(),
        from_version: get_n(item, "fromVersion"),
        requested_version: get_n(item, "requestedVersion")?,
        last_written_version: get_n(item, "lastWrittenVersion"),
        phase: get_s(item, "phase")?.parse().ok()?,
        reason,
        failed_version: get_n(item, "failedVersion"),
        actor: get_s(item, "actor")?.to_string(),
        repin_matched: get_n(item, "repinMatched"),
        repin_repinned: get_n(item, "repinRepinned"),
        repin_failed: get_n(item, "repinFailed"),
        drain_until: get_ts(item, "drainUntil"),
        created_at: get_ts(item, "createdAt")?,
        updated_at: get_ts(item, "updatedAt")?,
        expires_at: get_n(item, "expiresAt"),
    })
}

/// The item form of a re-pin coordination record.
pub fn repin_to_item(state: &RepinState) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::from([
        ("floorVersion".to_string(), n(state.floor_version)),
        ("targetVersion".to_string(), n(state.target_version)),
        ("generation".to_string(), n(state.generation)),
        ("updatedAt".to_string(), s(&rfc3339(&state.updated_at))),
    ]);
    if let Some(t) = state.drain_until {
        item.insert("drainUntil".to_string(), s(&rfc3339(&t)));
    }
    if let Some(by) = &state.widened_by {
        item.insert("widenedBy".to_string(), s(by));
    }
    if let Some(pin) = state.pinned_before {
        item.insert("pinnedBefore".to_string(), n(pin));
    }
    item
}

pub fn repin_from_item(item: &HashMap<String, AttributeValue>) -> Option<RepinState> {
    Some(RepinState {
        floor_version: get_n(item, "floorVersion")?,
        target_version: get_n(item, "targetVersion")?,
        generation: get_n(item, "generation")?,
        widened_by: get_s(item, "widenedBy").map(str::to_string),
        pinned_before: get_n(item, "pinnedBefore"),
        drain_until: get_ts(item, "drainUntil"),
        updated_at: get_ts(item, "updatedAt")?,
    })
}

/// The item form of a version-content record.
pub fn version_content_to_item(record: &SchemaVersionContent) -> HashMap<String, AttributeValue> {
    HashMap::from([
        ("domain".to_string(), s(&record.domain)),
        ("schemaName".to_string(), s(&record.schema_name)),
        ("environment".to_string(), s(&record.environment)),
        ("version".to_string(), n(record.version)),
        ("content".to_string(), s(&record.content)),
        ("contentHash".to_string(), s(&record.content_hash)),
        ("writtenBy".to_string(), s(&record.written_by)),
        ("writtenAt".to_string(), s(&rfc3339(&record.written_at))),
    ])
}

pub fn version_content_from_item(
    item: &HashMap<String, AttributeValue>,
) -> Option<SchemaVersionContent> {
    Some(SchemaVersionContent {
        domain: get_s(item, "domain")?.to_string(),
        schema_name: get_s(item, "schemaName")?.to_string(),
        environment: get_s(item, "environment")?.to_string(),
        version: get_n(item, "version")?,
        content: get_s(item, "content")?.to_string(),
        content_hash: get_s(item, "contentHash")?.to_string(),
        written_by: get_s(item, "writtenBy")?.to_string(),
        written_at: get_ts(item, "writtenAt")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::promotion::{promotion_pk, PromotionStatus, PROMOTION_SK};

    fn sample() -> Promotion {
        Promotion {
            promotion_id: "promo-9".to_string(),
            domain: "orders".to_string(),
            schema_name: "OrderCreated".to_string(),
            source_environment: "dev".to_string(),
            target_environment: "staging".to_string(),
            from_version: None,
            requested_version: 3,
            last_written_version: None,
            phase: PromotionStatus::Validating,
            reason: None,
            failed_version: None,
            actor: "oauth:sub-1".to_string(),
            repin_matched: None,
            repin_repinned: None,
            repin_failed: None,
            drain_until: None,
            created_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .into(),
            updated_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .into(),
            expires_at: Some(1_770_000_000),
        }
    }

    #[test]
    fn promotion_item_round_trips() {
        let mut p = sample();
        let item = promotion_to_item(&p);
        assert!(!item.contains_key("fromVersion"));
        let back = promotion_from_item(&item).unwrap();
        assert_eq!(back.promotion_id, "promo-9");
        assert_eq!(back.phase, PromotionStatus::Validating);
        assert_eq!(back.requested_version, 3);
        assert_eq!(back.from_version, None);
        assert_eq!(back.expires_at, Some(1_770_000_000));

        p.from_version = Some(1);
        p.last_written_version = Some(3);
        p.phase = PromotionStatus::Completed;
        p.repin_matched = Some(4);
        p.repin_repinned = Some(4);
        p.repin_failed = Some(0);
        let item = promotion_to_item(&p);
        let back = promotion_from_item(&item).unwrap();
        assert_eq!(back.phase, PromotionStatus::Completed);
        assert_eq!(back.from_version, Some(1));
        assert_eq!(back.repin_repinned, Some(4));
        let view = back.view();
        assert_eq!(view.replayed_versions, vec![2, 3]);
        assert_eq!(view.target_version, Some(3));
        assert_eq!(view.repin.unwrap().matched, 4);
    }

    #[test]
    fn promotion_from_item_rejects_garbage() {
        let item = HashMap::from([("phase".to_string(), s("WAITING"))]);
        assert!(promotion_from_item(&item).is_none());
    }

    #[test]
    fn repin_item_round_trips() {
        let state = RepinState {
            floor_version: 2,
            target_version: 5,
            generation: 3,
            widened_by: None,
            pinned_before: None,
            drain_until: None,
            updated_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .into(),
        };
        let back = repin_from_item(&repin_to_item(&state)).unwrap();
        assert_eq!(back.generation, 3);
        assert_eq!(back.floor_version, 2);
        assert_eq!(back.drain_until, None);
    }

    #[test]
    fn version_content_item_round_trips() {
        let record = SchemaVersionContent {
            domain: "orders".to_string(),
            schema_name: "OrderCreated".to_string(),
            environment: "dev".to_string(),
            version: 3,
            content: "{\"type\":\"object\"}".to_string(),
            content_hash: "abc".to_string(),
            written_by: "schema-admin".to_string(),
            written_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .into(),
        };
        let back = version_content_from_item(&version_content_to_item(&record)).unwrap();
        assert_eq!(back.version, 3);
        assert_eq!(back.written_by, "schema-admin");
        assert_eq!(
            promotion_pk(&back.domain, &back.schema_name, &back.environment),
            "orders#OrderCreated#dev"
        );
        let _ = PROMOTION_SK;
    }
}
