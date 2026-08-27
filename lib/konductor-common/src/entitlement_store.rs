//! DynamoDB access for the authorization entitlements table: item mapping,
//! strongly consistent consumer-partition decision reads, administration GSI
//! queries, and conditional mutation transactions that commit each mutation
//! with its audit record atomically.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::{AttributeValue, CancellationReason, Delete, Put, TransactWriteItem};
use chrono::{DateTime, Utc};

use crate::dynamodb::{get_s, s};
use crate::entitlement::{Entitlement, EntitlementAuditRecord, EntitlementSummary, ScopePattern};
use crate::grant::AuditMutation;

/// The sparse GSI serving producer-scoped administration lists.
pub const GSI_BY_PRODUCER: &str = "by-producer";
/// The sparse GSI serving revocation lookups by stable identifier.
pub const GSI_BY_ENTITLEMENT_ID: &str = "by-entitlement-id";

/// A failure to read or write the authorization entitlements table.
#[derive(Debug, thiserror::Error)]
pub enum EntitlementStoreError {
    #[error("an active entitlement already exists for this consumer, producer, and scope")]
    Conflict,
    #[error("entitlement not found")]
    NotFound,
    #[error("stored item failed to parse: {0}")]
    CorruptItem(String),
    #[error("dynamodb error: {0}")]
    Dynamo(String),
}

/// Typed access to the authorization entitlements table.
pub struct EntitlementStore {
    client: aws_sdk_dynamodb::Client,
    table: String,
}

/// Serializes an entitlement to its table item, including the sparse GSI
/// attributes the two administration indexes key on.
pub fn entitlement_to_item(entitlement: &Entitlement) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    item.insert("PK".to_string(), s(&entitlement.pk()));
    item.insert("SK".to_string(), s(&entitlement.sk()));
    item.insert("GSI1PK".to_string(), s(&entitlement.gsi1pk()));
    item.insert("GSI1SK".to_string(), s(&entitlement.gsi1sk()));
    item.insert("itemType".to_string(), s("Entitlement"));
    item.insert("entitlementId".to_string(), s(&entitlement.entitlement_id));
    item.insert(
        "consumerDomain".to_string(),
        s(&entitlement.consumer_domain),
    );
    item.insert(
        "producerDomain".to_string(),
        s(&entitlement.producer_domain),
    );
    item.insert("scopePattern".to_string(), s(entitlement.scope.as_str()));
    item.insert(
        "createdAt".to_string(),
        s(&entitlement.created_at.to_rfc3339()),
    );
    item.insert("createdBy".to_string(), s(&entitlement.created_by));
    item
}

/// Parses a table item back into an entitlement; anything missing or invalid
/// is corrupt.
pub fn item_to_entitlement(
    item: &HashMap<String, AttributeValue>,
) -> Result<Entitlement, EntitlementStoreError> {
    let corrupt = |field: &str| EntitlementStoreError::CorruptItem(field.to_string());
    let entitlement_id = get_s(item, "entitlementId").ok_or_else(|| corrupt("entitlementId"))?;
    let consumer_domain = get_s(item, "consumerDomain").ok_or_else(|| corrupt("consumerDomain"))?;
    let producer_domain = get_s(item, "producerDomain").ok_or_else(|| corrupt("producerDomain"))?;
    let scope =
        ScopePattern::new(get_s(item, "scopePattern").ok_or_else(|| corrupt("scopePattern"))?)
            .map_err(|_| corrupt("scopePattern"))?;
    let created_at = get_s(item, "createdAt")
        .and_then(|v| v.parse::<DateTime<Utc>>().ok())
        .ok_or_else(|| corrupt("createdAt"))?;
    let created_by = get_s(item, "createdBy").ok_or_else(|| corrupt("createdBy"))?;
    Ok(Entitlement {
        entitlement_id,
        consumer_domain,
        producer_domain,
        scope,
        created_at,
        created_by,
    })
}

fn summary_to_attr(summary: &EntitlementSummary) -> AttributeValue {
    let mut map = HashMap::new();
    map.insert("consumerDomain".to_string(), s(&summary.consumer_domain));
    map.insert("producerDomain".to_string(), s(&summary.producer_domain));
    map.insert("scopePattern".to_string(), s(&summary.scope_pattern));
    AttributeValue::M(map)
}

/// Serializes an audit record. Carries no GSI or identifier attributes, so
/// the sparse indexes exclude audit items.
pub fn entitlement_audit_to_item(
    record: &EntitlementAuditRecord,
) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    item.insert("PK".to_string(), s(&record.pk()));
    item.insert("SK".to_string(), s(&record.sk()));
    item.insert("itemType".to_string(), s("EntitlementAuditRecord"));
    item.insert(
        "mutation".to_string(),
        s(match record.mutation {
            AuditMutation::Create => "create",
            AuditMutation::Revoke => "revoke",
        }),
    );
    item.insert("actor".to_string(), s(&record.actor));
    if let Some(before) = &record.before {
        item.insert("before".to_string(), summary_to_attr(before));
    }
    if let Some(after) = &record.after {
        item.insert("after".to_string(), summary_to_attr(after));
    }
    item
}

