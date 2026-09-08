use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::env;

use crate::handler::Clients;
use crate::outcome;
use konductor_common::dynamodb::{get_string_list, s};
use konductor_common::envelope::rule_name;
use konductor_common::grant_engine::actions;
use konductor_common::principal::{ResourceAuthorizer, TrustedPrincipal};
use konductor_common::streams::{EventStream, StreamStatus, SubscriptionId};
use konductor_common::streams_db;
use konductor_common::subscription_resource::SubscriptionResource;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeMessage {
    pub body: UnsubscribeBody,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeBody {
    pub subscription_id: String,
}

pub async fn handle(
    clients: &Clients,
    authorizer: &impl ResourceAuthorizer,
    message: &UnsubscribeMessage,
    principal: &TrustedPrincipal,
) -> Result<()> {
    let cp_bus = env::var("KONDUKTOR_EVENT_BUS").context("KONDUKTOR_EVENT_BUS not set")?;
    let streams_table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let state_machine_arn = env::var("REMOVE_SUBSCRIPTION_STATE_MACHINE_ARN")
        .context("REMOVE_SUBSCRIPTION_STATE_MACHINE_ARN not set")?;

    let subscription_id = message.body.subscription_id.clone();

    let emit_failed = |reason: String| {
        outcome::emit(
            &clients.eb,
            &cp_bus,
            "UnsubscribeFailed",
            json!({
                "subscriptionId": subscription_id,
                "reason": reason,
                "failedAt": chrono::Utc::now().to_rfc3339(),
            }),
        )
    };

    let id = match SubscriptionId::parse(&subscription_id) {
        Ok(id) => id,
        Err(e) => return emit_failed(e.to_string()).await,
    };

    let resource = match SubscriptionResource::new(
        &id.consumer_domain,
        &id.producer_domain,
        &id.detail_type,
    ) {
        Ok(resource) => resource.to_string(),
        Err(e) => return emit_failed(e.to_string()).await,
    };

    let decision = authorizer
        .authorize(principal, actions::SUBSCRIPTION_DELETE, &resource)
        .await;
    if !decision.is_allowed() {
        tracing::warn!(
            action = actions::SUBSCRIPTION_DELETE,
            subscription_id,
            "unsubscribe authorization denied"
        );
        return emit_failed("unauthorized".to_string()).await;
    }

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
        return emit_failed(format!("subscription '{subscription_id}' not found")).await;
    };

    if stream.status == StreamStatus::Removed {
        tracing::info!(
            subscription_id,
            "subscription already removed; idempotent no-op"
        );
        return outcome::emit(
            &clients.eb,
            &cp_bus,
            "UnsubscribeSucceeded",
            json!({"subscriptionId": subscription_id, "idempotent": true}),
        )
        .await;
    }

    start_removal_workflow(clients, &state_machine_arn, &stream, &subscription_id).await?;

    outcome::emit(&clients.eb, &cp_bus, "UnsubscribeSucceeded", json!({"subscriptionId": subscription_id, "producerDomain": stream.producer_domain, "consumerDomain": stream.consumer_domain, "schemaName": stream.schema_name})).await
}

async fn start_removal_workflow(
    clients: &Clients,
    state_machine_arn: &str,
    stream: &EventStream,
    subscription_id: &str,
) -> Result<()> {
    let derived = rule_name(&stream.consumer_domain, &stream.detail_type).ok();
    let rule = derived.or_else(|| {
        stream
            .environments
            .values()
            .next()
            .map(|env| env.rule_name.clone())
    });

    let Some(rule) = rule else {
        return Err(anyhow::anyhow!(
            "cannot derive the deterministic rule name for '{subscription_id}'"
        ));
    };

    // Teardown targets every producer environment, not just recorded ones:
    // a crash between rule creation and the record merge leaves an orphan
    // the record never captured. The rule name is deterministic and the
    // workflow treats not-found as a no-op, so extra attempts are harmless.
    let mut environments: Vec<String> = stream.environments.keys().cloned().collect();
    for env_name in producer_environments(clients, &stream.producer_domain).await {
        if !environments.contains(&env_name) {
            environments.push(env_name);
        }
    }
    environments.sort();

    let workflow_input = json!({
        "producerDomain": stream.producer_domain,
        "consumerDomain": stream.consumer_domain,
        "detailType": stream.detail_type,
        "schemaName": stream.schema_name,
        "ruleName": rule,
        "environments": environments,
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
            konductor_common::error::ControlPlaneError::StepFunctions(
                e.into_service_error().to_string(),
            )
        })?;

    Ok(())
}

/// Producer environments for teardown coverage. Status is irrelevant here —
/// a suspended producer's buses still hold rules to delete — so this reads
/// the record raw, and any lookup failure degrades to the recorded
/// environments rather than blocking the unsubscribe.
async fn producer_environments(clients: &Clients, producer_domain: &str) -> Vec<String> {
    let Ok(table) = env::var("DOMAINS_TABLE") else {
        return Vec::new();
    };
    let output = clients
        .ddb
        .get_item()
        .table_name(&table)
        .key("domainName", s(producer_domain))
        .send()
        .await;
    match output {
        Ok(output) => output
            .item
            .as_ref()
            .and_then(|item| get_string_list(item, "environments"))
            .unwrap_or_default(),
        Err(error) => {
            tracing::warn!(
                error = %error,
                producer_domain,
                "producer domain lookup failed; removing only recorded environments"
            );
            Vec::new()
        }
    }
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
