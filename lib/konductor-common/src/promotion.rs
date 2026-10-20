//! Schema promotion records: the `konductor-schema-promotions` head and
//! re-pin items and the `konductor-schema-versions` content store.
//!
//! Both tables key on `{domain}#{eventName}#{environment}`. The promotions
//! table holds one `PROMOTION` head item (doubling as the per-target lease)
//! and one `REPIN` coordination item per schema and environment; the versions
//! table holds one item per schema version with the canonical content the
//! registry itself can no longer return.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Public lifecycle of a promotion, stored as the head item's `phase`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PromotionStatus {
    Validating,
    Replaying,
    Repinning,
    Completed,
    Failed,
    Cancelled,
}

impl PromotionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Validating => "VALIDATING",
            Self::Replaying => "REPLAYING",
            Self::Repinning => "REPINNING",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
        }
    }

    /// Whether the head item still holds the per-target lease.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Validating | Self::Replaying | Self::Repinning)
    }
}

impl std::fmt::Display for PromotionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for PromotionStatus {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "VALIDATING" => Ok(Self::Validating),
            "REPLAYING" => Ok(Self::Replaying),
            "REPINNING" => Ok(Self::Repinning),
            "COMPLETED" => Ok(Self::Completed),
            "FAILED" => Ok(Self::Failed),
            "CANCELLED" => Ok(Self::Cancelled),
            _ => Err(()),
        }
    }
}

/// Why a promotion reached `FAILED`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PromotionFailureReason {
    /// Target history is not a prefix of the source's.
    Diverged,
    /// A replay step failed and retries were exhausted.
    ReplayFailed,
    /// The source is not the target's immediate predecessor in the
    /// configured environment pipeline. Only reachable by invoking the
    /// promotion workflow out-of-band; the API derives an adjacent pair.
    SkippedEnvironment,
}

/// Re-pin tallies in the status view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepinCounts {
    pub matched: i64,
    pub repinned: i64,
    pub failed: i64,
}

