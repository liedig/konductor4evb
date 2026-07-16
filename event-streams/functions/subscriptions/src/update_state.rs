use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::env;

use crate::handler::Clients;
use crate::outcome;
use crate::validate::{self, DomainLookup};
use nexus_common::dynamodb::s;
use nexus_common::streams::{EventStream, StreamStatus, SubscriptionId};
use nexus_common::streams_db;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStateMessage {
    pub subscription_id: String,
    pub caller_account_id: String,
    pub body: UpdateStateBody,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStateBody {
    pub environments: HashMap<String, EnvDesiredState>,
}

#[derive(Debug, Deserialize)]
pub struct EnvDesiredState {
    pub enabled: bool,
}

pub async fn handle(clients: &Clients, message: &UpdateStateMessage) -> Result<()> {
    let cp_bus = env::var("NEXUS_EVENT_BUS").context("NEXUS_EVENT_BUS not set")?;
    let streams_table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;

    let emit_failed = |reason: String| {
        outcome::emit(
            &clients.eb,
            &cp_bus,
            "UpdateSubscriptionFailed",
            json!({
                "subscriptionId": message.subscription_id,
                "reason": reason,
                "failedAt": chrono::Utc::now().to_rfc3339(),
            }),
        )
    };

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

    if stream.status != StreamStatus::Active {
        return emit_failed(format!(
            "subscription '{}' is {}, not ACTIVE — only active streams can change state",
            message.subscription_id, stream.status
        ))
        .await;
    }

    for env_name in message.body.environments.keys() {
        if !stream.environments.contains_key(env_name) {
            return emit_failed(format!(
                "unknown environment '{env_name}' for subscription '{}'",
                message.subscription_id
            ))
            .await;
        }
    }

    for (env_name, desired) in &message.body.environments {
        apply_rule_state(clients, &organization, env_name, &stream, desired.enabled).await?;
        persist_env_enabled(clients, &streams_table, &stream, env_name, desired.enabled).await?;
    }

    outcome::emit(
        &clients.eb,
        &cp_bus,
        "UpdateSubscriptionSucceeded",
        json!({
            "subscriptionId": message.subscription_id,
            "environments": message.body.environments.iter()
                .map(|(env, d)| (env.clone(), json!({ "enabled": d.enabled })))
                .collect::<serde_json::Map<_, _>>(),
        }),
    )
    .await
}

/// Enablement is materialized as native rule state — an atomic flip, no
/// teardown; the contract (pinned version) is untouched.
async fn apply_rule_state(
    clients: &Clients,
    organization: &str,
    env_name: &str,
    stream: &EventStream,
    enabled: bool,
) -> Result<()> {
    let Some(env_stream) = stream.environments.get(env_name) else {
        return Ok(());
    };
    let bus_name = format!(
        "{organization}-{env_name}-{}-event-bus",
        stream.producer_domain
    );

    let result = if enabled {
        clients
            .eb
            .enable_rule()
            .name(&env_stream.rule_name)
            .event_bus_name(&bus_name)
            .send()
            .await
            .map(|_| ())
            .map_err(|e| e.into_service_error().to_string())
    } else {
        clients
            .eb
            .disable_rule()
            .name(&env_stream.rule_name)
            .event_bus_name(&bus_name)
            .send()
            .await
            .map(|_| ())
            .map_err(|e| e.into_service_error().to_string())
    };

    result.map_err(|e| nexus_common::error::ControlPlaneError::EventBridge(e).into())
}

async fn persist_env_enabled(
    clients: &Clients,
    table: &str,
    stream: &EventStream,
    env_name: &str,
    enabled: bool,
) -> Result<()> {
    clients
        .ddb
        .update_item()
        .table_name(table)
        .key(streams_db::PK, s(&stream.producer_domain))
        .key(
            streams_db::SK,
            s(&EventStream::sort_key(
                &stream.schema_name,
                &stream.consumer_domain,
            )),
        )
        .update_expression("SET environments.#env.enabled = :enabled, updatedAt = :now")
        .expression_attribute_names("#env", env_name)
        .expression_attribute_values(":enabled", AttributeValue::Bool(enabled))
        .expression_attribute_values(":now", s(&chrono::Utc::now().to_rfc3339()))
        .send()
        .await
        .context("failed to persist env enabled state")?;
    Ok(())
}
