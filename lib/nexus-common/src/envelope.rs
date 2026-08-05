use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{ControlPlaneError, ValidationExceptionField, ValidationExceptionReason};

/// Maximum length EventBridge allows for a rule name.
pub const RULE_NAME_MAX_LEN: usize = 64;

/// Maximum size EventBridge accepts for a single PutEvents entry.
pub const PUT_EVENTS_ENTRY_MAX_BYTES: usize = 256 * 1024;

/// Canonical `detail.metadata` stamped by the publish path. All three fields
/// are platform-owned: callers can neither supply nor override them.
/// `idempotency_key` and `published_at` are fixed at queue-acceptance time and
/// survive SQS retries and EventBridge replays.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EventMetadata {
    pub schema_version: i64,
    pub idempotency_key: String,
    #[serde(with = "iso8601_millis")]
    pub published_at: DateTime<Utc>,
}

/// The canonical envelope `detail`: platform-owned `metadata` plus the
/// caller's business `data`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvelopeDetail {
    pub metadata: EventMetadata,
    pub data: Value,
}

/// A fully constructed event ready for PutEvents: canonical identity,
/// envelope detail, and target bus.
#[derive(Debug, Clone, PartialEq)]
pub struct EventEnvelope {
    pub source: String,
    pub detail_type: String,
    pub event_bus_name: String,
    pub detail: EnvelopeDetail,
}

impl EventEnvelope {
    /// Builds and validates an envelope. `source` is the domain's
    /// environment-independent namespace identity; `detail_type` is the event
    /// name; `data` is the caller's business payload, passed through
    /// unvalidated.
    pub fn new(
        source: String,
        detail_type: String,
        event_bus_name: String,
        schema_version: i64,
        idempotency_key: String,
        published_at: DateTime<Utc>,
        data: Value,
    ) -> Result<Self, ControlPlaneError> {
        let envelope = Self {
            source,
            detail_type,
            event_bus_name,
            detail: EnvelopeDetail {
                metadata: EventMetadata {
                    schema_version,
                    idempotency_key,
                    published_at,
                },
                data,
            },
        };
        envelope.validate()?;
        Ok(envelope)
    }

    /// Envelope and EventBridge entry constraints, checked before delivery.
    /// Business data is never validated against schema content.
    pub fn validate(&self) -> Result<(), ControlPlaneError> {
        if self.source.is_empty() {
            return Err(validation_error(
                "source",
                "source must not be empty".to_string(),
            ));
        }
        if self.detail_type.is_empty() {
            return Err(validation_error(
                "detailType",
                "detail-type must not be empty".to_string(),
            ));
        }
        if self.event_bus_name.is_empty() {
            return Err(validation_error(
                "eventBusName",
                "event bus name must not be empty".to_string(),
            ));
        }
        if self.detail.metadata.schema_version <= 0 {
            return Err(validation_error(
                "schemaVersion",
                format!(
                    "schema version must be a positive number, got {}",
                    self.detail.metadata.schema_version
                ),
            ));
        }
        if self.detail.metadata.idempotency_key.is_empty() {
            return Err(validation_error(
                "idempotencyKey",
                "idempotency key must not be empty".to_string(),
            ));
        }
        let entry_bytes = serde_json::to_vec(&self.entry_json()).map_err(|e| {
            validation_error("detail", format!("envelope is not serializable: {e}"))
        })?;
        if entry_bytes.len() > PUT_EVENTS_ENTRY_MAX_BYTES {
            return Err(validation_error(
                "detail",
                format!(
                    "entry is {} bytes; EventBridge allows at most {PUT_EVENTS_ENTRY_MAX_BYTES}",
                    entry_bytes.len()
                ),
            ));
        }
        Ok(())
    }

    /// The PutEvents entry shape: top-level envelope fields plus the
    /// serialized detail, mirroring the wire format for size validation.
    pub fn entry_json(&self) -> Value {
        json!({
            "Source": self.source,
            "DetailType": self.detail_type,
            "EventBusName": self.event_bus_name,
            "Detail": serde_json::to_string(&self.detail).unwrap_or_default(),
        })
    }
}

/// ISO 8601 with exactly millisecond precision and a `Z` suffix, matching the
/// SQS `SentTimestamp` epoch-milliseconds origin of `publishedAt`.
mod iso8601_millis {
    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(dt: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&dt.to_rfc3339_opts(SecondsFormat::Millis, true))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        DateTime::parse_from_rfc3339(&String::deserialize(d)?)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

/// Converts an SQS `SentTimestamp` attribute (epoch milliseconds, as a string)
/// into the envelope's `publishedAt` timestamp.
pub fn published_at_from_sent_timestamp(
    sent_timestamp: &str,
) -> Result<DateTime<Utc>, ControlPlaneError> {
    let millis: i64 = sent_timestamp.parse().map_err(|_| {
        validation_error(
            "publishedAt",
            format!("SQS SentTimestamp '{sent_timestamp}' is not epoch milliseconds"),
        )
    })?;
    DateTime::from_timestamp_millis(millis).ok_or_else(|| {
        validation_error(
            "publishedAt",
            format!("SQS SentTimestamp '{sent_timestamp}' is out of range"),
        )
    })
}

/// Canonical event identity for a schema: `{source}@{detail-type}`.
///
/// `source` is the producing domain's environment-independent namespace
/// identity (e.g. `acme-orders`); `detail_type` is the event name. The schema
/// name alone encodes the full subscription filter — it is the same string in
/// every environment and round-trips to the AWS Schemas API.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaName {
    pub source: String,
    pub detail_type: String,
}

