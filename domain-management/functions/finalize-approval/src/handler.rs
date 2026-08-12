use std::env;

use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::{Put, TransactWriteItem, Update};
use lambda_runtime::{Error, LambdaEvent};
use serde::Deserialize;
use serde_json::{json, Value};

use konductor_common::dynamodb::{get_s, s};
use konductor_common::grant::{
    AuditMutation, AuditRecord, BindingPrincipal, ResourceRole, RoleBinding,
};
use konductor_common::grant_store::{audit_to_item, binding_to_item};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FinalizeInput {
    domain_name: String,
    decided_by: String,
    #[serde(default)]
    justification: Option<String>,
}

/// Commits the approval outcome and — when the registration recorded an
/// owning team — the owner binding and its audit record in one transaction,
/// so a domain can never reach APPROVED without its ownership landing (or
/// vice versa). The domain update is conditioned on `PENDING_APPROVAL`; a
/// repeated invocation after a successful commit is treated as already
/// finalized.
pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let bindings_table = env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    let issuer = env::var("COGNITO_ISSUER").context("COGNITO_ISSUER not set")?;

    let input: FinalizeInput =
        serde_json::from_value(event.payload.clone()).context("invalid finalize payload")?;
    let now = chrono::Utc::now();

    let domain = ddb
        .get_item()
        .table_name(&domains_table)
        .key("domainName", s(&input.domain_name))
        .consistent_read(true)
        .send()
        .await
        .context("failed to read domain record")?;
    let owning_team = domain.item().and_then(|item| get_s(item, "owningTeam"));

    let approved_at = now.to_rfc3339();
    let mut items = vec![domain_approval_update(
        &domains_table,
        &input,
        &approved_at,
    )?];

    if let Some(team) = owning_team.as_deref() {
        let binding = RoleBinding::new(
            format!("rb-{}", uuid::Uuid::new_v4()),
            BindingPrincipal::cognito_group(&issuer, team)
                .map_err(|e| anyhow::anyhow!("stored owningTeam is unkeyable: {e}"))?,
            ResourceRole::Owner,
            &input.domain_name,
            now,
            &input.decided_by,
        )
        .map_err(|e| anyhow::anyhow!("cannot build owner binding: {e}"))?;
        let audit = AuditRecord {
            binding_id: binding.binding_id.clone(),
            timestamp: now,
            mutation: AuditMutation::Create,
            actor: input.decided_by.clone(),
            before: None,
            after: Some((&binding).into()),
        };
        items.push(idempotent_binding_put(&bindings_table, &binding)?);
        items.push(audit_put(&bindings_table, &audit)?);
    }

    let request = items
        .into_iter()
        .fold(ddb.transact_write_items(), |tx, item| {
            tx.transact_items(item)
        });

    match request.send().await {
        Ok(_) => {
            tracing::info!(
                domain = %input.domain_name,
                owner_binding = owning_team.is_some(),
                "registration approval finalized"
            );
        }
        Err(e) if is_condition_failure(&e) => {
            tracing::warn!(
                domain = %input.domain_name,
                "approval already finalized or domain not pending; treating as processed"
            );
        }
        Err(e) => return Err(anyhow::anyhow!("approval finalize transaction failed: {e}").into()),
    }

    Ok(json!({
        "approved": true,
        "decidedBy": input.decided_by,
        "justification": input.justification.unwrap_or_default(),
    }))
}

fn domain_approval_update(
    table: &str,
    input: &FinalizeInput,
    approved_at: &str,
) -> Result<TransactWriteItem> {
    let update = Update::builder()
        .table_name(table)
        .key("domainName", s(&input.domain_name))
        .update_expression(
            "SET #s = :status, decidedBy = :decidedBy, justification = :justification, approvedAt = :approvedAt, updatedAt = :updatedAt",
        )
        .condition_expression("#s = :pending")
        .expression_attribute_names("#s", "status")
        .expression_attribute_values(":status", s("APPROVED"))
        .expression_attribute_values(":pending", s("PENDING_APPROVAL"))
        .expression_attribute_values(":decidedBy", s(&input.decided_by))
        .expression_attribute_values(
            ":justification",
            s(input.justification.as_deref().unwrap_or("")),
        )
        .expression_attribute_values(":approvedAt", s(approved_at))
        .expression_attribute_values(":updatedAt", s(approved_at))
        .build()
        .map_err(|e| anyhow::anyhow!("invalid approval update: {e}"))?;
    Ok(TransactWriteItem::builder().update(update).build())
}

fn idempotent_binding_put(table: &str, binding: &RoleBinding) -> Result<TransactWriteItem> {
    let put = Put::builder()
        .table_name(table)
        .set_item(Some(binding_to_item(binding)))
        .condition_expression("attribute_not_exists(PK)")
        .build()
        .map_err(|e| anyhow::anyhow!("invalid binding put: {e}"))?;
    Ok(TransactWriteItem::builder().put(put).build())
}

fn audit_put(table: &str, audit: &AuditRecord) -> Result<TransactWriteItem> {
    let put = Put::builder()
        .table_name(table)
        .set_item(Some(audit_to_item(audit)))
        .build()
        .map_err(|e| anyhow::anyhow!("invalid audit write: {e}"))?;
    Ok(TransactWriteItem::builder().put(put).build())
}