impl EntitlementStore {
    /// Builds a store against `table` (the `konductor-authorization-entitlements`
    /// table provisioned by the core stack).
    pub fn new(client: aws_sdk_dynamodb::Client, table: impl Into<String>) -> Self {
        Self {
            client,
            table: table.into(),
        }
    }

    /// The coverage-decision read: a strongly consistent list of a consumer
    /// domain's current entitlements from its base-table partition. Consumer
    /// partitions are small (bounded by the per-consumer cap), so a single
    /// page always holds them.
    pub async fn entitlements_for_consumer(
        &self,
        consumer_domain: &str,
    ) -> Result<Vec<Entitlement>, EntitlementStoreError> {
        let output = self
            .client
            .query()
            .table_name(&self.table)
            .consistent_read(true)
            .key_condition_expression("PK = :pk")
            .expression_attribute_values(":pk", s(&format!("CONSUMER#{consumer_domain}")))
            .send()
            .await
            .map_err(|e| EntitlementStoreError::Dynamo(format!("{e:?}")))?;
        output
            .items
            .unwrap_or_default()
            .iter()
            .map(item_to_entitlement)
            .collect()
    }

    /// Locates an entitlement by its stable identifier through the sparse
    /// by-entitlement-id GSI. Administration path only; never feeds a
    /// coverage decision.
    pub async fn find_entitlement_by_id(
        &self,
        entitlement_id: &str,
    ) -> Result<Option<Entitlement>, EntitlementStoreError> {
        let output = self
            .client
            .query()
            .table_name(&self.table)
            .index_name(GSI_BY_ENTITLEMENT_ID)
            .key_condition_expression("entitlementId = :id")
            .expression_attribute_values(":id", s(entitlement_id))
            .send()
            .await
            .map_err(|e| EntitlementStoreError::Dynamo(format!("{e:?}")))?;
        match output.items.unwrap_or_default().first() {
            Some(item) => Ok(Some(item_to_entitlement(item)?)),
            None => Ok(None),
        }
    }

    /// Enumerates the entitlements a producer domain has granted, through the
    /// by-producer GSI. Eventually consistent; administration path only.
    pub async fn list_entitlements_for_producer(
        &self,
        producer_domain: &str,
    ) -> Result<Vec<Entitlement>, EntitlementStoreError> {
        let output = self
            .client
            .query()
            .table_name(&self.table)
            .index_name(GSI_BY_PRODUCER)
            .key_condition_expression("GSI1PK = :pk")
            .expression_attribute_values(":pk", s(&format!("PRODUCER#{producer_domain}")))
            .send()
            .await
            .map_err(|e| EntitlementStoreError::Dynamo(format!("{e:?}")))?;
        output
            .items
            .unwrap_or_default()
            .iter()
            .map(item_to_entitlement)
            .collect()
    }

    /// Commits a new entitlement and its audit record in one transaction.
    /// The condition makes the create idempotent under retries: the same
    /// triple derives the same identifier, so a replayed create conflicts
    /// instead of duplicating.
    pub async fn create_entitlement(
        &self,
        entitlement: &Entitlement,
        audit: &EntitlementAuditRecord,
    ) -> Result<(), EntitlementStoreError> {
        let put = Put::builder()
            .table_name(&self.table)
            .set_item(Some(entitlement_to_item(entitlement)))
            .condition_expression("attribute_not_exists(PK)")
            .build()
            .map_err(|e| EntitlementStoreError::Dynamo(format!("{e:?}")))?;
        let audit_put = Put::builder()
            .table_name(&self.table)
            .set_item(Some(entitlement_audit_to_item(audit)))
            .build()
            .map_err(|e| EntitlementStoreError::Dynamo(format!("{e:?}")))?;
        self.client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put).build())
            .transact_items(TransactWriteItem::builder().put(audit_put).build())
            .send()
            .await
            .map_err(|e| map_transact_error(e, EntitlementStoreError::Conflict))?;
        Ok(())
    }

    /// Removes an entitlement and commits its audit record in one
    /// transaction; the condition turns a double-revoke into `NotFound`.
    pub async fn revoke_entitlement(
        &self,
        entitlement: &Entitlement,
        audit: &EntitlementAuditRecord,
    ) -> Result<(), EntitlementStoreError> {
        let delete = Delete::builder()
            .table_name(&self.table)
            .key("PK", s(&entitlement.pk()))
            .key("SK", s(&entitlement.sk()))
            .condition_expression("attribute_exists(PK)")
            .build()
            .map_err(|e| EntitlementStoreError::Dynamo(format!("{e:?}")))?;
        let audit_put = Put::builder()
            .table_name(&self.table)
            .set_item(Some(entitlement_audit_to_item(audit)))
            .build()
            .map_err(|e| EntitlementStoreError::Dynamo(format!("{e:?}")))?;
        self.client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().delete(delete).build())
            .transact_items(TransactWriteItem::builder().put(audit_put).build())
            .send()
            .await
            .map_err(|e| map_transact_error(e, EntitlementStoreError::NotFound))?;
        Ok(())
    }
}

