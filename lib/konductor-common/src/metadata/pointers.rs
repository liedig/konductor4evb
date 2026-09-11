//! Per-environment active pointers, the activation transaction, and the
//! mutation audit history. The pointer update is the cutover: one
//! transaction conditionally moves the pointer from the client's expected
//! active revision and commits the audit record that carries everything
//! the stream consumer and recovery paths need.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::types::{CancellationReason, Put, TransactWriteItem, Update};
use serde::Serialize;

use crate::dynamodb::{get_s, n, s};
use crate::metadata::revisions::{MetadataStore, MetadataStoreError, MutationAuditRecord};
use crate::metadata::store::{audit_sort_key, AUDIT_PARTITION, ENVIRONMENT_PARTITION};

/// The mutation-record operation marker for an environment activation.
pub const AUDIT_OPERATION_ENVIRONMENT_ACTIVATED: &str = "environment-activated";

/// One activation, fully resolved: the target environment, the candidate
/// revision and its digest, the actor, the cutover instant, and the
/// expected active revision the pointer must currently hold.
pub struct ActivationIntent {
    pub environment: String,
    pub candidate: u32,
    pub definition_digest: String,
    pub actor: String,
    pub when: String,
    pub expected_active: u32,
}

/// The environment pointer: admission's authoritative metadata selection.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentPointer {
    pub environment: String,
    pub active_revision: u32,
    pub generation: u64,
    pub activated_by: String,
    pub activated_at: String,
}

impl MetadataStore {
    /// Strongly consistent point read of one environment pointer.
    pub async fn get_environment_pointer(
        &self,
        environment: &str,
    ) -> Result<Option<EnvironmentPointer>, MetadataStoreError> {
        let out = self
            .client
            .get_item()
            .table_name(&self.table)
            .consistent_read(true)
            .key("PK", s(ENVIRONMENT_PARTITION))
            .key("SK", s(environment))
            .send()
            .await
            .map_err(sdk_error)?;
        out.item().map(item_to_pointer).transpose()
    }

    /// Strongly consistent read of every environment pointer.
    pub async fn list_environment_pointers(
        &self,
    ) -> Result<Vec<EnvironmentPointer>, MetadataStoreError> {
        let out = self
            .client
            .query()
            .table_name(&self.table)
            .consistent_read(true)
            .key_condition_expression("PK = :pk")
            .expression_attribute_values(":pk", s(ENVIRONMENT_PARTITION))
            .send()
            .await
            .map_err(sdk_error)?;
        out.items().iter().map(item_to_pointer).collect()
    }

    /// Paginated mutation audit history, newest first, strongly
    /// consistent, optionally filtered to one operation kind.
    pub async fn list_audit_records(
        &self,
        limit: i32,
        exclusive_start: Option<HashMap<String, AttributeValue>>,
        operation: Option<&str>,
    ) -> Result<
        (
            Vec<MutationAuditRecord>,
            Option<HashMap<String, AttributeValue>>,
        ),
        MetadataStoreError,
    > {
        let mut request = self
            .client
            .query()
            .table_name(&self.table)
            .consistent_read(true)
            .scan_index_forward(false)
            .limit(limit)
            .key_condition_expression("PK = :pk")
            .expression_attribute_values(":pk", s(AUDIT_PARTITION));
        if let Some(operation) = operation {
            request = request
                .filter_expression("#op = :op")
                .expression_attribute_names("#op", "operation")
                .expression_attribute_values(":op", s(operation));
        }
        if let Some(key) = exclusive_start {
            request = request.set_exclusive_start_key(Some(key));
        }
        let out = request.send().await.map_err(sdk_error)?;
        let records = out
            .items()
            .iter()
            .map(item_to_audit)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((records, out.last_evaluated_key().cloned()))
    }