/// The `GET /schemas/promotions` response: the head item projected into the
/// public status shape.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionView {
    pub promotion_id: String,
    pub schema_name: String,
    pub source_environment: String,
    pub target_environment: String,
    pub requested_version: i64,
    pub status: PromotionStatus,
    /// Source versions the workflow has durably written to the target.
    pub replayed_versions: Vec<i64>,
    /// The target's latest version once replaying finished.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repin: Option<RepinCounts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<PromotionFailureReason>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Promotion {
    /// Projects the head item into the public status shape.
    pub fn view(&self) -> PromotionView {
        let replayed_versions = match (self.from_version, self.last_written_version) {
            (Some(from), Some(last)) if last > from => ((from + 1)..=last).collect(),
            _ => Vec::new(),
        };
        let repin = match (self.repin_matched, self.repin_repinned, self.repin_failed) {
            (Some(matched), Some(repinned), Some(failed)) => Some(RepinCounts {
                matched,
                repinned,
                failed,
            }),
            _ => None,
        };
        PromotionView {
            promotion_id: self.promotion_id.clone(),
            schema_name: self.schema_name.clone(),
            source_environment: self.source_environment.clone(),
            target_environment: self.target_environment.clone(),
            requested_version: self.requested_version,
            status: self.phase,
            replayed_versions,
            target_version: self.last_written_version,
            repin,
            reason: self.reason,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// The `PROMOTION` head item: the most recent promotion into one target
/// environment for one schema, doubling as the per-target lease.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Promotion {
    pub promotion_id: String,
    pub domain: String,
    pub schema_name: String,
    pub source_environment: String,
    pub target_environment: String,
    /// Source version the target was verified against when planning ran;
    /// absent until the workflow's plan step commits it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_version: Option<i64>,
    /// Source version the promotion replays through.
    pub requested_version: i64,
    /// Highest version the workflow has durably written to the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_written_version: Option<i64>,
    pub phase: PromotionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<PromotionFailureReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_version: Option<i64>,
    /// Principal that requested the promotion.
    pub actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repin_matched: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repin_repinned: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repin_failed: Option<i64>,
    /// When the drain window closes, for console display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drain_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// DynamoDB TTL, epoch seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

/// The `REPIN` item: re-pin coordination state for one schema and
/// environment. Outside a re-pin, `floor_version == target_version` equals
/// the pinned version and the derived rule pattern is `[pinned, pinned + 1]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepinState {
    pub floor_version: i64,
    pub target_version: i64,
    /// Optimistic-concurrency token; narrowing commits only on its own
    /// generation, so a newer re-pin always wins.
    pub generation: i64,
    /// Promotion that performed the latest widening; a retried widen
    /// short-circuits on it instead of double-bumping the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widened_by: Option<String>,
    /// The pin subscriptions held before the latest widening; the narrow
    /// step falls back to it when the replay wrote nothing. Removed by a
    /// committed narrow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_before: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drain_until: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// One item of the `konductor-schema-versions` content store: the canonical
/// content of one schema version in one environment, recorded because the
/// registry serves only the latest version and normalizes whitespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaVersionContent {
    pub domain: String,
    pub schema_name: String,
    pub environment: String,
    pub version: i64,
    /// Content as accepted by the append-time gate.
    pub content: String,
    /// SHA-256 of the canonical JSON form.
    pub content_hash: String,
    /// `schema-admin` for authoring writes, a promotion id for replays.
    pub written_by: String,
    pub written_at: DateTime<Utc>,
}

/// Partition key shared by both tables.
pub fn promotion_pk(domain: &str, schema_name: &str, environment: &str) -> String {
    format!("{domain}#{schema_name}#{environment}")
}

/// The `writtenBy` identity schema-admin stamps on version records it
/// wrote; the promotion plan gate treats any other writer as divergence.
pub const WRITTEN_BY_SCHEMA_ADMIN: &str = "schema-admin";

pub const PROMOTION_SK: &str = "PROMOTION";
pub const REPIN_SK: &str = "REPIN";

/// Versions-table sort key. Zero-padded so lexicographic order matches
/// version order.
pub fn version_sk(version: i64) -> String {
    format!("{version:020}")
}

/// Canonical-JSON equality: whitespace- and key-order-insensitive. Returns
/// `None` when either side is not valid JSON.
pub fn canonical_content_eq(a: &str, b: &str) -> Option<bool> {
    let va: serde_json::Value = serde_json::from_str(a).ok()?;
    let vb: serde_json::Value = serde_json::from_str(b).ok()?;
    Some(va == vb)
}

/// Recursively re-orders object keys so serialization is stable regardless
/// of the `preserve_order` feature or input document order.
fn canonical_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let mut sorted = serde_json::Map::new();
            for (key, value) in entries {
                sorted.insert(key.clone(), canonical_value(value));
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical_value).collect())
        }
        other => other.clone(),
    }
}

