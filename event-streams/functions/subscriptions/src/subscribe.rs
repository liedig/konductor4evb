use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::env;

use crate::handler::Clients;
use crate::outcome;
use crate::validate::{self, DomainLookup};
use nexus_common::envelope::{rule_name, SchemaName};
use nexus_common::principal::{FoundationAuthorizer, ResourceAuthorizer, TrustedPrincipal};
use nexus_common::streams::{EnvStream, EventStream, StreamStatus};
use nexus_common::streams_db;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeMessage {
    pub caller_account_id: String,
    pub body: SubscribeBody,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeBody {
    pub consumer_domain: String,
    pub producer_domain: String,
    pub schema_name: String,
    #[serde(default)]
    pub environments: HashMap<String, EnvDesiredState>,
}

#[derive(Debug, Deserialize)]
pub struct EnvDesiredState {
    pub enabled: bool,
}

const OAUTH_MUTATIONS_DISABLED: &str =
    "unauthorized: interactive OAuth mutations are not yet enabled";

pub async fn handle(
    clients: &Clients,
    message: &SubscribeMessage,
    principal: &TrustedPrincipal,
    request_id: &str,
) -> Result<()> {
    let cp_bus = env::var("NEXUS_EVENT_BUS").context("NEXUS_EVENT_BUS not set")?;
    let streams_table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let state_machine_arn = env::var("ACTIVATE_SUBSCRIPTION_STATE_MACHINE_ARN")
        .context("ACTIVATE_SUBSCRIPTION_STATE_MACHINE_ARN not set")?;

    let body = &message.body;
    let subscription_id = format!(
        "{}.{}.{}",
        body.consumer_domain,
        body.producer_domain,
        schema_detail_type(&body.schema_name)
    );

    let decision = FoundationAuthorizer
        .authorize(principal, "subscriptions:Subscribe", &subscription_id)
        .await;
    if !decision.is_allowed() {
        return outcome::emit(&clients.eb, &cp_bus, "SubscribeFailed", json!({"subscriptionId": subscription_id, "consumerDomain": body.consumer_domain, "producerDomain": body.producer_domain, "schemaName": body.schema_name, "reason": OAUTH_MUTATIONS_DISABLED, "failedAt": chrono::Utc::now().to_rfc3339()})).await;
    }

    let validated = match validate_request(clients, message, &organization).await? {
        Ok(validated) => validated,
        Err(reason) => {
            tracing::warn!(subscription_id, reason, "subscribe rejected by validation");
            return outcome::emit(&clients.eb, &cp_bus, "SubscribeFailed", json!({"subscriptionId": subscription_id, "consumerDomain": body.consumer_domain, "producerDomain": body.producer_domain, "schemaName": body.schema_name, "reason": reason, "failedAt": chrono::Utc::now().to_rfc3339()})).await;
        }
    };
    let Validated {
        producer,
        consumer,
        schema,
        rule_name: derived_rule_name,
    } = validated;

    if is_idempotent_noop(clients, &streams_table, body, &cp_bus, &subscription_id).await? {
        return Ok(());
    }

    let stream = build_pending_stream(
        body,
        &schema,
        &derived_rule_name,
        &producer.environments,
        request_id,
    );
    streams_db::put_stream(&clients.ddb, &streams_table, &stream).await?;

    let validated = Validated {
        producer,
        consumer,
        schema,
        rule_name: derived_rule_name,
    };
    start_activation_workflow(clients, &state_machine_arn, &validated, body, request_id).await?;

    outcome::emit(&clients.eb, &cp_bus, "SubscribeSucceeded", json!({"subscriptionId": subscription_id, "consumerDomain": body.consumer_domain, "producerDomain": body.producer_domain, "schemaName": body.schema_name, "status": StreamStatus::Pending, "requestId": request_id})).await
}

async fn is_idempotent_noop(
    clients: &Clients,
    table: &str,
    body: &SubscribeBody,
    cp_bus: &str,
    subscription_id: &str,
) -> Result<bool> {
    if let Some(existing) =
        streams_db::get_stream(&clients.ddb, table, &body.producer_domain, &body.schema_name, &body.consumer_domain)
            .await?
    {
        if matches!(existing.status, StreamStatus::Active | StreamStatus::Pending) {
            tracing::info!(subscription_id, "subscription already exists; idempotent no-op");
            outcome::emit(
                &clients.eb,
                cp_bus,
                "SubscribeSucceeded",
                json!({"subscriptionId": subscription_id, "consumerDomain": body.consumer_domain, "producerDomain": body.producer_domain, "schemaName": body.schema_name, "status": existing.status, "idempotent": true}),
            )
            .await?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn build_pending_stream(
    body: &SubscribeBody,
    schema: &SchemaName,
    derived_rule_name: &str,
    environments: &[String],
    request_id: &str,
) -> EventStream {
    let now = chrono::Utc::now();
    let env_map: HashMap<String, EnvStream> = environments
        .iter()
        .map(|env_name| {
            let enabled = body
                .environments
                .get(env_name)
                .map(|s| s.enabled)
                .unwrap_or(true);
            (
                env_name.clone(),
                EnvStream {
                    pinned_version: None,
                    rule_name: derived_rule_name.to_string(),
                    rule_arn: None,
                    enabled,
                    status: StreamStatus::Pending,
                },
            )
        })
        .collect();

    EventStream {
        producer_domain: body.producer_domain.clone(),
        consumer_domain: body.consumer_domain.clone(),
        source: schema.source.clone(),
        detail_type: schema.detail_type.clone(),
        schema_name: body.schema_name.clone(),
        status: StreamStatus::Pending,
        environments: env_map,
        request_id: Some(request_id.to_string()),
        created_at: now,
        updated_at: now,
        removed_at: None,
        expires_at: None,
    }
}

async fn start_activation_workflow(
    clients: &Clients,
    state_machine_arn: &str,
    validated: &Validated,
    body: &SubscribeBody,
    request_id: &str,
) -> Result<()> {
    let workflow_input = json!({
        "producerDomain": body.producer_domain,
        "consumerDomain": body.consumer_domain,
        "consumerAccountId": validated.consumer.owner_account_id,
        "source": validated.schema.source,
        "detailType": validated.schema.detail_type,
        "schemaName": body.schema_name,
        "ruleName": validated.rule_name,
        "environments": validated.producer.environments.iter().map(|env_name| json!({"name": env_name, "enabled": body.environments.get(env_name).map(|s| s.enabled).unwrap_or(true)})).collect::<Vec<_>>(),
    });

    clients
        .sfn
        .start_execution()
        .state_machine_arn(state_machine_arn)
        .name(format!("activate-{request_id}"))
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

struct Validated {
    producer: validate::DomainRecord,
    consumer: validate::DomainRecord,
    schema: SchemaName,
    rule_name: String,
}

impl std::fmt::Debug for Validated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Validated")
            .field("schema", &self.schema)
            .field("rule_name", &self.rule_name)
            .finish_non_exhaustive()
    }
}

/// Deterministic business validation. `Ok(Err(reason))` means the command is
/// rejected — the caller emits `SubscribeFailed` and acknowledges the message.
async fn validate_request(
    clients: &Clients,
    message: &SubscribeMessage,
    organization: &str,
) -> Result<std::result::Result<Validated, String>> {
    let body = &message.body;

    let schema = match SchemaName::parse(&body.schema_name) {
        Ok(schema) => schema,
        Err(e) => return Ok(Err(e.to_string())),
    };

    let expected_source = format!("{organization}-{}", body.producer_domain);
    if schema.source != expected_source {
        return Ok(Err(format!(
            "schema source '{}' does not match producer domain identity '{expected_source}'",
            schema.source
        )));
    }

    let derived_rule_name = match rule_name(&body.consumer_domain, &schema.detail_type) {
        Ok(name) => name,
        Err(e) => return Ok(Err(e.to_string())),
    };

    let producer = match validate::lookup_active_domain(&clients.ddb, &body.producer_domain).await?
    {
        DomainLookup::Found(record) => record,
        DomainLookup::NotFound => {
            return Ok(Err(format!(
                "producer domain '{}' not found",
                body.producer_domain
            )))
        }
        DomainLookup::NotActive(status) => {
            return Ok(Err(format!(
                "producer domain '{}' is {status}, not ACTIVE",
                body.producer_domain
            )))
        }
    };

    let consumer = match validate::lookup_active_domain(&clients.ddb, &body.consumer_domain).await?
    {
        DomainLookup::Found(record) => record,
        DomainLookup::NotFound => {
            return Ok(Err(format!(
                "consumer domain '{}' not found",
                body.consumer_domain
            )))
        }
        DomainLookup::NotActive(status) => {
            return Ok(Err(format!(
                "consumer domain '{}' is {status}, not ACTIVE",
                body.consumer_domain
            )))
        }
    };

    if !validate::caller_owns_consumer(&message.caller_account_id, &consumer) {
        return Ok(Err(format!(
            "caller account {} does not own consumer domain '{}'",
            message.caller_account_id, body.consumer_domain
        )));
    }

    for env_name in body.environments.keys() {
        if !producer.environments.contains(env_name) {
            return Ok(Err(format!(
                "unknown environment '{env_name}' — producer domain has {:?}",
                producer.environments
            )));
        }
    }

    Ok(Ok(Validated {
        producer,
        consumer,
        schema,
        rule_name: derived_rule_name,
    }))
}

fn schema_detail_type(schema_name: &str) -> &str {
    schema_name
        .split_once('@')
        .map(|(_, detail_type)| detail_type)
        .unwrap_or(schema_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client, RuleMode};
    use nexus_common::dynamodb::s;

    fn set_env() {
        std::env::set_var("DOMAINS_TABLE", "domains");
        std::env::set_var("EVENT_STREAMS_TABLE", "streams");
        std::env::set_var("NEXUS_EVENT_BUS", "nexus-bus");
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var(
            "ACTIVATE_SUBSCRIPTION_STATE_MACHINE_ARN",
            "arn:aws:states:us-east-1:123:stateMachine:activate",
        );
    }

    fn domain_item(name: &str, owner: &str, status: &str) -> GetItemOutput {
        GetItemOutput::builder()
            .item("domainName", s(name))
            .item("ownerAccountId", s(owner))
            .item("status", s(status))
            .item("environments", AttributeValue::L(vec![s("dev"), s("prod")]))
            .build()
    }

    fn message(consumer: &str, producer: &str, schema: &str, caller: &str) -> SubscribeMessage {
        SubscribeMessage {
            caller_account_id: caller.to_string(),
            body: SubscribeBody {
                consumer_domain: consumer.to_string(),
                producer_domain: producer.to_string(),
                schema_name: schema.to_string(),
                environments: HashMap::new(),
            },
        }
    }

    fn clients_with_domains(producer_owner: &str, consumer_owner: &str) -> Clients {
        let producer_owner = producer_owner.to_string();
        let consumer_owner = consumer_owner.to_string();
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.key().unwrap().get("domainName").is_some())
            .then_output(move || domain_item("orders", &producer_owner, "ACTIVE"));
        let consumer_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.key()
                    .unwrap()
                    .get("domainName")
                    .and_then(|v| v.as_s().ok())
                    .map(|v| v == "checkout")
                    .unwrap_or(false)
            })
            .then_output(move || domain_item("checkout", &consumer_owner, "ACTIVE"));
        Clients {
            ddb: mock_client!(
                aws_sdk_dynamodb,
                RuleMode::MatchAny,
                [&consumer_rule, &get_rule]
            ),
            eb: mock_client!(aws_sdk_eventbridge, []),
            sfn: mock_client!(aws_sdk_sfn, []),
        }
    }

    #[tokio::test]
    async fn caller_not_owning_consumer_is_rejected() {
        set_env();
        let clients = clients_with_domains("111122223333", "444455556666");
        let msg = message(
            "checkout",
            "orders",
            "acme-orders@OrderNotification",
            "999999999999",
        );

        let verdict = validate_request(&clients, &msg, "acme").await.unwrap();

        assert!(verdict
            .unwrap_err()
            .contains("does not own consumer domain"));
    }

    #[tokio::test]
    async fn caller_owning_consumer_passes_ownership_check() {
        set_env();
        let clients = clients_with_domains("111122223333", "444455556666");
        let msg = message(
            "checkout",
            "orders",
            "acme-orders@OrderNotification",
            "444455556666",
        );

        let verdict = validate_request(&clients, &msg, "acme").await.unwrap();

        assert!(verdict.is_ok());
    }

    #[tokio::test]
    async fn self_subscription_is_permitted() {
        set_env();
        let owner = "444455556666";
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(move || domain_item("checkout", owner, "ACTIVE"));
        let clients = Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&get_rule]),
            eb: mock_client!(aws_sdk_eventbridge, []),
            sfn: mock_client!(aws_sdk_sfn, []),
        };
        let msg = message("checkout", "checkout", "acme-checkout@CartAbandoned", owner);

        let verdict = validate_request(&clients, &msg, "acme").await.unwrap();

        assert!(verdict.is_ok());
    }

    #[tokio::test]
    async fn schema_source_mismatching_producer_identity_is_rejected() {
        set_env();
        let clients = clients_with_domains("111122223333", "444455556666");
        let msg = message(
            "checkout",
            "orders",
            "acme-billing@OrderNotification",
            "444455556666",
        );

        let verdict = validate_request(&clients, &msg, "acme").await.unwrap();

        assert!(verdict
            .unwrap_err()
            .contains("does not match producer domain identity"));
    }

    #[tokio::test]
    async fn malformed_schema_name_is_rejected() {
        set_env();
        let clients = clients_with_domains("111122223333", "444455556666");
        let msg = message("checkout", "orders", "no-at-sign", "444455556666");

        let verdict = validate_request(&clients, &msg, "acme").await.unwrap();

        assert!(verdict.unwrap_err().contains("must be"));
    }

    #[tokio::test]
    async fn rule_name_over_budget_is_rejected() {
        set_env();
        let clients = clients_with_domains("111122223333", "444455556666");
        let long_event = "E".repeat(80);
        let msg = message(
            "checkout",
            "orders",
            &format!("acme-orders@{long_event}"),
            "444455556666",
        );

        let verdict = validate_request(&clients, &msg, "acme").await.unwrap();

        assert!(verdict.unwrap_err().contains("at most 64"));
    }

    #[tokio::test]
    async fn unknown_environment_override_is_rejected() {
        set_env();
        let clients = clients_with_domains("111122223333", "444455556666");
        let mut msg = message(
            "checkout",
            "orders",
            "acme-orders@OrderNotification",
            "444455556666",
        );
        msg.body
            .environments
            .insert("qa".to_string(), EnvDesiredState { enabled: false });

        let verdict = validate_request(&clients, &msg, "acme").await.unwrap();

        assert!(verdict.unwrap_err().contains("unknown environment 'qa'"));
    }

    #[test]
    fn schema_detail_type_extracts_after_at() {
        assert_eq!(
            schema_detail_type("acme-orders@OrderNotification"),
            "OrderNotification"
        );
    }
}
