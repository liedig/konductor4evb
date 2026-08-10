//! Store-and-forward publish consumer: reads publish commands from SQS,
//! authorizes the trusted principal through the grant engine, resolves the
//! latest schema version in the stage-selected environment's registry, builds
//! the canonical envelope, and calls PutEvents on the domain bus.
//!
//! Retry semantics: authorization denials are deterministic and acknowledged;
//! everything else (malformed envelope, missing domain, absent schema,
//! envelope validation, PutEvents failure) is a processing failure reported
//! for SQS retry with the same immutable message attributes.

use std::env;
use std::time::Instant;

use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde::Deserialize;
use serde_json::{json, Value};

use nexus_common::dynamodb::{get_s, s};
use nexus_common::envelope::{published_at_from_sent_timestamp, EventEnvelope, SchemaName};
use nexus_common::grant_engine::actions;
use nexus_common::models::{DomainStatus, PublishRequest};
use nexus_common::namespace::NamespaceResolver;
use nexus_common::principal::{AuthorizationDecision, ResourceAuthorizer, TrustedPrincipal};
use nexus_common::principal_context::sqs_attribute_map;

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub schemas: aws_sdk_schemas::Client,
    pub eb: aws_sdk_eventbridge::Client,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: aws_sdk_dynamodb::Client::new(config),
            schemas: aws_sdk_schemas::Client::new(config),
            eb: aws_sdk_eventbridge::Client::new(config),
        }
    }
}

/// The SQS message wrapper mirrors the other write commands
/// (`callerAccountId` plus `body`); publish authorization uses the trusted
/// principal envelope, so only the body is consumed here.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishMessage {
    pub body: PublishRequest,
}

pub async fn handle<A: ResourceAuthorizer>(
    clients: &Clients,
    authorizer: &A,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let records = event
        .payload
        .get("Records")
        .and_then(|r| r.as_array())
        .context("missing Records array")?;

    let mut batch_item_failures = Vec::new();

    for record in records {
        let Some(message_id) = record.get("messageId").and_then(|v| v.as_str()) else {
            tracing::error!("skipping SQS record without messageId");
            continue;
        };

        if let Err(e) = process_record(clients, authorizer, record).await {
            tracing::error!(error = %e, message_id, "publish processing failed; reporting for retry");
            batch_item_failures.push(json!({ "itemIdentifier": message_id }));
        }
    }

    Ok(json!({ "batchItemFailures": batch_item_failures }))
}

/// Ok(()) acknowledges the record (delivered or deterministically denied);
/// Err reports it for SQS retry.
async fn process_record<A: ResourceAuthorizer>(
    clients: &Clients,
    authorizer: &A,
    record: &Value,
) -> Result<()> {
    let started = Instant::now();
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let resolver = NamespaceResolver::default_pattern();

    let parsed = parse_record(record)?;
    let mechanism = parsed.principal.authentication_mechanism();
    let request = &parsed.request;

    tracing::info!(
        metric = "PublishReceived",
        mechanism = ?mechanism,
        domain = request.domain,
        environment = parsed.environment,
        "publish command received"
    );

    if !publish_authorized(authorizer, &parsed).await {
        return Ok(());
    }

    require_active_domain(clients, &domains_table, &request.domain).await?;

    let (schema_name, schema_version) =
        resolve_schema(clients, &resolver, &organization, &parsed).await?;

    let envelope = EventEnvelope::new(
        schema_name.source.clone(),
        schema_name.detail_type.clone(),
        resolver.event_bus_name(&parsed.environment, &organization, &request.domain),
        schema_version,
        parsed.idempotency_key.clone(),
        parsed.published_at,
        request.data.clone(),
    )?;

    put_event(clients, &envelope).await?;

    tracing::info!(
        metric = "PublishDelivered",
        domain = request.domain,
        environment = parsed.environment,
        latency_ms = started.elapsed().as_secs_f64() * 1000.0,
        queue_latency_ms = (chrono::Utc::now() - parsed.published_at).num_milliseconds(),
        "event published to domain bus"
    );
    Ok(())
}