impl SchemaName {
    /// Derives the schema name for an event of a domain, using the domain's
    /// namespace identity as `source`. The platform derives `source`; it is
    /// never free-typed by producers.
    pub fn for_domain(namespace_identity: &str, detail_type: &str) -> Self {
        Self {
            source: namespace_identity.to_string(),
            detail_type: detail_type.to_string(),
        }
    }

    /// Parses `{source}@{detail-type}`, splitting on the first `@`.
    pub fn parse(schema_name: &str) -> Result<Self, ControlPlaneError> {
        match schema_name.split_once('@') {
            Some((source, detail_type)) if !source.is_empty() && !detail_type.is_empty() => {
                Ok(Self {
                    source: source.to_string(),
                    detail_type: detail_type.to_string(),
                })
            }
            _ => Err(validation_error(
                "schemaName",
                format!("schema name '{schema_name}' must be '{{source}}@{{detail-type}}' with non-empty source and detail-type"),
            )),
        }
    }

    /// The event pattern for a subscription rule pinned to `schema_version`.
    /// Filters on source, detail-type, and the numeric schema version stamped
    /// at `detail.metadata.schemaVersion`.
    pub fn rule_pattern(&self, schema_version: i64) -> Value {
        json!({
            "source": [self.source],
            "detail-type": [self.detail_type],
            "detail": { "metadata": { "schemaVersion": [schema_version] } }
        })
    }
}

impl std::fmt::Display for SchemaName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.source, self.detail_type)
    }
}

/// Derives the subscription rule name `{consumerDomain}-{detailType}` and
/// validates it against EventBridge constraints. The rule lives on the
/// producer's platform bus, so source is implied and the version is banned
/// from the name — migration must be a pattern update on the same rule.
pub fn rule_name(consumer_domain: &str, detail_type: &str) -> Result<String, ControlPlaneError> {
    let name = format!("{consumer_domain}-{detail_type}");
    if name.len() > RULE_NAME_MAX_LEN {
        return Err(validation_error(
            "detailType",
            format!(
                "rule name '{name}' is {} characters; EventBridge allows at most {RULE_NAME_MAX_LEN}",
                name.len()
            ),
        ));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !c.is_ascii_alphanumeric() && !matches!(c, '.' | '_' | '-'))
    {
        return Err(validation_error(
            "detailType",
            format!("rule name '{name}' contains '{bad}'; allowed characters are [A-Za-z0-9._-]"),
        ));
    }
    Ok(name)
}