fn map_transact_error(
    error: aws_sdk_dynamodb::error::SdkError<
        aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError,
    >,
    on_condition_failure: EntitlementStoreError,
) -> EntitlementStoreError {
    use aws_sdk_dynamodb::error::SdkError;
    use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
    if let SdkError::ServiceError(service) = &error {
        if let TransactWriteItemsError::TransactionCanceledException(cancelled) = service.err() {
            if cancelled
                .cancellation_reasons()
                .iter()
                .any(|r: &CancellationReason| r.code() == Some("ConditionalCheckFailed"))
            {
                return on_condition_failure;
            }
        }
    }
    EntitlementStoreError::Dynamo(format!("{error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_ts() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z")
            .unwrap()
            .to_utc()
    }

    fn domain_wide_entitlement() -> Entitlement {
        Entitlement::new(
            "checkout",
            "orders",
            ScopePattern::new("*").unwrap(),
            fixed_ts(),
            "platform-admin-sub",
        )
        .unwrap()
    }

    fn prefix_entitlement() -> Entitlement {
        Entitlement::new(
            "checkout",
            "orders",
            ScopePattern::new("Order*").unwrap(),
            fixed_ts(),
            "platform-admin-sub",
        )
        .unwrap()
    }

    #[test]
    fn domain_wide_item_round_trips() {
        let entitlement = domain_wide_entitlement();
        let item = entitlement_to_item(&entitlement);
        assert_eq!(item_to_entitlement(&item).unwrap(), entitlement);
    }

    #[test]
    fn prefix_item_round_trips() {
        let entitlement = prefix_entitlement();
        let item = entitlement_to_item(&entitlement);
        assert_eq!(item_to_entitlement(&item).unwrap(), entitlement);
    }

    #[test]
    fn entitlement_item_carries_documented_keys() {
        let entitlement = prefix_entitlement();
        let item = entitlement_to_item(&entitlement);
        assert_eq!(get_s(&item, "PK").unwrap(), "CONSUMER#checkout");
        assert_eq!(
            get_s(&item, "SK").unwrap(),
            format!("ENTITLEMENT#{}", entitlement.entitlement_id)
        );
        assert_eq!(get_s(&item, "GSI1PK").unwrap(), "PRODUCER#orders");
        assert_eq!(
            get_s(&item, "GSI1SK").unwrap(),
            format!(
                "CONSUMER#checkout#ENTITLEMENT#{}",
                entitlement.entitlement_id
            )
        );
        assert_eq!(get_s(&item, "itemType").unwrap(), "Entitlement");
        assert_eq!(get_s(&item, "scopePattern").unwrap(), "Order*");
        assert_eq!(
            get_s(&item, "entitlementId").unwrap(),
            entitlement.entitlement_id
        );
    }

    #[test]
    fn corrupt_items_are_rejected() {
        let mut item = entitlement_to_item(&prefix_entitlement());
        item.remove("scopePattern");
        assert!(matches!(
            item_to_entitlement(&item),
            Err(EntitlementStoreError::CorruptItem(_))
        ));

        let mut item = entitlement_to_item(&prefix_entitlement());
        item.insert("scopePattern".to_string(), s("Or*der"));
        assert!(matches!(
            item_to_entitlement(&item),
            Err(EntitlementStoreError::CorruptItem(_))
        ));

        let mut item = entitlement_to_item(&prefix_entitlement());
        item.remove("createdAt");
        assert!(matches!(
            item_to_entitlement(&item),
            Err(EntitlementStoreError::CorruptItem(_))
        ));
    }

    #[test]
    fn audit_item_carries_no_gsi_or_id_attributes() {
        let entitlement = domain_wide_entitlement();
        let record = EntitlementAuditRecord {
            entitlement_id: entitlement.entitlement_id.clone(),
            timestamp: fixed_ts(),
            mutation: AuditMutation::Create,
            actor: "platform-admin-sub".to_string(),
            before: None,
            after: Some(EntitlementSummary::from(&entitlement)),
        };
        let item = entitlement_audit_to_item(&record);
        assert_eq!(
            get_s(&item, "PK").unwrap(),
            format!("ENTITLEMENT#{}#AUDIT", entitlement.entitlement_id)
        );
        assert_eq!(get_s(&item, "itemType").unwrap(), "EntitlementAuditRecord");
        assert_eq!(get_s(&item, "mutation").unwrap(), "create");
        assert!(!item.contains_key("GSI1PK"));
        assert!(!item.contains_key("GSI1SK"));
        assert!(!item.contains_key("entitlementId"));
        assert!(item.contains_key("after"));
        assert!(!item.contains_key("before"));
    }
}
