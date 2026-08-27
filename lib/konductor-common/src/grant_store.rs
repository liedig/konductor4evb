//! DynamoDB access for the authorization bindings table: item mapping, strongly
//! consistent decision reads, administration GSI queries, and conditional
//! mutation transactions that commit each mutation with its audit record
//! atomically.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::{AttributeValue, CancellationReason, Delete, Put, TransactWriteItem};
use chrono::DateTime;

use crate::dynamodb::{get_s, s};
use crate::grant::{
    AuditMutation, AuditRecord, BindingPrincipal, BindingState, BindingSummary, ResourceRole,
    RoleBinding,
};

/// The administration GSI enumerating bindings by domain.
pub const GSI_BY_DOMAIN: &str = "by-domain";
/// The administration GSI locating a binding by its stable identifier.
pub const GSI_BY_BINDING_ID: &str = "by-binding-id";

/// A failure from the bindings store.
///
/// Conditional-check failures map to deterministic variants so callers can
/// answer 409/404 without inspecting SDK error strings.
#[derive(Debug, thiserror::Error)]
pub enum GrantStoreError {
    #[error("an active binding already exists for this principal, role, and domain")]
    Conflict,
    #[error("binding not found or not in a revocable state")]
    NotFound,
    #[error("stored item failed to parse: {0}")]
    CorruptItem(String),
    #[error("dynamodb error: {0}")]
    Dynamo(String),
}

/// DynamoDB-backed access to the authorization bindings table.
pub struct BindingStore {
    client: aws_sdk_dynamodb::Client,
    table: String,
}

impl BindingStore {
    pub fn new(client: aws_sdk_dynamodb::Client, table: impl Into<String>) -> Self {
        Self {
            client,
            table: table.into(),
        }
    }

    /// Strongly consistent point read used by authorization decisions.
    pub async fn get_binding(
        &self,
        pk: &str,
        sk: &str,
    ) -> Result<Option<RoleBinding>, GrantStoreError> {
        let out = self
            .client
            .get_item()
            .table_name(&self.table)
            .consistent_read(true)
            .key("PK", s(pk))
            .key("SK", s(sk))
            .send()
            .await
            .map_err(sdk_error)?;
        out.item().map(item_to_binding).transpose()
    }

    /// Locates a binding by its stable identifier through the sparse
    /// `by-binding-id` GSI. Administration path only; never feeds a decision.
    pub async fn find_binding_by_id(
        &self,
        binding_id: &str,
    ) -> Result<Option<RoleBinding>, GrantStoreError> {
        let out = self
            .client
            .query()
            .table_name(&self.table)
            .index_name(GSI_BY_BINDING_ID)
            .key_condition_expression("bindingId = :id")
            .expression_attribute_values(":id", s(binding_id))
            .send()
            .await
            .map_err(sdk_error)?;
        match out.items().first() {
            None => Ok(None),
            Some(item) => item_to_binding(item).map(Some),
        }
    }

    /// Enumerates bindings for a domain through the `by-domain` GSI.
    /// Eventually consistent; administration path only.
    pub async fn list_bindings_for_domain(
        &self,
        domain: &str,
    ) -> Result<Vec<RoleBinding>, GrantStoreError> {
        let out = self
            .client
            .query()
            .table_name(&self.table)
            .index_name(GSI_BY_DOMAIN)
            .key_condition_expression("GSI1PK = :pk")
            .expression_attribute_values(":pk", s(&format!("DOMAIN#{domain}")))
            .send()
            .await
            .map_err(sdk_error)?;
        out.items().iter().map(item_to_binding).collect()
    }

    /// Idempotent create: the binding put is conditioned on the full key not
    /// existing, and the audit record commits in the same transaction.
    pub async fn create_binding(
        &self,
        binding: &RoleBinding,
        audit: &AuditRecord,
    ) -> Result<(), GrantStoreError> {
        let put = Put::builder()
            .table_name(&self.table)
            .set_item(Some(binding_to_item(binding)))
            .condition_expression("attribute_not_exists(PK)")
            .build()
            .map_err(|e| GrantStoreError::Dynamo(e.to_string()))?;
        self.transact_mutation(
            TransactWriteItem::builder().put(put).build(),
            audit,
            GrantStoreError::Conflict,
        )
        .await
    }

    /// Durable revoke: the delete is conditioned on the binding existing in the
    /// active state, and the audit record commits in the same transaction.
    pub async fn revoke_binding(
        &self,
        binding: &RoleBinding,
        audit: &AuditRecord,
    ) -> Result<(), GrantStoreError> {
        let delete = Delete::builder()
            .table_name(&self.table)
            .key("PK", s(&binding.pk()))
            .key("SK", s(&binding.sk()))
            .condition_expression("attribute_exists(PK) AND #st = :active")
            .expression_attribute_names("#st", "state")
            .expression_attribute_values(":active", s("active"))
            .build()
            .map_err(|e| GrantStoreError::Dynamo(e.to_string()))?;
        self.transact_mutation(
            TransactWriteItem::builder().delete(delete).build(),
            audit,
            GrantStoreError::NotFound,
        )
        .await
    }

