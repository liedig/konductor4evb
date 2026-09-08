use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;

use crate::handler::Clients;
use crate::outcome;
use crate::validate::{self, DomainLookup, DomainRecord};
use konductor_common::entitlement_store::EntitlementStore;
use konductor_common::grant_engine::{actions, covers};
use konductor_common::principal::{ResourceAuthorizer, TrustedPrincipal};
use konductor_common::streams::{EventStream, StreamStatus, SubscriptionId};
use konductor_common::streams_db;
use konductor_common::subscription_resource::SubscriptionResource;

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
    Ready(Box<EventStream>, DomainRecord),
    Failed(String),
}

pub async fn handle(
    clients: &Clients,
    authorizer: &impl ResourceAuthorizer,
    message: &UpdateStateMessage,
    principal: &TrustedPrincipal,
) -> Result<()> {
    let cp_bus = env::var("KONDUKTOR_EVENT_BUS").context("KONDUKTOR_EVENT_BUS not set")?;
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

    let (stream, domain) =
        match load_validated_stream(clients, body, &id, &streams_table, &organization).await? {
            StreamLookup::Ready(stream, domain) => (stream, domain),
            StreamLookup::Failed(reason) => return emit_failed(reason).await,
        };

    if let Err(reason) =
        recheck_coverage_before_enable(clients, &entitlements_table, body, &id).await
    {
        return emit_failed(reason).await;
    }

    let mut env_outcomes = serde_json::Map::new();
    for (env_name, desired) in &body.environments {
        let result = reconcile_env(
            clients,
            &organization,
            &streams_table,
            &stream,
            &domain,
            env_name,
            desired.enabled,
        )
        .await?;
        env_outcomes.insert(env_name.clone(), result);
    }

    outcome::emit(
        &clients.eb,
        &cp_bus,
        "UpdateSubscriptionSucceeded",
        json!({
            "subscriptionId": body.subscription_id,
            "environments": env_outcomes,
        }),
    )
    .await
}

/// Desired-state reconciliation per environment. Materialized entries flip
/// native rule state; entries awaiting promotion (or still pending) only
/// record the desired flag — there is no rule to flip, and promotion
/// materialization honors the flag. Enabling a FAILED entry re-invokes
/// materialization, which re-resolves the version, re-checks coverage, and
/// merges the fresh entry itself; disabling a FAILED entry only records the
/// flag (a partially created rule with no target delivers nothing).
async fn reconcile_env(
    clients: &Clients,
    organization: &str,
    streams_table: &str,
    stream: &EventStream,
    domain: &DomainRecord,
    env_name: &str,
    enabled: bool,
) -> Result<Value> {
    let entry = stream
        .environments
        .get(env_name)
        .ok_or_else(|| anyhow!("unknown environment '{env_name}'"))?;

    if entry.status == StreamStatus::Failed && enabled {
        let outcome = rematerialize(clients, stream, domain, env_name, &entry.rule_name).await?;
        let status = outcome
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("FAILED");
        return Ok(json!({ "enabled": true, "materialization": status }));
    }

    if entry.status == StreamStatus::Active {
        apply_rule_state(clients, organization, env_name, stream, enabled).await?;
    }
    streams_db::set_stream_env_enabled(&clients.ddb, streams_table, stream, env_name, enabled)
        .await?;
    Ok(json!({ "enabled": enabled }))
}

/// The repair path for a transient per-env failure: re-run the same
/// per-env materialization unit the activation workflow runs. The function
/// merges the resulting entry (including `enabled: true`) itself.
async fn rematerialize(
    clients: &Clients,
    stream: &EventStream,
    domain: &DomainRecord,
    env_name: &str,
    rule_name: &str,
) -> Result<Value> {
    let function_arn =
        env::var("MATERIALIZE_FUNCTION_ARN").context("MATERIALIZE_FUNCTION_ARN not set")?;
    let payload = json!({
        "producerDomain": stream.producer_domain,
        "consumerDomain": stream.consumer_domain,
        "consumerAccountId": domain.owner_account_id,
        "source": stream.source,
        "detailType": stream.detail_type,
        "schemaName": stream.schema_name,
        "ruleName": rule_name,
        "environment": env_name,
        "enabled": true,
    });
    let response = clients
        .lambda
        .invoke()
        .function_name(&function_arn)
        .payload(aws_sdk_lambda::primitives::Blob::new(serde_json::to_vec(
            &payload,
        )?))
        .send()
        .await
        .context("failed to invoke materialize-subscription-env")?;
    if let Some(error) = response.function_error() {
        return Err(anyhow!("materialize-subscription-env errored: {error}"));
    }
    let bytes = response.payload().map(|b| b.as_ref()).unwrap_or(b"{}");
    serde_json::from_slice(bytes).context("materialize-subscription-env returned no outcome")
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
    let domain = match validate::lookup_active_domain(&clients.ddb, &id.consumer_domain).await? {
        DomainLookup::Found(domain) => domain,
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
    };

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

    Ok(StreamLookup::Ready(Box::new(stream), domain))
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

    result.map_err(|e| konductor_common::error::ControlPlaneError::EventBridge(e).into())
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
