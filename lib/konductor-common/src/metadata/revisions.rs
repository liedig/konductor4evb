//! Immutable revision records, the global revision head, mutation audit
//! records, and the conditional transaction that creates a revision — the
//! persistence half of metadata administration. Every mutation commits its
//! revision, head move, and audit record in one transaction; nothing is
//! ever overwritten in place.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::types::{CancellationReason, Put, TransactWriteItem, Update};
use serde::Serialize;

use crate::dynamodb::{get_s, n, s};
use crate::metadata::store::{
    audit_sort_key, revision_sort_key, AUDIT_PARTITION, HEAD_PARTITION, HEAD_SORT_KEY,
    REVISION_PARTITION,
};

/// The mutation-record operation marker for a revision creation.
pub const AUDIT_OPERATION_REVISION_CREATED: &str = "revision-created";

/// A failure from the metadata configuration store. Conditional-check
/// failures surface as a dedicated variant so callers can resolve replay
/// versus conflict without inspecting SDK error strings.
#[derive(Debug, thiserror::Error)]
pub enum MetadataStoreError {
    #[error("conditional check failed")]
    ConditionFailed,
    #[error("stored item failed to parse: {0}")]
    CorruptItem(String),
    #[error("metadata configuration unavailable: {0}")]
    Unavailable(String),
    #[error("dynamodb error: {0}")]
    Dynamo(String),
}

/// An immutable revision record. The canonical definition and the compiled
/// metadata schema are persisted together so a future compiler change can
/// never silently reinterpret an old revision.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionRecord {
    pub revision: u32,
    pub predecessor: u32,
    pub definition: String,
    pub metadata_schema: String,
    pub definition_digest: String,
    pub compiler_format: u32,
    pub created_by: String,
    pub created_at: String,
    pub operation_id: String,
}

/// The global head item: the latest revision identity.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadRecord {
    pub revision: u32,
    pub definition_digest: String,
    pub updated_at: String,
}

/// The immutable mutation/audit record committed with every configuration
/// transaction. Carries everything the stream consumer needs to emit
/// lifecycle events and everything recovery needs beyond the stream window.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationAuditRecord {
    pub operation: String,
    pub actor: String,
    pub before_revision: u32,
    pub after_revision: u32,
    pub operation_id: String,
    pub occurred_at: String,
    pub occurred_at_millis: i64,
}

/// A revision record as the administration API returns it, with the stored
/// documents parsed back into JSON.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionView {
    pub revision: u32,
    pub predecessor: u32,
    pub definition_digest: String,
    pub compiler_format: u32,
    pub created_by: String,
    pub created_at: String,
    pub definition: serde_json::Value,
    pub metadata_schema: serde_json::Value,
}

/// DynamoDB-backed access to the metadata configuration table.
pub struct MetadataStore {
    pub(crate) client: aws_sdk_dynamodb::Client,
    pub(crate) table: String,
}

impl MetadataStore {
    pub fn new(client: aws_sdk_dynamodb::Client, table: impl Into<String>) -> Self {
        Self {
            client,
            table: table.into(),
        }
    }

    /// Strongly consistent point read of one immutable revision.
    pub async fn get_revision(
        &self,
        revision: u32,
    ) -> Result<Option<RevisionRecord>, MetadataStoreError> {
        let out = self
            .client
            .get_item()
            .table_name(&self.table)
            .consistent_read(true)
            .key("PK", s(REVISION_PARTITION))
            .key("SK", s(&revision_sort_key(revision)))
            .send()
            .await
            .map_err(sdk_error)?;
        out.item().map(item_to_revision).transpose()
    }

    /// Strongly consistent read of the global head.
    pub async fn get_head(&self) -> Result<Option<HeadRecord>, MetadataStoreError> {
        let out = self
            .client
            .get_item()
            .table_name(&self.table)
            .consistent_read(true)
            .key("PK", s(HEAD_PARTITION))
            .key("SK", s(HEAD_SORT_KEY))
            .send()
            .await
            .map_err(sdk_error)?;
        out.item().map(item_to_head).transpose()
    }