    async fn transact_mutation(
        &self,
        mutation: TransactWriteItem,
        audit: &AuditRecord,
        on_condition_failure: GrantStoreError,
    ) -> Result<(), GrantStoreError> {
        let audit_put = Put::builder()
            .table_name(&self.table)
            .set_item(Some(audit_to_item(audit)))
            .build()
            .map_err(|e| GrantStoreError::Dynamo(e.to_string()))?;
        let audit_put = TransactWriteItem::builder().put(audit_put).build();
        self.client
            .transact_write_items()
            .transact_items(mutation)
            .transact_items(audit_put)
            .send()
            .await
            .map_err(|e| map_transact_error(e, on_condition_failure))?;
        Ok(())
    }
}

/// Maps a stored item back to a binding, rejecting items that do not parse.
pub fn item_to_binding(
    item: &HashMap<String, AttributeValue>,
) -> Result<RoleBinding, GrantStoreError> {
    let corrupt = |field: &str| GrantStoreError::CorruptItem(field.to_string());
    let principal = match get_s(item, "principalType").ok_or(corrupt("principalType"))? {
        t if t == "cognito-group" => BindingPrincipal::CognitoGroup {
            issuer: get_s(item, "issuer").ok_or(corrupt("issuer"))?,
            group: get_s(item, "group").ok_or(corrupt("group"))?,
        },
        t if t == "aws-account" => BindingPrincipal::AwsAccount {
            account_id: get_s(item, "accountId").ok_or(corrupt("accountId"))?,
        },
        other => {
            return Err(GrantStoreError::CorruptItem(format!(
                "principalType={other}"
            )))
        }
    };
    let role = match get_s(item, "role").ok_or(corrupt("role"))?.as_str() {
        "publisher" => ResourceRole::Publisher,
        "subscriber" => ResourceRole::Subscriber,
        "owner" => ResourceRole::Owner,
        other => return Err(GrantStoreError::CorruptItem(format!("role={other}"))),
    };
    let created_at =
        DateTime::parse_from_rfc3339(&get_s(item, "createdAt").ok_or(corrupt("createdAt"))?)
            .map_err(|e| GrantStoreError::CorruptItem(format!("createdAt: {e}")))?
            .to_utc();
    Ok(RoleBinding {
        binding_id: get_s(item, "bindingId").ok_or(corrupt("bindingId"))?,
        principal,
        role,
        domain: get_s(item, "domain").ok_or(corrupt("domain"))?,
        state: BindingState::Active,
        created_at,
        created_by: get_s(item, "createdBy").ok_or(corrupt("createdBy"))?,
    })
}

/// Maps a binding to its stored item, including both key sets. The item carries
/// structured principal attributes so reads never split key strings.
pub fn binding_to_item(binding: &RoleBinding) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    item.insert("PK".to_string(), s(&binding.pk()));
    item.insert("SK".to_string(), s(&binding.sk()));
    item.insert("GSI1PK".to_string(), s(&binding.gsi1pk()));
    item.insert("GSI1SK".to_string(), s(&binding.gsi1sk()));
    item.insert("itemType".to_string(), s("RoleBinding"));
    item.insert("bindingId".to_string(), s(&binding.binding_id));
    match &binding.principal {
        BindingPrincipal::CognitoGroup { issuer, group } => {
            item.insert("principalType".to_string(), s("cognito-group"));
            item.insert("issuer".to_string(), s(issuer));
            item.insert("group".to_string(), s(group));
        }
        BindingPrincipal::AwsAccount { account_id } => {
            item.insert("principalType".to_string(), s("aws-account"));
            item.insert("accountId".to_string(), s(account_id));
        }
    }
    item.insert("role".to_string(), s(binding.role.as_str()));
    item.insert("domain".to_string(), s(&binding.domain));
    item.insert("state".to_string(), s("active"));
    item.insert("createdAt".to_string(), s(&binding.created_at.to_rfc3339()));
    item.insert("createdBy".to_string(), s(&binding.created_by));
    item
}

