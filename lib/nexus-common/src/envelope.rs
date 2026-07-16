use serde_json::{json, Value};

use crate::error::{ControlPlaneError, ValidationExceptionField, ValidationExceptionReason};

/// Maximum length EventBridge allows for a rule name.
pub const RULE_NAME_MAX_LEN: usize = 64;

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
}
