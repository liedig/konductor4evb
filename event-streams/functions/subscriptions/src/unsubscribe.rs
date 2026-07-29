use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::env;

use crate::handler::Clients;
use crate::outcome;
use crate::validate::{self, DomainLookup};
use nexus_common::principal::{FoundationAuthorizer, ResourceAuthorizer, TrustedPrincipal};
use nexus_common::streams::{StreamStatus, SubscriptionId};
use nexus_common::streams_db;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeMessage {
    pub subscription_id: String,
    pub caller_account_id: String,
}

const OAUTH_MUTATIONS_DISABLED: &str =
    "unauthorized: interactive OAuth mutations are not yet enabled";

pub async fn handle(
    clients: &Clients,
    message: &UnsubscribeMessage,
    principal: &TrustedPrincipal,
) -> Result<()> {
    let cp_bus = env::var("NEXUS_EVENT_BUS").context("NEXUS_EVENT_BUS not set")?;
    let streams_table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let state_machine_arn = env::var("REMOVE_SUBSCRIPTION_STATE_MACHINE_ARN")
        .context("REMOVE_SUBSCRIPTION_STATE_MACHINE_ARN not set")?;

    let emit_failed = |reason: String| {
        outcome::emit(
            &clients.eb,
            &cp_bus,
            "UnsubscribeFailed",
            json!({
                "subscriptionId": message.subscription_id,
                "reason": reason,
                "failedAt": chrono::Utc::now().to_rfc3339(),
            }),
        )
    };

    let decision = FoundationAuthorizer
        .authorize(principal, "subscriptions:Unsubscribe", &message.subscription_id)
        .await;
    if !decision.is_allowed() {
        return emit_failed(OAUTH_MUTATIONS_DISABLED.to_string()).await;
    }

    let id = match SubscriptionId::parse(&message.subscription_id) {
        Ok(id) => id,
        Err(e) => return emit_failed(e.to_string()).await,
    };

    let consumer = match validate::lookup_active_domain(&clients.ddb, &id.consumer_domain).await? {
        DomainLookup::Found(record) => record,
        DomainLookup::NotFound => {
            return emit_failed(format!(
                "consumer domain '{}' not found",
                id.consumer_domain
            ))
            .await
        }
        DomainLookup::NotActive(status) => {
            return emit_failed(format!(
                "consumer domain '{}' is {status}, not ACTIVE",
                id.consumer_domain
            ))
            .await
        }
    };

    if !validate::caller_owns_consumer(&message.caller_account_id, &consumer) {
        return emit_failed(format!(
            "caller account {} does not own consumer domain '{}'",
            message.caller_account_id, id.consumer_domain
        ))
        .await;
    }

    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let schema_name = format!("{organization}-{}@{}", id.producer_domain, id.detail_type);
    let Some(stream) = streams_db::get_stream(
        &clients.ddb,
        &streams_table,
        &id.producer_domain,
        &schema_name,
        &id.consumer_domain,
    )
    .await?
    else {
        return emit_failed(format!(
            "subscription '{}' not found",
            message.subscription_id
        ))
        .await;
    };

    if stream.status == StreamStatus::Removed {
        tracing::info!(
            subscription_id = message.subscription_id,
            "subscription already removed; idempotent no-op"
        );
        return outcome::emit(
            &clients.eb,
            &cp_bus,
            "UnsubscribeSucceeded",
            json!({"subscriptionId": message.subscription_id, "idempotent": true}),
        )
        .await;
    }

    start_removal_workflow(
        clients,
        &state_machine_arn,
        &stream,
        &message.subscription_id,
    )
    .await?;

    outcome::emit(&clients.eb, &cp_bus, "UnsubscribeSucceeded", json!({"subscriptionId": message.subscription_id, "producerDomain": stream.producer_domain, "consumerDomain": stream.consumer_domain, "schemaName": stream.schema_name})).await
}

async fn start_removal_workflow(
    clients: &Clients,
    state_machine_arn: &str,
    stream: &nexus_common::streams::EventStream,
    subscription_id: &str,
) -> Result<()> {
    let rule_name = stream
        .environments
        .values()
        .next()
        .map(|env| env.rule_name.clone())
        .unwrap_or_default();

    let workflow_input = json!({
        "producerDomain": stream.producer_domain,
        "consumerDomain": stream.consumer_domain,
        "detailType": stream.detail_type,
        "schemaName": stream.schema_name,
        "ruleName": rule_name,
        "environments": stream.environments.keys().collect::<Vec<_>>(),
    });

    clients
        .sfn
        .start_execution()
        .state_machine_arn(state_machine_arn)
        .name(format!(
            "remove-{}-{}",
            sanitize_execution_name(subscription_id),
            chrono::Utc::now().timestamp()
        ))
        .input(workflow_input.to_string())
        .send()
        .await
        .map_err(|e| {
            nexus_common::error::ControlPlaneError::StepFunctions(
                e.into_service_error().to_string(),
            )
        })?;

    Ok(())
}

fn sanitize_execution_name(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .take(60)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_name_replaces_dots_and_truncates() {
        let name = sanitize_execution_name("checkout.orders.OrderNotification");
        assert_eq!(name, "checkout-orders-OrderNotification");
        assert!(sanitize_execution_name(&"x".repeat(100)).len() <= 60);
    }
}
