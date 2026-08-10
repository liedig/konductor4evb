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
use nexus_common::entitlement_store::EntitlementStore;
use nexus_common::grant_engine::{actions, covers};
use nexus_common::principal::{ResourceAuthorizer, TrustedPrincipal};
use nexus_common::streams::{EventStream, StreamStatus, SubscriptionId};
use nexus_common::streams_db;
use nexus_common::subscription_resource::SubscriptionResource;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStateMessage {
    pub body: UpdateStateBody,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStateBody {
    pub subscription_id: String,
    pub consumer_domain: String,
    pub environments: HashMap<String, EnvDesiredState>,
}

#[derive(Debug, Deserialize)]
pub struct EnvDesiredState {
    pub enabled: bool,
}

enum StreamLookup {
    Ready(Box<EventStream>),
    Failed(String),
}

pub async fn handle(
    clients: &Clients,
    authorizer: &impl ResourceAuthorizer,
    message: &UpdateStateMessage,
    principal: &TrustedPrincipal,
) -> Result<()> {
    let cp_bus = env::var("NEXUS_EVENT_BUS").context("NEXUS_EVENT_BUS not set")?;
    let streams_table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let entitlements_table =
        env::var("ENTITLEMENTS_TABLE").context("ENTITLEMENTS_TABLE not set")?;
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;

    let body = &message.body;
    let subscription_id = body.subscription_id.clone();

    let emit_failed = |reason: String| {
        outcome::emit(
            &clients.eb,
            &cp_bus,
            "UpdateSubscriptionFailed",
            json!({
                "subscriptionId": subscription_id,
                "reason": reason,
                "failedAt": chrono::Utc::now().to_rfc3339(),
            }),
        )
    };

    let id = match SubscriptionId::parse(&body.subscription_id) {
        Ok(id) => id,
        Err(e) => return emit_failed(e.to_string()).await,
    };
    if id.consumer_domain != body.consumer_domain {
        return emit_failed(format!(
            "consumerDomain '{}' does not match subscription id consumer '{}'",
            body.consumer_domain, id.consumer_domain
        ))
        .await;
    }

    // Authorization precedes every existence check so a denied caller learns
    // nothing about the subscription.
    if let Err(reason) = authorize_transitions(authorizer, principal, body, &id).await {
        return emit_failed(reason).await;
    }

    let stream =
        match load_validated_stream(clients, body, &id, &streams_table, &organization).await? {
            StreamLookup::Ready(stream) => stream,
            StreamLookup::Failed(reason) => return emit_failed(reason).await,
        };

    if let Err(reason) =
        recheck_coverage_before_enable(clients, &entitlements_table, body, &id).await
    {
        return emit_failed(reason).await;
    }

    for (env_name, desired) in &body.environments {
        apply_rule_state(clients, &organization, env_name, &stream, desired.enabled).await?;
        streams_db::set_stream_env_enabled(
            &clients.ddb,
            &streams_table,
            &stream,
            env_name,
            desired.enabled,
        )
        .await?;
    }

    outcome::emit(
        &clients.eb,
        &cp_bus,
        "UpdateSubscriptionSucceeded",
        json!({
            "subscriptionId": body.subscription_id,
            "environments": body.environments.iter()
                .map(|(env, d)| (env.clone(), json!({ "enabled": d.enabled })))
                .collect::<serde_json::Map<_, _>>(),
        }),
    )
    .await
}

async fn allowed(
    authorizer: &impl ResourceAuthorizer,
    principal: &TrustedPrincipal,
    action: &str,
    resource: &str,
) -> bool {
    let decision = authorizer.authorize(principal, action, resource).await;
    if !decision.is_allowed() {
        tracing::warn!(
            action,
            resource,
            "subscription state-change authorization denied"
        );
    }
    decision.is_allowed()
}

/// Enabling any environment requires `subscription:enable` (binding plus
/// covering entitlement, engine-side); disabling requires
/// `subscription:disable` (binding only).
async fn authorize_transitions(
    authorizer: &impl ResourceAuthorizer,
    principal: &TrustedPrincipal,
    body: &UpdateStateBody,
    id: &SubscriptionId,
) -> std::result::Result<(), String> {
    let resource =
        SubscriptionResource::new(&id.consumer_domain, &id.producer_domain, &id.detail_type)
            .map_err(|e| e.to_string())?
            .to_string();

    let enabling = body.environments.values().any(|d| d.enabled);
    let disabling = body.environments.values().any(|d| !d.enabled);

    if enabling
        && !allowed(
            authorizer,
            principal,
            actions::SUBSCRIPTION_ENABLE,
            &resource,
        )
        .await
    {
        return Err("unauthorized".to_string());
    }
    if disabling
        && !allowed(
            authorizer,
            principal,
            actions::SUBSCRIPTION_DISABLE,
            &resource,
        )
        .await
    {
        return Err("unauthorized".to_string());
    }
    Ok(())
}

/// Final coverage re-check immediately before any rule flips to ENABLED:
/// narrows the revoke-between-authorize-and-enable race. Fail-closed on store
/// errors. Self-subscriptions need no entitlement and skip the check.
async fn recheck_coverage_before_enable(
    clients: &Clients,
    entitlements_table: &str,
    body: &UpdateStateBody,
    id: &SubscriptionId,
) -> std::result::Result<(), String> {
    let enabling = body.environments.values().any(|d| d.enabled);
    if !enabling || id.consumer_domain == id.producer_domain {
        return Ok(());
    }
    let store = EntitlementStore::new(clients.ddb.clone(), entitlements_table);
    let covered = match store.entitlements_for_consumer(&id.consumer_domain).await {
        Ok(current) => covers(&current, &id.producer_domain, &id.detail_type),
        Err(e) => {
            tracing::warn!(error = %e, "entitlement re-check failed closed");
            false
        }
    };
    if covered {
        Ok(())
    } else {
        Err(format!(
            "no covering entitlement for consumer '{}' on '{}'",
            id.consumer_domain, id.detail_type
        ))
    }
}

async fn load_validated_stream(
    clients: &Clients,
    body: &UpdateStateBody,
    id: &SubscriptionId,
    streams_table: &str,
    organization: &str,
) -> Result<StreamLookup> {
    match validate::lookup_active_domain(&clients.ddb, &id.consumer_domain).await? {
        DomainLookup::Found(_) => {}
        DomainLookup::NotFound => {
            return Ok(StreamLookup::Failed(format!(
                "consumer domain '{}' not found",
                id.consumer_domain
            )))
        }
        DomainLookup::NotActive(status) => {
            return Ok(StreamLookup::Failed(format!(
                "consumer domain '{}' is {status}, not ACTIVE",
                id.consumer_domain
            )))
        }
    }

    let schema_name = format!("{organization}-{}@{}", id.producer_domain, id.detail_type);
    let Some(stream) = streams_db::get_stream(
        &clients.ddb,
        streams_table,
        &id.producer_domain,
        &schema_name,
        &id.consumer_domain,
    )
    .await?
    else {
        return Ok(StreamLookup::Failed(format!(
            "subscription '{}' not found",
            body.subscription_id
        )));
    };

    if stream.status != StreamStatus::Active {
        return Ok(StreamLookup::Failed(format!(
            "subscription '{}' is {}, not ACTIVE — only active streams can change state",
            body.subscription_id, stream.status
        )));
    }

    for env_name in body.environments.keys() {
        if !stream.environments.contains_key(env_name) {
            return Ok(StreamLookup::Failed(format!(
                "unknown environment '{env_name}' for subscription '{}'",
                body.subscription_id
            )));
        }
    }

    Ok(StreamLookup::Ready(Box::new(stream)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_deserializes_flat_verb_body() {
        let message: UpdateStateMessage = serde_json::from_value(json!({
            "body": {
                "subscriptionId": "checkout.orders.OrderNotification",
                "consumerDomain": "checkout",
                "environments": { "prod": { "enabled": true } }
            }
        }))
        .unwrap();

        assert_eq!(
            message.body.subscription_id,
            "checkout.orders.OrderNotification"
        );
        assert_eq!(message.body.consumer_domain, "checkout");
        assert!(message.body.environments["prod"].enabled);
    }
}