fn validation_error(field: &str, message: String) -> ControlPlaneError {
    ControlPlaneError::ValidationException {
        message: message.clone(),
        reason: ValidationExceptionReason::FieldValidationFailed,
        field_list: Some(vec![ValidationExceptionField {
            name: field.to_string(),
            message,
        }]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_name_composes_from_domain_identity() {
        let schema = SchemaName::for_domain("acme-orders", "OrderNotification");
        assert_eq!(schema.to_string(), "acme-orders@OrderNotification");
    }

    #[test]
    fn schema_name_parses_on_first_at_sign() {
        let schema = SchemaName::parse("acme-orders@OrderNotification").unwrap();
        assert_eq!(schema.source, "acme-orders");
        assert_eq!(schema.detail_type, "OrderNotification");
    }

    #[test]
    fn schema_name_allows_dots_in_detail_type() {
        let schema = SchemaName::parse("acme-orders@order.notification.v2").unwrap();
        assert_eq!(schema.detail_type, "order.notification.v2");
    }

    #[test]
    fn schema_name_rejects_missing_at_sign() {
        assert!(SchemaName::parse("acme-orders-OrderNotification").is_err());
    }

    #[test]
    fn schema_name_rejects_empty_source_or_detail_type() {
        assert!(SchemaName::parse("@OrderNotification").is_err());
        assert!(SchemaName::parse("acme-orders@").is_err());
    }

    #[test]
    fn schema_name_round_trips_through_display() {
        let input = "acme-orders@OrderNotification";
        let schema = SchemaName::parse(input).unwrap();
        assert_eq!(schema.to_string(), input);
    }

    #[test]
    fn rule_pattern_pins_numeric_version() {
        let schema = SchemaName::parse("acme-orders@OrderNotification").unwrap();
        let pattern = schema.rule_pattern(3);
        assert_eq!(
            pattern,
            serde_json::json!({
                "source": ["acme-orders"],
                "detail-type": ["OrderNotification"],
                "detail": { "metadata": { "schemaVersion": [3] } }
            })
        );
    }

    #[test]
    fn rule_name_combines_consumer_and_detail_type() {
        assert_eq!(
            rule_name("checkout", "OrderNotification").unwrap(),
            "checkout-OrderNotification"
        );
    }

    #[test]
    fn rule_name_accepts_exactly_64_chars() {
        let detail_type = "a".repeat(RULE_NAME_MAX_LEN - "checkout-".len());
        assert!(rule_name("checkout", &detail_type).is_ok());
    }

    #[test]
    fn rule_name_rejects_65_chars() {
        let detail_type = "a".repeat(RULE_NAME_MAX_LEN - "checkout-".len() + 1);
        let err = rule_name("checkout", &detail_type).unwrap_err();
        assert!(err.to_string().contains("at most 64"));
    }

    #[test]
    fn rule_name_rejects_invalid_characters() {
        assert!(rule_name("checkout", "Order Notification").is_err());
        assert!(rule_name("checkout", "Order@Notification").is_err());
        assert!(rule_name("checkout", "Order/Notification").is_err());
    }

    #[test]
    fn rule_name_allows_dots_underscores_hyphens() {
        assert!(rule_name("checkout", "order.placed_v2-final").is_ok());
    }

    fn sample_envelope() -> EventEnvelope {
        EventEnvelope::new(
            "acme-orders".to_string(),
            "OrderNotification".to_string(),
            "acme-prod-orders-event-bus".to_string(),
            4,
            "key-123".to_string(),
            DateTime::from_timestamp_millis(1785407400123).unwrap(),
            json!({"orderId": 123, "amount": 99.50}),
        )
        .unwrap()
    }

    #[test]
    fn envelope_metadata_contains_exactly_the_publish_fields() {
        let envelope = sample_envelope();
        let detail = serde_json::to_value(&envelope.detail).unwrap();

        let metadata = detail["metadata"].as_object().unwrap();
        let mut keys: Vec<&String> = metadata.keys().collect();
        keys.sort();
        assert_eq!(keys, ["idempotencyKey", "publishedAt", "schemaVersion"]);
        assert_eq!(metadata["schemaVersion"], json!(4));
        assert_eq!(metadata["idempotencyKey"], json!("key-123"));
        assert_eq!(metadata["publishedAt"], json!("2026-07-30T10:30:00.123Z"));
        assert_eq!(detail["data"], json!({"orderId": 123, "amount": 99.50}));
    }

    #[test]
    fn envelope_entry_carries_canonical_identity() {
        let entry = sample_envelope().entry_json();
        assert_eq!(entry["Source"], json!("acme-orders"));
        assert_eq!(entry["DetailType"], json!("OrderNotification"));
        assert_eq!(entry["EventBusName"], json!("acme-prod-orders-event-bus"));
    }

    #[test]
    fn envelope_rejects_non_positive_schema_version() {
        for version in [0, -1] {
            let result = EventEnvelope::new(
                "acme-orders".into(),
                "OrderNotification".into(),
                "acme-prod-orders-event-bus".into(),
                version,
                "key-123".into(),
                Utc::now(),
                json!({}),
            );
            assert!(result.is_err(), "version {version} must be rejected");
        }
    }

    #[test]
    fn envelope_rejects_empty_idempotency_key() {
        let result = EventEnvelope::new(
            "acme-orders".into(),
            "OrderNotification".into(),
            "acme-prod-orders-event-bus".into(),
            1,
            String::new(),
            Utc::now(),
            json!({}),
        );
        assert!(result.is_err());
    }

    #[test]
    fn envelope_rejects_oversized_entry() {
        let big_data = json!({ "blob": "x".repeat(PUT_EVENTS_ENTRY_MAX_BYTES) });
        let result = EventEnvelope::new(
            "acme-orders".into(),
            "OrderNotification".into(),
            "acme-prod-orders-event-bus".into(),
            1,
            "key-123".into(),
            Utc::now(),
            big_data,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("at most"), "unexpected error: {err}");
    }

    #[test]
    fn published_at_serializes_as_iso8601_millis() {
        let ts = DateTime::from_timestamp_millis(1785407400123).unwrap();
        let metadata = EventMetadata {
            schema_version: 1,
            idempotency_key: "k".to_string(),
            published_at: ts,
        };
        let value = serde_json::to_value(&metadata).unwrap();
        assert_eq!(value["publishedAt"], json!("2026-07-30T10:30:00.123Z"));

        let round_tripped: EventMetadata = serde_json::from_value(value).unwrap();
        assert_eq!(round_tripped.published_at, ts);
    }

    #[test]
    fn sent_timestamp_converts_to_published_at() {
        let ts = published_at_from_sent_timestamp("1785407400123").unwrap();
        assert_eq!(
            ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "2026-07-30T10:30:00.123Z"
        );
    }

    #[test]
    fn sent_timestamp_rejects_malformed_input() {
        assert!(published_at_from_sent_timestamp("not-a-number").is_err());
        assert!(published_at_from_sent_timestamp("").is_err());
    }
}