    /// Paginated revision history, newest first, strongly consistent.
    /// Returns the page plus the opaque last-evaluated key for continuation.
    pub async fn list_revisions(
        &self,
        limit: i32,
        exclusive_start: Option<HashMap<String, AttributeValue>>,
    ) -> Result<(Vec<RevisionRecord>, Option<HashMap<String, AttributeValue>>), MetadataStoreError>
    {
        let mut request = self
            .client
            .query()
            .table_name(&self.table)
            .consistent_read(true)
            .scan_index_forward(false)
            .limit(limit)
            .key_condition_expression("PK = :pk")
            .expression_attribute_values(":pk", s(REVISION_PARTITION));
        if let Some(key) = exclusive_start {
            request = request.set_exclusive_start_key(Some(key));
        }
        let out = request.send().await.map_err(sdk_error)?;
        let revisions = out
            .items()
            .iter()
            .map(item_to_revision)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((revisions, out.last_evaluated_key().cloned()))
    }

    /// Commits a new immutable revision atomically: the revision record is
    /// created only if absent, the head moves only from `expected_head`, and
    /// the audit record is created in the same transaction. Any other
    /// concurrent state leaves all three untouched.
    pub async fn create_revision(
        &self,
        record: &RevisionRecord,
        audit: &MutationAuditRecord,
        expected_head: u32,
    ) -> Result<(), MetadataStoreError> {
        let revision_put = Put::builder()
            .table_name(&self.table)
            .set_item(Some(revision_to_item(record)))
            .condition_expression("attribute_not_exists(PK)")
            .build()
            .map_err(|e| MetadataStoreError::Dynamo(e.to_string()))?;
        let head_update = Update::builder()
            .table_name(&self.table)
            .key("PK", s(HEAD_PARTITION))
            .key("SK", s(HEAD_SORT_KEY))
            .condition_expression("attribute_exists(PK) AND #rev = :expected")
            .update_expression("SET #rev = :next, definitionDigest = :digest, updatedAt = :when")
            .expression_attribute_names("#rev", "revision")
            .expression_attribute_values(":expected", n(expected_head as i64))
            .expression_attribute_values(":next", n(record.revision as i64))
            .expression_attribute_values(":digest", s(&record.definition_digest))
            .expression_attribute_values(":when", s(&record.created_at))
            .build()
            .map_err(|e| MetadataStoreError::Dynamo(e.to_string()))?;
        let audit_put = Put::builder()
            .table_name(&self.table)
            .set_item(Some(audit_to_item(audit)))
            .condition_expression("attribute_not_exists(PK)")
            .build()
            .map_err(|e| MetadataStoreError::Dynamo(e.to_string()))?;

        self.client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(revision_put).build())
            .transact_items(TransactWriteItem::builder().update(head_update).build())
            .transact_items(TransactWriteItem::builder().put(audit_put).build())
            .send()
            .await
            .map_err(map_transact_error)?;
        Ok(())
    }
}

fn sdk_error(e: impl std::fmt::Debug) -> MetadataStoreError {
    MetadataStoreError::Dynamo(format!("{e:?}"))
}

fn map_transact_error(
    error: aws_sdk_dynamodb::error::SdkError<
        aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError,
    >,
) -> MetadataStoreError {
    use aws_sdk_dynamodb::error::SdkError;
    use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
    if let SdkError::ServiceError(service) = &error {
        if let TransactWriteItemsError::TransactionCanceledException(cancelled) = service.err() {
            if cancelled
                .cancellation_reasons()
                .iter()
                .any(|r: &CancellationReason| r.code() == Some("ConditionalCheckFailed"))
            {
                return MetadataStoreError::ConditionFailed;
            }
        }
    }
    sdk_error(error)
}

fn get_n(item: &HashMap<String, AttributeValue>, key: &str) -> Option<i64> {
    item.get(key)
        .and_then(|v| v.as_n().ok())
        .and_then(|raw| raw.parse().ok())
}