/// Maps an audit record to its stored item. Audit items deliberately carry no
/// GSI attributes, so they stay out of the sparse administration indexes.
pub fn audit_to_item(record: &AuditRecord) -> HashMap<String, AttributeValue> {
    let mut item = HashMap::new();
    item.insert("PK".to_string(), s(&record.pk()));
    item.insert("SK".to_string(), s(&record.sk()));
    item.insert("itemType".to_string(), s("AuditRecord"));
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

fn summary_to_attr(summary: &BindingSummary) -> AttributeValue {
    let mut map = HashMap::new();
    map.insert("principal".to_string(), s(&summary.principal));
    map.insert("role".to_string(), s(summary.role.as_str()));
    map.insert("domain".to_string(), s(&summary.domain));
    map.insert("state".to_string(), s("active"));
    AttributeValue::M(map)
}

fn sdk_error(e: impl std::fmt::Debug) -> GrantStoreError {
    GrantStoreError::Dynamo(format!("{e:?}"))
}

fn map_transact_error(
    error: aws_sdk_dynamodb::error::SdkError<
        aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError,
    >,
    on_condition_failure: GrantStoreError,
) -> GrantStoreError {
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
    // Debug format: SdkError's Display is terse ("service error") and hides
    // the underlying service error kind needed to diagnose live failures.
    GrantStoreError::Dynamo(format!("{error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

    fn group_binding() -> RoleBinding {
        RoleBinding::new(
            "rb-001",
            BindingPrincipal::cognito_group(ISSUER, "team-orders").unwrap(),
            ResourceRole::Publisher,
            "orders",
            DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z")
                .unwrap()
                .to_utc(),
            "platform-admin-sub",
        )
        .unwrap()
    }

    fn account_binding() -> RoleBinding {
        RoleBinding::new(
            "rb-002",
            BindingPrincipal::aws_account("111122223333").unwrap(),
            ResourceRole::Subscriber,
            "analytics",
            DateTime::parse_from_rfc3339("2026-08-01T01:00:00Z")
                .unwrap()
                .to_utc(),
            "platform-admin-sub",
        )
        .unwrap()
    }

    #[test]
    fn owner_binding_item_round_trips() {
        let binding = RoleBinding::new(
            "rb-owner",
            BindingPrincipal::cognito_group(ISSUER, "team-orders").unwrap(),
            ResourceRole::Owner,
            "orders",
            DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z")
                .unwrap()
                .to_utc(),
            "platform-admin-sub",
        )
        .unwrap();
        let item = binding_to_item(&binding);
        assert_eq!(item_to_binding(&item).unwrap(), binding);
    }

    #[test]
    fn group_binding_item_round_trips() {
        let binding = group_binding();
        let item = binding_to_item(&binding);
        assert_eq!(item_to_binding(&item).unwrap(), binding);
    }

    #[test]
    fn account_binding_item_round_trips() {
        let binding = account_binding();
        let item = binding_to_item(&binding);
        assert_eq!(item_to_binding(&item).unwrap(), binding);
    }

    #[test]
    fn binding_item_carries_documented_keys() {
        let item = binding_to_item(&group_binding());
        assert_eq!(
            get_s(&item, "PK").unwrap(),
            format!("PRINCIPAL#cognito-group#{ISSUER}#team-orders")
        );
        assert_eq!(get_s(&item, "SK").unwrap(), "ROLE#publisher#DOMAIN#orders");
        assert_eq!(get_s(&item, "GSI1PK").unwrap(), "DOMAIN#orders");
        assert_eq!(get_s(&item, "itemType").unwrap(), "RoleBinding");
        assert_eq!(get_s(&item, "state").unwrap(), "active");
    }

    #[test]
    fn corrupt_items_are_rejected() {
        let mut item = binding_to_item(&group_binding());
        item.remove("issuer");
        assert!(matches!(
            item_to_binding(&item),
            Err(GrantStoreError::CorruptItem(_))
        ));

        let mut item = binding_to_item(&group_binding());
        item.insert("role".to_string(), s("admin"));
        assert!(matches!(
            item_to_binding(&item),
            Err(GrantStoreError::CorruptItem(_))
        ));
    }

    #[test]
    fn audit_item_carries_no_gsi_attributes() {
        let record = AuditRecord {
            binding_id: "rb-001".to_string(),
            timestamp: DateTime::parse_from_rfc3339("2026-08-01T12:00:00Z")
                .unwrap()
                .to_utc(),
            mutation: AuditMutation::Revoke,
            actor: "platform-admin-sub".to_string(),
            before: Some(BindingSummary::from(&group_binding())),
            after: None,
        };
        let item = audit_to_item(&record);
        assert_eq!(get_s(&item, "PK").unwrap(), "GRANT#rb-001#AUDIT");
        assert_eq!(get_s(&item, "itemType").unwrap(), "AuditRecord");
        assert_eq!(get_s(&item, "mutation").unwrap(), "revoke");
        assert!(!item.contains_key("GSI1PK"));
        assert!(!item.contains_key("GSI1SK"));
        assert!(!item.contains_key("bindingId"));
    }
}