/// A cancelled transaction with a condition-failure reason means this
/// approval already committed (the domain update's `PENDING_APPROVAL` guard
/// or the binding's `attribute_not_exists` tripped) — safe to treat as
/// processed. Any other cancellation reason stays an error.
fn is_condition_failure(
    e: &aws_sdk_dynamodb::error::SdkError<
        aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError,
    >,
) -> bool {
    use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
    let Some(TransactWriteItemsError::TransactionCanceledException(tce)) = e.as_service_error()
    else {
        return false;
    };
    tce.cancellation_reasons()
        .iter()
        .any(|r| r.code() == Some("ConditionalCheckFailed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::operation::transact_write_items::{
        TransactWriteItemsInput, TransactWriteItemsOutput,
    };
    use aws_smithy_mocks::{mock, mock_client, Rule};
    use lambda_runtime::Context as LambdaContext;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

    fn set_env() {
        env::set_var("DOMAINS_TABLE", "domains");
        env::set_var("BINDINGS_TABLE", "bindings");
        env::set_var("COGNITO_ISSUER", ISSUER);
    }

    fn event(domain: &str) -> LambdaEvent<Value> {
        LambdaEvent::new(
            json!({
                "domainName": domain,
                "decidedBy": "admin-subject-123",
                "justification": "looks good",
            }),
            LambdaContext::default(),
        )
    }

    fn domain_with_team(team: Option<&str>) -> GetItemOutput {
        let mut item = HashMap::from([("domainName".to_string(), s("orders"))]);
        if let Some(team) = team {
            item.insert("owningTeam".to_string(), s(team));
        }
        GetItemOutput::builder().set_item(Some(item)).build()
    }

    fn capturing_tx(captured: Arc<Mutex<Vec<TransactWriteItemsInput>>>) -> Rule {
        let seen = Arc::clone(&captured);
        mock!(aws_sdk_dynamodb::Client::transact_write_items)
            .match_requests(move |req| {
                seen.lock().unwrap().push(req.clone());
                true
            })
            .then_output(|| TransactWriteItemsOutput::builder().build())
    }

    #[tokio::test]
    async fn approval_with_owning_team_commits_domain_binding_and_audit_in_one_transaction() {
        set_env();
        let captured: Arc<Mutex<Vec<TransactWriteItemsInput>>> = Arc::new(Mutex::new(Vec::new()));
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| domain_with_team(Some("team-orders")));
        let tx_rule = capturing_tx(Arc::clone(&captured));
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule, &tx_rule]);

        let resp = handle(&ddb, event("orders")).await.unwrap();
        assert_eq!(resp["approved"], true);
        assert_eq!(resp["decidedBy"], "admin-subject-123");

        let calls = captured.lock().unwrap();
        assert_eq!(calls.len(), 1, "single transaction");
        let items = calls[0].transact_items();
        assert_eq!(items.len(), 3, "domain update + binding put + audit put");

        let update = items[0]
            .update
            .as_ref()
            .expect("first item is the domain update");
        assert_eq!(update.table_name(), "domains");
        assert_eq!(update.condition_expression(), Some("#s = :pending"));
        let values = update
            .expression_attribute_values()
            .expect("update carries attribute values");
        assert_eq!(
            values.get(":status").and_then(|v| v.as_s().ok()),
            Some(&"APPROVED".to_string())
        );
        assert_eq!(
            values.get(":decidedBy").and_then(|v| v.as_s().ok()),
            Some(&"admin-subject-123".to_string())
        );

        let put = items[1]
            .put
            .as_ref()
            .expect("second item is the binding put");
        assert_eq!(put.table_name(), "bindings");
        assert_eq!(put.condition_expression(), Some("attribute_not_exists(PK)"));
        let item = put.item();
        assert_eq!(
            get_s(item, "PK"),
            Some(format!("PRINCIPAL#cognito-group#{ISSUER}#team-orders"))
        );
        assert_eq!(get_s(item, "SK"), Some("ROLE#owner#DOMAIN#orders".into()));
        assert_eq!(get_s(item, "role"), Some("owner".into()));
        assert_eq!(get_s(item, "createdBy"), Some("admin-subject-123".into()));

        let audit = items[2].put.as_ref().expect("third item is the audit put");
        let audit_item = audit.item();
        let sk = get_s(audit_item, "SK").expect("audit SK");
        assert!(sk.starts_with("TS#"), "audit SK is time-ordered: {sk}");
        let pk = get_s(audit_item, "PK").expect("audit PK");
        assert!(
            pk.starts_with("GRANT#rb-"),
            "audit PK scopes to the binding: {pk}"
        );
        assert_eq!(get_s(audit_item, "mutation"), Some("create".into()));
        assert_eq!(get_s(audit_item, "actor"), Some("admin-subject-123".into()));
    }

    #[tokio::test]
    async fn approval_without_owning_team_commits_only_the_domain_update() {
        set_env();
        let captured: Arc<Mutex<Vec<TransactWriteItemsInput>>> = Arc::new(Mutex::new(Vec::new()));
        let get_rule =
            mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| domain_with_team(None));
        let tx_rule = capturing_tx(Arc::clone(&captured));
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule, &tx_rule]);

        let resp = handle(&ddb, event("orders")).await.unwrap();
        assert_eq!(resp["approved"], true);

        let calls = captured.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let items = calls[0].transact_items();
        assert_eq!(items.len(), 1, "no binding without an owning team");
        assert!(items[0].update.is_some());
    }

    #[tokio::test]
    async fn repeated_finalize_after_commit_is_treated_as_processed() {
        set_env();
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| domain_with_team(Some("team-orders")));
        let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items).then_error(|| {
            let reason = aws_sdk_dynamodb::types::CancellationReason::builder()
                .code("ConditionalCheckFailed")
                .build();
            aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError::TransactionCanceledException(
                aws_sdk_dynamodb::types::error::TransactionCanceledException::builder()
                    .cancellation_reasons(reason)
                    .build(),
            )
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule, &tx_rule]);

        let resp = handle(&ddb, event("orders")).await;
        assert!(
            resp.is_ok(),
            "condition failure maps to already-processed success"
        );
        assert_eq!(resp.unwrap()["approved"], true);
    }
}