    /// The activation cutover: the pointer moves only from the client's
    /// expected active revision, its generation advances (invalidating
    /// admission's generation-scoped caches), and the audit record commits
    /// in the same transaction. Any concurrent pointer state leaves both
    /// untouched.
    pub async fn activate_revision(
        &self,
        intent: &ActivationIntent,
        audit: &MutationAuditRecord,
    ) -> Result<(), MetadataStoreError> {
        let pointer_update = Update::builder()
            .table_name(&self.table)
            .key("PK", s(ENVIRONMENT_PARTITION))
            .key("SK", s(&intent.environment))
            .condition_expression("attribute_exists(PK) AND activeRevision = :expected")
            .update_expression(
                "SET activeRevision = :candidate, \
                 generation = if_not_exists(generation, :zero) + :one, \
                 definitionDigest = :digest, activatedBy = :actor, activatedAt = :when",
            )
            .expression_attribute_values(":expected", n(intent.expected_active as i64))
            .expression_attribute_values(":candidate", n(intent.candidate as i64))
            .expression_attribute_values(":zero", n(0))
            .expression_attribute_values(":one", n(1))
            .expression_attribute_values(":digest", s(&intent.definition_digest))
            .expression_attribute_values(":actor", s(&intent.actor))
            .expression_attribute_values(":when", s(&intent.when))
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
            .transact_items(TransactWriteItem::builder().update(pointer_update).build())
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

fn item_to_pointer(
    item: &HashMap<String, AttributeValue>,
) -> Result<EnvironmentPointer, MetadataStoreError> {
    let corrupt = |field: &str| MetadataStoreError::CorruptItem(format!("pointer.{field}"));
    Ok(EnvironmentPointer {
        environment: get_s(item, "SK").ok_or_else(|| corrupt("environment"))?,
        active_revision: get_n(item, "activeRevision")
            .ok_or_else(|| corrupt("activeRevision"))?
            .try_into()
            .map_err(|_| corrupt("activeRevision"))?,
        generation: get_n(item, "generation")
            .ok_or_else(|| corrupt("generation"))?
            .try_into()
            .map_err(|_| corrupt("generation"))?,
        activated_by: get_s(item, "activatedBy").ok_or_else(|| corrupt("activatedBy"))?,
        activated_at: get_s(item, "activatedAt").ok_or_else(|| corrupt("activatedAt"))?,
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

fn item_to_audit(
    item: &HashMap<String, AttributeValue>,
) -> Result<MutationAuditRecord, MetadataStoreError> {
    let corrupt = |field: &str| MetadataStoreError::CorruptItem(format!("audit.{field}"));
    let sk = get_s(item, "SK").ok_or_else(|| corrupt("SK"))?;
    let occurred_at_millis = sk
        .split('#')
        .next()
        .and_then(|millis| millis.parse().ok())
        .ok_or_else(|| corrupt("SK"))?;
    Ok(MutationAuditRecord {
        operation: get_s(item, "operation").ok_or_else(|| corrupt("operation"))?,
        actor: get_s(item, "actor").ok_or_else(|| corrupt("actor"))?,
        before_revision: get_n(item, "beforeRevision")
            .ok_or_else(|| corrupt("beforeRevision"))?
            .try_into()
            .map_err(|_| corrupt("beforeRevision"))?,
        after_revision: get_n(item, "afterRevision")
            .ok_or_else(|| corrupt("afterRevision"))?
            .try_into()
            .map_err(|_| corrupt("afterRevision"))?,
        operation_id: get_s(item, "operationId").ok_or_else(|| corrupt("operationId"))?,
        occurred_at: get_s(item, "occurredAt").ok_or_else(|| corrupt("occurredAt"))?,
        occurred_at_millis,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer() -> EnvironmentPointer {
        EnvironmentPointer {
            environment: "dev".to_string(),
            active_revision: 2,
            generation: 3,
            activated_by: "admin-sub".to_string(),
            activated_at: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    fn audit() -> MutationAuditRecord {
        MutationAuditRecord {
            operation: AUDIT_OPERATION_ENVIRONMENT_ACTIVATED.to_string(),
            actor: "admin-sub".to_string(),
            before_revision: 1,
            after_revision: 2,
            operation_id: "op-act-1".to_string(),
            occurred_at: "2026-01-01T00:00:00.000Z".to_string(),
            occurred_at_millis: 1_767_225_600_000,
        }
    }

    #[test]
    fn audit_items_round_trip() {
        let item = audit_to_item(&audit());
        assert_eq!(item_to_audit(&item).unwrap(), audit());
        assert_eq!(get_s(&item, "SK").unwrap(), "1767225600000#op-act-1");
    }

    #[test]
    fn pointer_items_parse_or_reject() {
        let audit = audit();
        let mut item = audit_to_item(&audit);
        item.insert("SK".to_string(), s("dev"));
        item.insert("activeRevision".to_string(), n(2));
        item.insert("generation".to_string(), n(3));
        item.insert("activatedBy".to_string(), s("admin-sub"));
        item.insert("activatedAt".to_string(), s("2026-01-01T00:00:00.000Z"));
        let parsed = item_to_pointer(&item).unwrap();
        assert_eq!(parsed, pointer());

        item.remove("generation");
        assert!(matches!(
            item_to_pointer(&item),
            Err(MetadataStoreError::CorruptItem(_))
        ));
    }
}