/// The authorization gate: denials (including fail-closed engine errors) are
/// deterministic, so they are logged and acknowledged rather than retried.
async fn publish_authorized<A: ResourceAuthorizer>(authorizer: &A, parsed: &ParsedRecord) -> bool {
    let decision = authorizer
        .authorize(
            &parsed.principal,
            actions::SCHEMA_PUBLISH,
            &parsed.request.domain,
        )
        .await;
    if decision == AuthorizationDecision::Allow {
        return true;
    }
    tracing::warn!(
        metric = "PublishDenied",
        mechanism = ?parsed.principal.authentication_mechanism(),
        domain = parsed.request.domain,
        action = actions::SCHEMA_PUBLISH,
        "publisher authorization denied"
    );
    false
}

struct ParsedRecord {
    principal: TrustedPrincipal,
    environment: String,
    idempotency_key: String,
    published_at: chrono::DateTime<chrono::Utc>,
    request: PublishRequest,
}

fn parse_record(record: &Value) -> Result<ParsedRecord> {
    let attributes = sqs_attribute_map(record);
    let principal = TrustedPrincipal::from_sqs_attributes(&attributes)
        .context("invalid trusted-principal envelope")?;
    let environment = required_attribute(&attributes, "Environment")?;
    let idempotency_key = required_attribute(&attributes, "IdempotencyKey")?;
    let sent_timestamp = record
        .get("attributes")
        .and_then(|a| a.get("SentTimestamp"))
        .and_then(|v| v.as_str())
        .context("missing SQS SentTimestamp attribute")?;
    let published_at = published_at_from_sent_timestamp(sent_timestamp)?;

    let body_str = record
        .get("body")
        .and_then(|b| b.as_str())
        .context("missing SQS record body")?;
    let message: PublishMessage =
        serde_json::from_str(body_str).context("invalid PublishMessage")?;

    Ok(ParsedRecord {
        principal,
        environment,
        idempotency_key,
        published_at,
        request: message.body,
    })
}

async fn resolve_schema(
    clients: &Clients,
    resolver: &NamespaceResolver,
    organization: &str,
    parsed: &ParsedRecord,
) -> Result<(SchemaName, i64)> {
    let request = &parsed.request;
    let schema_name = SchemaName::for_domain(
        &resolver.namespace(organization, &request.domain),
        &request.event_name,
    );
    let registry =
        resolver.schema_registry_name(&parsed.environment, organization, &request.domain);
    let resolve_started = Instant::now();
    let schema_version = latest_schema_version(clients, &registry, &schema_name.to_string())
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!("schema '{schema_name}' not found in registry '{registry}'")
        })?;
    tracing::info!(
        metric = "SchemaResolved",
        domain = request.domain,
        environment = parsed.environment,
        schema_version,
        latency_ms = resolve_started.elapsed().as_secs_f64() * 1000.0,
        "latest schema version resolved"
    );
    Ok((schema_name, schema_version))
}

fn required_attribute(
    attributes: &std::collections::HashMap<String, String>,
    name: &'static str,
) -> Result<String> {
    attributes
        .get(name)
        .filter(|v| !v.is_empty())
        .cloned()
        .with_context(|| format!("missing message attribute {name}"))
}

async fn require_active_domain(clients: &Clients, table: &str, domain_name: &str) -> Result<()> {
    let output = clients
        .ddb
        .get_item()
        .table_name(table)
        .key("domainName", s(domain_name))
        .send()
        .await
        .context("failed to get domain record")?;

    let Some(item) = output.item else {
        anyhow::bail!("domain '{domain_name}' not found");
    };
    let status = get_s(&item, "status").unwrap_or_default();
    if status.parse::<DomainStatus>() != Ok(DomainStatus::Active) {
        anyhow::bail!("domain '{domain_name}' is not ACTIVE (status '{status}')");
    }
    Ok(())
}

/// The latest version of a schema in one registry — DescribeSchema without a
/// version returns the latest.
async fn latest_schema_version(
    clients: &Clients,
    registry: &str,
    schema_name: &str,
) -> Result<Option<i64>> {
    match clients
        .schemas
        .describe_schema()
        .registry_name(registry)
        .schema_name(schema_name)
        .send()
        .await
    {
        Ok(output) => Ok(output
            .schema_version
            .and_then(|version| version.parse().ok())),
        Err(e) => {
            let service_err = e.into_service_error();
            if service_err.is_not_found_exception() {
                Ok(None)
            } else {
                Err(anyhow::anyhow!(
                    "failed to describe schema '{schema_name}' in '{registry}': {service_err}"
                ))
            }
        }
    }
}