/// Serializes a revision record into its DynamoDB item. Writers outside the
/// store (the baseline bootstrap) must go through this builder so seeded
/// records can never drift from what [`item_to_revision`] requires.
pub fn revision_to_item(record: &RevisionRecord) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    item.insert("PK".to_string(), s(REVISION_PARTITION));
    item.insert("SK".to_string(), s(&revision_sort_key(record.revision)));
    item.insert("revision".to_string(), n(record.revision as i64));
    item.insert("predecessor".to_string(), n(record.predecessor as i64));
    item.insert("definition".to_string(), s(&record.definition));
    item.insert("metadataSchema".to_string(), s(&record.metadata_schema));
    item.insert("definitionDigest".to_string(), s(&record.definition_digest));
    item.insert(
        "compilerFormat".to_string(),
        n(record.compiler_format as i64),
    );
    item.insert("createdBy".to_string(), s(&record.created_by));
    item.insert("createdAt".to_string(), s(&record.created_at));
    item.insert("operationId".to_string(), s(&record.operation_id));
    item
}

fn item_to_revision(
    item: &HashMap<String, AttributeValue>,
) -> Result<RevisionRecord, MetadataStoreError> {
    let corrupt = |field: &str| MetadataStoreError::CorruptItem(format!("revision.{field}"));
    Ok(RevisionRecord {
        revision: get_n(item, "revision")
            .ok_or_else(|| corrupt("revision"))?
            .try_into()
            .map_err(|_| corrupt("revision"))?,
        predecessor: get_n(item, "predecessor")
            .ok_or_else(|| corrupt("predecessor"))?
            .try_into()
            .map_err(|_| corrupt("predecessor"))?,
        definition: get_s(item, "definition").ok_or_else(|| corrupt("definition"))?,
        metadata_schema: get_s(item, "metadataSchema").ok_or_else(|| corrupt("metadataSchema"))?,
        definition_digest: get_s(item, "definitionDigest")
            .ok_or_else(|| corrupt("definitionDigest"))?,
        compiler_format: get_n(item, "compilerFormat")
            .ok_or_else(|| corrupt("compilerFormat"))?
            .try_into()
            .map_err(|_| corrupt("compilerFormat"))?,
        created_by: get_s(item, "createdBy").ok_or_else(|| corrupt("createdBy"))?,
        created_at: get_s(item, "createdAt").ok_or_else(|| corrupt("createdAt"))?,
        operation_id: get_s(item, "operationId").ok_or_else(|| corrupt("operationId"))?,
    })
}

fn item_to_head(item: &HashMap<String, AttributeValue>) -> Result<HeadRecord, MetadataStoreError> {
    let corrupt = |field: &str| MetadataStoreError::CorruptItem(format!("head.{field}"));
    Ok(HeadRecord {
        revision: get_n(item, "revision")
            .ok_or_else(|| corrupt("revision"))?
            .try_into()
            .map_err(|_| corrupt("revision"))?,
        definition_digest: get_s(item, "definitionDigest")
            .ok_or_else(|| corrupt("definitionDigest"))?,
        updated_at: get_s(item, "updatedAt").ok_or_else(|| corrupt("updatedAt"))?,
    })
}

fn audit_to_item(audit: &MutationAuditRecord) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    item.insert("PK".to_string(), s(AUDIT_PARTITION));
    item.insert(
        "SK".to_string(),
        s(&audit_sort_key(
            audit.occurred_at_millis,
            &audit.operation_id,
        )),
    );
    item.insert("operation".to_string(), s(&audit.operation));
    item.insert("actor".to_string(), s(&audit.actor));
    item.insert(
        "beforeRevision".to_string(),
        n(audit.before_revision as i64),
    );
    item.insert("afterRevision".to_string(), n(audit.after_revision as i64));
    item.insert("operationId".to_string(), s(&audit.operation_id));
    item.insert("occurredAt".to_string(), s(&audit.occurred_at));
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> RevisionRecord {
        RevisionRecord {
            revision: 2,
            predecessor: 1,
            definition: r#"{"children":[]}"#.to_string(),
            metadata_schema: "{}".to_string(),
            definition_digest: "a".repeat(64),
            compiler_format: 1,
            created_by: "admin-sub".to_string(),
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
            operation_id: "op-1".to_string(),
        }
    }

    #[test]
    fn revision_items_round_trip() {
        let item = revision_to_item(&record());
        assert_eq!(item_to_revision(&item).unwrap(), record());
    }

    #[test]
    fn corrupt_revision_items_are_rejected() {
        let mut item = revision_to_item(&record());
        item.remove("definitionDigest");
        assert!(matches!(
            item_to_revision(&item),
            Err(MetadataStoreError::CorruptItem(_))
        ));
    }
}