/// SHA-256 of the canonical JSON form, the equality basis for every content
/// comparison. Returns `None` when the content is not valid JSON, which the
/// append-time gate has already rejected.
pub fn canonical_content_hash(content: &str) -> Option<String> {
    use sha2::Digest;
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    let canonical = serde_json::to_vec(&canonical_value(&value)).ok()?;
    Some(
        sha2::Sha256::digest(&canonical)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_promotion() -> Promotion {
        Promotion {
            promotion_id: "promo-123".to_string(),
            domain: "orders".to_string(),
            schema_name: "OrderCreated".to_string(),
            source_environment: "dev".to_string(),
            target_environment: "staging".to_string(),
            from_version: Some(2),
            requested_version: 5,
            last_written_version: Some(3),
            phase: PromotionStatus::Replaying,
            reason: None,
            failed_version: None,
            actor: "cognito:acme/orders-publishers".to_string(),
            repin_matched: None,
            repin_repinned: None,
            repin_failed: None,
            drain_until: None,
            created_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .into(),
            updated_at: DateTime::parse_from_rfc3339("2026-01-01T00:01:00Z")
                .unwrap()
                .into(),
            expires_at: Some(1_770_000_000),
        }
    }

    #[test]
    fn promotion_status_serializes_screaming_snake() {
        let json = serde_json::to_value(PromotionStatus::Replaying).unwrap();
        assert_eq!(json, "REPLAYING");
        let parsed: PromotionStatus = serde_json::from_value(json!("VALIDATING")).unwrap();
        assert_eq!(parsed, PromotionStatus::Validating);
        assert!(PromotionStatus::Validating.is_active());
        assert!(!PromotionStatus::Completed.is_active());
        assert_eq!(
            "FAILED".parse::<PromotionStatus>(),
            Ok(PromotionStatus::Failed)
        );
    }

    #[test]
    fn promotion_round_trips_with_camel_case_fields() {
        let promotion = sample_promotion();
        let json = serde_json::to_value(&promotion).unwrap();
        assert_eq!(json["promotionId"], "promo-123");
        assert_eq!(json["sourceEnvironment"], "dev");
        assert_eq!(json["phase"], "REPLAYING");
        assert_eq!(json["lastWrittenVersion"], 3);
        assert!(json.get("reason").is_none());
        let back: Promotion = serde_json::from_value(json).unwrap();
        assert_eq!(back.promotion_id, promotion.promotion_id);
        assert_eq!(back.phase, promotion.phase);
        assert_eq!(back.last_written_version, Some(3));
    }

    #[test]
    fn failure_reason_serializes_screaming_snake() {
        let json = serde_json::to_value(PromotionFailureReason::Diverged).unwrap();
        assert_eq!(json, "DIVERGED");
        let parsed: PromotionFailureReason =
            serde_json::from_value(json!("REPLAY_FAILED")).unwrap();
        assert_eq!(parsed, PromotionFailureReason::ReplayFailed);
        let skipped = serde_json::to_value(PromotionFailureReason::SkippedEnvironment).unwrap();
        assert_eq!(skipped, "SKIPPED_ENVIRONMENT");
    }

    #[test]
    fn schema_version_content_round_trips() {
        let item = SchemaVersionContent {
            domain: "orders".to_string(),
            schema_name: "OrderCreated".to_string(),
            environment: "staging".to_string(),
            version: 5,
            content: "{\"type\":\"object\"}".to_string(),
            content_hash: "abc123".to_string(),
            written_by: "promo-123".to_string(),
            written_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .into(),
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["contentHash"], "abc123");
        assert_eq!(json["writtenBy"], "promo-123");
        let back: SchemaVersionContent = serde_json::from_value(json).unwrap();
        assert_eq!(back.version, 5);
        assert_eq!(back.environment, "staging");
    }

    #[test]
    fn repin_state_round_trips() {
        let state = RepinState {
            widened_by: None,
            pinned_before: None,
            floor_version: 2,
            target_version: 5,
            generation: 7,
            drain_until: None,
            updated_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .into(),
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["floorVersion"], 2);
        assert_eq!(json["generation"], 7);
        let back: RepinState = serde_json::from_value(json).unwrap();
        assert_eq!(back.target_version, 5);
    }

    #[test]
    fn keys_follow_the_table_layout() {
        assert_eq!(
            promotion_pk("orders", "OrderCreated", "staging"),
            "orders#OrderCreated#staging"
        );
        assert_eq!(PROMOTION_SK, "PROMOTION");
        assert_eq!(REPIN_SK, "REPIN");
        assert_eq!(version_sk(3), "00000000000000000003");
        assert!(version_sk(2) < version_sk(10));
    }

    #[test]
    fn canonical_hash_is_whitespace_and_key_order_insensitive() {
        let a = canonical_content_hash("{\"a\":1,\"b\":2}").unwrap();
        let b = canonical_content_hash("{\n  \"b\": 2,\n  \"a\": 1\n}").unwrap();
        assert_eq!(a, b);
        let c = canonical_content_hash("{\"a\":1,\"b\":3}").unwrap();
        assert_ne!(a, c);
        assert!(canonical_content_hash("not json").is_none());
        assert_eq!(
            canonical_content_eq("{\"a\":1,\"b\":[2,3]}", "{ \"b\": [2, 3], \"a\": 1 }"),
            Some(true)
        );
        assert_eq!(canonical_content_eq("{\"a\":1}", "{\"a\":2}"), Some(false));
        assert_eq!(canonical_content_eq("nope", "{}"), None);
    }
}