async fn put_event(clients: &Clients, envelope: &EventEnvelope) -> Result<()> {
    let detail = serde_json::to_string(&envelope.detail).context("envelope not serializable")?;
    let put_started = Instant::now();
    let result = clients
        .eb
        .put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(&envelope.event_bus_name)
                .source(&envelope.source)
                .detail_type(&envelope.detail_type)
                .detail(detail)
                .build(),
        )
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("PutEvents failed: {}", e.into_service_error()))?;

    if result.failed_entry_count > 0 {
        let reason = result
            .entries()
            .first()
            .and_then(|e| e.error_message.clone())
            .unwrap_or_default();
        tracing::warn!(
            metric = "PublishPutEventsRejected",
            latency_ms = put_started.elapsed().as_secs_f64() * 1000.0,
            "EventBridge rejected the entry"
        );
        anyhow::bail!("PutEvents rejected the entry: {reason}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use nexus_common::grant::{BindingPrincipal, ResourceRole, RoleBinding};
    use nexus_common::grant_engine::InMemoryGrantEngine;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_TestPool";
    const SENT_MS: &str = "1785407400123"; // 2026-07-30T10:30:00.123Z

    fn set_env() {
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var("DOMAINS_TABLE", "domains-test");
    }

    fn publisher_binding(group: &str, domain: &str) -> RoleBinding {
        RoleBinding::new(
            "rb-1",
            BindingPrincipal::cognito_group(ISSUER, group).unwrap(),
            ResourceRole::Publisher,
            domain,
            Utc::now(),
            "test",
        )
        .unwrap()
    }

    fn iam_publisher_binding(account: &str, domain: &str) -> RoleBinding {
        RoleBinding::new(
            "rb-2",
            BindingPrincipal::aws_account(account).unwrap(),
            ResourceRole::Publisher,
            domain,
            Utc::now(),
            "test",
        )
        .unwrap()
    }

    fn oauth_attributes() -> Vec<(&'static str, String)> {
        vec![
            ("PrincipalVersion", "1".into()),
            ("AuthMechanism", "oauth".into()),
            ("PrincipalId", "sub-123".into()),
            ("Issuer", ISSUER.into()),
            ("ClientId", "client-1".into()),
            ("GlobalRoles", "[]".into()),
            ("TeamGroups", "[\"team-orders\"]".into()),
            ("Environment", "prod".into()),
            ("IdempotencyKey", "key-1".into()),
        ]
    }

    fn iam_attributes() -> Vec<(&'static str, String)> {
        vec![
            ("PrincipalVersion", "1".into()),
            ("AuthMechanism", "iam".into()),
            ("AccountId", "111122223333".into()),
            (
                "PrincipalArn",
                "arn:aws:iam::111122223333:role/publisher".into(),
            ),
            ("Environment", "prod".into()),
            ("IdempotencyKey", "key-1".into()),
        ]
    }

    fn record(attributes: Vec<(&'static str, String)>) -> Value {
        let message_attributes: serde_json::Map<String, Value> = attributes
            .into_iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    json!({"stringValue": v, "dataType": "String"}),
                )
            })
            .collect();
        json!({
            "messageId": "msg-1",
            "body": json!({
                "callerAccountId": "111122223333",
                "body": {
                    "domain": "orders",
                    "eventName": "OrderNotification",
                    "data": {"orderId": 123, "amount": 99.5}
                }
            })
            .to_string(),
            "messageAttributes": Value::Object(message_attributes),
            "attributes": {"SentTimestamp": SENT_MS}
        })
    }

    fn sqs_event(records: Vec<Value>) -> LambdaEvent<Value> {
        LambdaEvent::new(json!({"Records": records}), Default::default())
    }

    fn failures(response: &Value) -> Vec<&str> {
        response["batchItemFailures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["itemIdentifier"].as_str().unwrap())
            .collect()
    }

    fn active_domain_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.table_name() == Some("domains-test")
                    && req.key().and_then(|k| k.get("domainName")) == Some(&s("orders"))
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .item("domainName", s("orders"))
                    .item("status", s("ACTIVE"))
                    .build()
            })
    }

    fn schema_rule() -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema)
            .match_requests(|req| {
                req.registry_name() == Some("acme-prod-orders-schema-registry")
                    && req.schema_name() == Some("acme-orders@OrderNotification")
            })
            .then_output(|| {
                aws_sdk_schemas::operation::describe_schema::DescribeSchemaOutput::builder()
                    .schema_version("4")
                    .build()
            })
    }

    fn put_events_rule() -> Rule {
        mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(|req| {
                let entry = &req.entries()[0];
                if entry.event_bus_name() != Some("acme-prod-orders-event-bus")
                    || entry.source() != Some("acme-orders")
                    || entry.detail_type() != Some("OrderNotification")
                {
                    return false;
                }
                let detail: Value = serde_json::from_str(entry.detail().unwrap()).unwrap();
                detail["metadata"]["schemaVersion"] == json!(4)
                    && detail["metadata"]["idempotencyKey"] == json!("key-1")
                    && detail["metadata"]["publishedAt"] == json!("2026-07-30T10:30:00.123Z")
                    && detail["data"] == json!({"orderId": 123, "amount": 99.5})
            })
            .then_output(|| {
                aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                    .failed_entry_count(0)
                    .entries(
                        aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
                            .event_id("evt-1")
                            .build(),
                    )
                    .build()
            })
    }

    fn clients(ddb_rules: Vec<Rule>, schemas_rules: Vec<Rule>, eb_rules: Vec<Rule>) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, &ddb_rules),
            schemas: mock_client!(aws_sdk_schemas, RuleMode::MatchAny, &schemas_rules),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, &eb_rules),
        }
    }

    #[tokio::test]
    async fn oauth_publisher_binding_delivers_the_canonical_envelope() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(
            vec![active_domain_rule()],
            vec![schema_rule()],
            vec![put_events_rule()],
        );

        let response = handle(
            &clients,
            &authorizer,
            sqs_event(vec![record(oauth_attributes())]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
    }

    #[tokio::test]
    async fn iam_publisher_binding_delivers() {
        set_env();
        let authorizer =
            InMemoryGrantEngine::new(vec![iam_publisher_binding("111122223333", "orders")]);
        let clients = clients(
            vec![active_domain_rule()],
            vec![schema_rule()],
            vec![put_events_rule()],
        );

        let response = handle(
            &clients,
            &authorizer,
            sqs_event(vec![record(iam_attributes())]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty());
    }

    #[tokio::test]
    async fn denied_publish_is_acknowledged_without_any_resource_access() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![]);
        // Zero rules on every client: any DDB, Schemas, or EventBridge call
        // panics, proving authorization precedes resource access.
        let clients = clients(vec![], vec![], vec![]);

        let response = handle(
            &clients,
            &authorizer,
            sqs_event(vec![record(oauth_attributes())]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty(), "denial is acknowledged");
    }

    #[tokio::test]
    async fn binding_for_another_domain_is_denied() {
        set_env();
        let authorizer =
            InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "payments")]);
        let clients = clients(vec![], vec![], vec![]);

        let response = handle(
            &clients,
            &authorizer,
            sqs_event(vec![record(oauth_attributes())]),
        )
        .await
        .unwrap();

        assert!(failures(&response).is_empty(), "denial is acknowledged");
    }

    #[tokio::test]
    async fn malformed_principal_is_reported_for_retry() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![]);
        let clients = clients(vec![], vec![], vec![]);
        let broken: Vec<(&'static str, String)> = oauth_attributes()
            .into_iter()
            .filter(|(k, _)| *k != "AuthMechanism")
            .collect();

        let response = handle(&clients, &authorizer, sqs_event(vec![record(broken)]))
            .await
            .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }

    #[tokio::test]
    async fn put_events_api_error_is_reported_for_retry() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let eb_error = mock!(aws_sdk_eventbridge::Client::put_events).then_error(|| {
            aws_sdk_eventbridge::operation::put_events::PutEventsError::generic(
                aws_sdk_eventbridge::error::ErrorMetadata::builder()
                    .message("throttled")
                    .build(),
            )
        });
        let clients = clients(
            vec![active_domain_rule()],
            vec![schema_rule()],
            vec![eb_error],
        );

        let response = handle(
            &clients,
            &authorizer,
            sqs_event(vec![record(oauth_attributes())]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }

    #[tokio::test]
    async fn put_events_failed_entry_is_reported_for_retry() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let failed_entry = mock!(aws_sdk_eventbridge::Client::put_events).then_output(|| {
            aws_sdk_eventbridge::operation::put_events::PutEventsOutput::builder()
                .failed_entry_count(1)
                .entries(
                    aws_sdk_eventbridge::types::PutEventsResultEntry::builder()
                        .error_code("InternalFailure")
                        .error_message("boom")
                        .build(),
                )
                .build()
        });
        let clients = clients(
            vec![active_domain_rule()],
            vec![schema_rule()],
            vec![failed_entry],
        );

        let response = handle(
            &clients,
            &authorizer,
            sqs_event(vec![record(oauth_attributes())]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }

    #[tokio::test]
    async fn absent_schema_is_reported_for_retry_without_put_events() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let not_found = mock!(aws_sdk_schemas::Client::describe_schema).then_error(|| {
            aws_sdk_schemas::operation::describe_schema::DescribeSchemaError::NotFoundException(
                aws_sdk_schemas::types::error::NotFoundException::builder()
                    .message("not found")
                    .build(),
            )
        });
        // No EventBridge rule: a PutEvents call would panic.
        let clients = clients(vec![active_domain_rule()], vec![not_found], vec![]);

        let response = handle(
            &clients,
            &authorizer,
            sqs_event(vec![record(oauth_attributes())]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }

    #[tokio::test]
    async fn inactive_domain_is_reported_for_retry_without_schema_or_bus_access() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let inactive = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("domainName", s("orders"))
                .item("status", s("PROVISIONING"))
                .build()
        });
        let clients = clients(vec![inactive], vec![], vec![]);

        let response = handle(
            &clients,
            &authorizer,
            sqs_event(vec![record(oauth_attributes())]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }

    #[tokio::test]
    async fn missing_domain_is_reported_for_retry() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let absent = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build()
        });
        let clients = clients(vec![absent], vec![], vec![]);

        let response = handle(
            &clients,
            &authorizer,
            sqs_event(vec![record(oauth_attributes())]),
        )
        .await
        .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }

    #[tokio::test]
    async fn grant_engine_store_error_fails_closed_and_is_acknowledged() {
        set_env();
        let store_error = mock!(aws_sdk_dynamodb::Client::get_item).then_error(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemError::generic(
                aws_sdk_dynamodb::error::ErrorMetadata::builder()
                    .message("simulated bindings store outage")
                    .build(),
            )
        });
        let store_client = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&store_error]);
        let authorizer = nexus_common::grant_engine::DynamoGrantEngine::new(
            nexus_common::grant_store::BindingStore::new(store_client.clone(), "bindings-test"),
            nexus_common::entitlement_store::EntitlementStore::new(store_client, "bindings-test"),
        );
        // Zero rules: the fail-closed denial must precede any resource access.
        let clients = clients(vec![], vec![], vec![]);

        let response = handle(
            &clients,
            &authorizer,
            sqs_event(vec![record(oauth_attributes())]),
        )
        .await
        .unwrap();

        assert!(
            failures(&response).is_empty(),
            "fail-closed denial is acknowledged"
        );
    }

    #[tokio::test]
    async fn missing_environment_attribute_is_reported_for_retry() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![]);
        let clients = clients(vec![], vec![], vec![]);
        let broken: Vec<(&'static str, String)> = oauth_attributes()
            .into_iter()
            .filter(|(k, _)| *k != "Environment")
            .collect();

        let response = handle(&clients, &authorizer, sqs_event(vec![record(broken)]))
            .await
            .unwrap();

        assert_eq!(failures(&response), ["msg-1"]);
    }
}
