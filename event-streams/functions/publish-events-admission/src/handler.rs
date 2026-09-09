//! Publish admission edge: the synchronous front door for `POST /publish`.
//!
//! Replaces the direct API Gateway → SQS integration. Each request is
//! authorized (publisher binding via the grant engine), checked against an
//! ACTIVE domain, resolved to the latest schema version in the stage-selected
//! environment's registry (through a per-instance TTL cache), and validated
//! against that draft-4 document — all before the immutable command is
//! enqueued for the delivery-only `publish-events` consumer.
//!
//! Outcomes: 202 once the command is durably queued (the new durability
//! boundary), 400 for malformed requests or envelopes that could never be
//! delivered (over EventBridge's entry size limit), 403 for missing publisher
//! binding, 404 for unknown/inactive domain or unregistered schema, 422 for
//! payload conformance failures, 503 for platform-side failures (domain store,
//! registry, unusable stored schema, enqueue). A 422 triggers a forced cache
//! refresh before rejecting, so a publish racing a schema append is never
//! rejected against a definition the caller could not have known.

use std::env;
use std::time::Instant;

use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use konductor_common::api;
use konductor_common::dynamodb::{get_s, s};
use konductor_common::envelope::{EventEnvelope, SchemaName};
use konductor_common::error::ControlPlaneError;
use konductor_common::grant_engine::actions;
use konductor_common::metadata::admission::MetadataAdmission;
use konductor_common::models::{ApiError, DomainStatus, PublishRequest};
use konductor_common::namespace::NamespaceResolver;
use konductor_common::principal::{AuthorizationDecision, ResourceAuthorizer, TrustedPrincipal};
use konductor_common::principal_context::log_principal;
use konductor_common::schema_validation::{
    validate_against_latest, LatestSchema, SchemaCache, SchemaCacheError, ValidationError,
};

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub schemas: aws_sdk_schemas::Client,
    pub sqs: aws_sdk_sqs::Client,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: aws_sdk_dynamodb::Client::new(config),
            schemas: aws_sdk_schemas::Client::new(config),
            sqs: aws_sdk_sqs::Client::new(config),
        }
    }
}

pub async fn handle<A: ResourceAuthorizer>(
    clients: &Clients,
    authorizer: &A,
    cache: &SchemaCache,
    metadata: &MetadataAdmission,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let payload = &event.payload;
    let request_context = payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(_) => {
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ))
        }
    };

    let method = payload
        .get("httpMethod")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let resource = payload
        .get("resource")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match (method, resource) {
        ("POST", "/publish") => {
            admit(clients, authorizer, cache, metadata, payload, &principal).await
        }
        _ => Ok(api::from_control_plane_error(
            &ControlPlaneError::SchemaNotFound {
                schema_name: format!("{method} {resource}"),
            },
        )),
    }
}

async fn admit<A: ResourceAuthorizer>(
    clients: &Clients,
    authorizer: &A,
    cache: &SchemaCache,
    metadata: &MetadataAdmission,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let started = Instant::now();
    let (request, raw_body) = match parse_publish_request(payload) {
        Ok(parsed) => parsed,
        Err(response) => return Ok(response),
    };

    tracing::info!(
        metric = "AdmissionReceived",
        mechanism = ?principal.authentication_mechanism(),
        domain = request.domain,
        "publish admission request received"
    );

    if let Err(response) = authorize_publish(clients, authorizer, &request, principal).await {
        return Ok(response);
    }

    let stage = match stage_of(payload) {
        Ok(s) => s,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    if let Err(response) = resolve_metadata(metadata, &stage).await {
        return Ok(response);
    }
    let organization = match env::var("ORGANIZATION") {
        Ok(o) => o,
        Err(_) => return Ok(internal_error("ORGANIZATION not set")),
    };
    let resolver = NamespaceResolver::default_pattern();
    let schema_name = SchemaName::for_domain(
        &resolver.namespace(&organization, &request.domain),
        &request.event_name,
    );
    let registry = resolver.schema_registry_name(&stage, &organization, &request.domain);

    let latest =
        match validated_latest_schema(clients, cache, &registry, &schema_name, &request).await {
            Ok(latest) => latest,
            Err(response) => return Ok(response),
        };

    let idempotency_key = request
        .idempotency_key
        .clone()
        .filter(|k| !k.is_empty())
        .or_else(|| {
            payload
                .pointer("/requestContext/requestId")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_else(uuid_v4);

    if let Err(response) = ensure_deliverable_envelope(
        &resolver,
        &stage,
        &organization,
        &request,
        &schema_name,
        latest.version,
        &idempotency_key,
    ) {
        return Ok(response);
    }

    let command = AdmittedCommand {
        request: &request,
        raw_body: &raw_body,
        stage: &stage,
        schema_version: latest.version,
        idempotency_key,
        caller_account_id: payload
            .pointer("/requestContext/identity/accountId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    };

    Ok(queue_admitted(clients, principal, &command, started).await)
}

/// Resolves the stage environment's metadata selection. Authorization
/// has already passed; the pointer read is strongly consistent and fresh
/// on every publish, and a missing or unreadable pointer is a 503 — never
/// a stale-pointer fallback.
async fn resolve_metadata(
    metadata: &MetadataAdmission,
    stage: &str,
) -> Result<Option<konductor_common::metadata::admission::MetadataSelection>, Value> {
    match metadata.resolve(stage).await {
        Ok(selection) => {
            if let Some(selection) = &selection {
                tracing::info!(
                    metric = "MetadataSelection",
                    environment = %stage,
                    revision = selection.active_revision,
                    generation = selection.generation,
                    "resolved active metadata revision"
                );
            }
            Ok(selection)
        }
        Err(error) => {
            tracing::error!(
                metric = "PublishUnavailable",
                environment = %stage,
                error = %error,
                "metadata configuration unavailable"
            );
            Err(temporarily_unavailable())
        }
    }
}

/// Parses and deserializes the publish request; anything malformed is a 400.
fn parse_publish_request(payload: &Value) -> Result<(PublishRequest, Value), Value> {
    let raw_body: Value =
        api::parse_body(payload).map_err(|e| api::from_control_plane_error(&e))?;
    let request: PublishRequest = serde_json::from_value(raw_body.clone()).map_err(|e| {
        api::from_control_plane_error(&api::parse_error(format!("Invalid request body: {e}")))
    })?;
    Ok((request, raw_body))
}

/// Publisher binding plus domain liveness: 403 without a binding, 404 for an
/// unknown/inactive domain. A domain-store outage is a platform-side
/// evaluation failure, not an answer about the domain: retryable 503, never
/// a 404/500.
async fn authorize_publish<A: ResourceAuthorizer>(
    clients: &Clients,
    authorizer: &A,
    request: &PublishRequest,
    principal: &TrustedPrincipal,
) -> Result<(), Value> {
    if authorizer
        .authorize(principal, actions::SCHEMA_PUBLISH, &request.domain)
        .await
        != AuthorizationDecision::Allow
    {
        tracing::warn!(
            metric = "AdmissionDenied",
            mechanism = ?principal.authentication_mechanism(),
            domain = request.domain,
            action = actions::SCHEMA_PUBLISH,
            "publisher authorization denied"
        );
        return Err(api::from_control_plane_error(
            &ControlPlaneError::AccessDenied,
        ));
    }

    match require_active_domain(clients, &request.domain).await {
        Ok(()) => Ok(()),
        Err(ControlPlaneError::DynamoDb(detail)) => {
            tracing::error!(
                metric = "PublishUnavailable",
                domain = request.domain,
                error = detail,
                "domain store unavailable"
            );
            Err(temporarily_unavailable())
        }
        Err(other) => Err(api::from_control_plane_error(&other)),
    }
}

/// A publish that can never be delivered must not be admitted: build the
/// envelope now so structural or size violations answer 400 here instead of
/// a 202 that retries deterministically into the DLQ. The placeholder
/// timestamp stands in for the queue-stamped SentTimestamp; the
/// ISO-8601-millis format is fixed-width, so the size check is exact.
#[allow(clippy::too_many_arguments)]
fn ensure_deliverable_envelope(
    resolver: &NamespaceResolver,
    stage: &str,
    organization: &str,
    request: &PublishRequest,
    schema_name: &SchemaName,
    schema_version: i64,
    idempotency_key: &str,
) -> Result<(), Value> {
    EventEnvelope::new(
        schema_name.source.clone(),
        schema_name.detail_type.clone(),
        resolver.event_bus_name(stage, organization, &request.domain),
        schema_version,
        idempotency_key.to_string(),
        chrono::Utc::now(),
        request.data.clone(),
    )
    .map(|_| ())
    .map_err(|e| {
        tracing::info!(
            metric = "AdmissionRejected",
            domain = request.domain,
            error = %e,
            "envelope would be undeliverable; refusing admission"
        );
        api::from_control_plane_error(&e)
    })
}

/// An admitted publish command, ready for the queue: the caller's request
/// plus everything admission derived on its behalf. The schema version is the
/// one the payload was validated against; the idempotency key is the caller's
/// or the API Gateway request ID.
struct AdmittedCommand<'a> {
    request: &'a PublishRequest,
    raw_body: &'a Value,
    stage: &'a str,
    schema_version: i64,
    idempotency_key: String,
    caller_account_id: String,
}

/// Enqueues the admitted command and builds the 202 response; an enqueue
/// failure is a 503 (the command may or may not have landed — the publisher's
/// retry with the same idempotency key is safe under at-least-once delivery).
///
/// The message matches the envelope the delivery consumer parses: principal
/// attributes, `Environment`, `IdempotencyKey`, the admission-resolved
/// `SchemaVersion` (so the stamped version can never drift from the validated
/// definition), and a `callerAccountId` + `body` message body.
async fn queue_admitted(
    clients: &Clients,
    principal: &TrustedPrincipal,
    command: &AdmittedCommand<'_>,
    started: Instant,
) -> Value {
    let Some(queue_url) = publish_queue_url() else {
        return temporarily_unavailable();
    };
    let message_body = json!({
        "callerAccountId": command.caller_account_id,
        "body": command.raw_body,
    })
    .to_string();

    let attributes = match build_message_attributes(principal, command) {
        Ok(attributes) => attributes,
        Err(response) => return response,
    };

    let mut send = clients
        .sqs
        .send_message()
        .queue_url(queue_url)
        .message_body(message_body);
    for (name, attribute) in attributes {
        send = send.message_attributes(name, attribute);
    }

    match send.send().await {
        Ok(output) => {
            let message_id = output.message_id.unwrap_or_default();
            tracing::info!(
                metric = "AdmissionAccepted",
                domain = command.request.domain,
                environment = command.stage,
                schema_version = command.schema_version,
                latency_ms = started.elapsed().as_secs_f64() * 1000.0,
                "publish admitted and queued"
            );
            api::success_response(
                202,
                &json!({"requestId": message_id, "message": "Request accepted"}),
            )
        }
        Err(error) => {
            tracing::error!(
                metric = "PublishUnavailable",
                domain = command.request.domain,
                error = %error.into_service_error(),
                "enqueue failed after successful validation"
            );
            temporarily_unavailable()
        }
    }
}

/// Builds the SQS message attributes for an admitted command. A build
/// failure is a platform-side defect: answer 503 like any enqueue failure,
/// never panic in the request path.
fn build_message_attributes(
    principal: &TrustedPrincipal,
    command: &AdmittedCommand<'_>,
) -> Result<Vec<(String, aws_sdk_sqs::types::MessageAttributeValue)>, Value> {
    let mut attributes = Vec::new();
    for (name, value) in message_attributes(principal, command) {
        let attribute = aws_sdk_sqs::types::MessageAttributeValue::builder()
            .string_value(value)
            .data_type("String")
            .build()
            .map_err(|error| {
                tracing::error!(
                    metric = "PublishUnavailable",
                    domain = command.request.domain,
                    error = %error,
                    "message attribute build failed before enqueue"
                );
                temporarily_unavailable()
            })?;
        attributes.push((name, attribute));
    }
    Ok(attributes)
}

/// The full message-attribute set the delivery consumer parses: the trusted
/// principal envelope plus the admission-stamped routing and idempotency
/// attributes.
///
/// SQS allows at most 10 attributes per message and the OAuth principal
/// envelope carries 7, leaving exactly the three routing stamps — there is no
/// headroom, so this set may not grow. (`HttpMethod` is deliberately absent:
/// unlike the multi-route command queues, this queue serves only `/publish`.)
fn message_attributes(
    principal: &TrustedPrincipal,
    command: &AdmittedCommand<'_>,
) -> std::collections::HashMap<String, String> {
    let mut attributes = principal.to_sqs_attributes();
    attributes.insert("Environment".to_string(), command.stage.to_string());
    attributes.insert(
        "IdempotencyKey".to_string(),
        command.idempotency_key.clone(),
    );
    attributes.insert(
        "SchemaVersion".to_string(),
        command.schema_version.to_string(),
    );
    attributes
}

/// Resolves the latest schema and validates the payload against it, mapping
/// every failure to its API response: absent schema → 404, unusable stored
/// schema or registry/refresh failure → 503, confirmed violations → 422.
async fn validated_latest_schema(
    clients: &Clients,
    cache: &SchemaCache,
    registry: &str,
    schema_name: &SchemaName,
    request: &PublishRequest,
) -> Result<LatestSchema, Value> {
    let latest = resolve_latest(clients, cache, registry, schema_name).await?;

    match validate_against_latest(&latest, &request.data) {
        Ok(()) => {
            tracing::info!(
                metric = "PublishValidated",
                domain = request.domain,
                schema_version = latest.version,
                "payload conforms to the latest schema"
            );
            Ok(latest)
        }
        Err(ValidationError::SchemaUnusable(reason)) => {
            tracing::error!(
                metric = "PublishUnavailable",
                domain = request.domain,
                schema_version = latest.version,
                reason,
                "stored schema does not compile as draft-4"
            );
            Err(schema_unusable())
        }
        Err(ValidationError::Violations(first_violations)) => {
            reject_or_refresh(
                clients,
                cache,
                registry,
                schema_name,
                request,
                first_violations,
            )
            .await
        }
    }
}

/// The 422 path: refresh-before-reject, then either admit against the newer
/// definition or reject with the confirmed violations. A failed refresh
/// refuses to reject on unverifiable grounds and degrades to 503.
async fn reject_or_refresh(
    clients: &Clients,
    cache: &SchemaCache,
    registry: &str,
    schema_name: &SchemaName,
    request: &PublishRequest,
    first_violations: String,
) -> Result<LatestSchema, Value> {
    match refresh_and_revalidate(clients, cache, registry, schema_name, &request.data).await {
        Ok(latest) => Ok(latest),
        Err(RefreshOutcome::StillViolating(message)) => {
            tracing::info!(
                metric = "PublishRejected",
                domain = request.domain,
                violations = message,
                "payload violates the latest schema"
            );
            Err(unprocessable(message))
        }
        Err(RefreshOutcome::RefreshFailed(error)) => {
            tracing::error!(
                metric = "SchemaCacheRefreshFailed",
                domain = request.domain,
                error = %error,
                violations = first_violations,
                "forced refresh failed; cannot reject with certainty"
            );
            Err(temporarily_unavailable())
        }
    }
}

/// Resolves the latest schema through the cache, mapping every failure to its
/// API response: absent schema → 404, everything else → 503 without registry
/// internals.
async fn resolve_latest(
    clients: &Clients,
    cache: &SchemaCache,
    registry: &str,
    schema_name: &SchemaName,
) -> Result<LatestSchema, Value> {
    cache
        .get_or_fetch(registry, &schema_name.to_string(), || {
            fetch_latest_schema(clients, registry, schema_name)
        })
        .await
        .map_err(|e| match e {
            SchemaCacheError::NotFound => {
                api::from_control_plane_error(&ControlPlaneError::SchemaNotFound {
                    schema_name: schema_name.to_string(),
                })
            }
            other => {
                tracing::error!(
                    metric = "PublishUnavailable",
                    error = %other,
                    "schema resolution failed"
                );
                temporarily_unavailable()
            }
        })
}

enum RefreshOutcome {
    StillViolating(String),
    RefreshFailed(SchemaCacheError),
}

/// Refresh-before-reject: a first validation failure forces a fresh registry
/// read, so a publish racing a schema append is judged against the newest
/// definition, never a stale cached one. Only a second failure rejects.
async fn refresh_and_revalidate(
    clients: &Clients,
    cache: &SchemaCache,
    registry: &str,
    schema_name: &SchemaName,
    data: &Value,
) -> Result<LatestSchema, RefreshOutcome> {
    let latest = cache
        .force_fetch(registry, &schema_name.to_string(), || {
            fetch_latest_schema(clients, registry, schema_name)
        })
        .await
        .map_err(RefreshOutcome::RefreshFailed)?;
    match validate_against_latest(&latest, data) {
        Ok(()) => {
            tracing::info!(
                metric = "PublishValidated",
                schema_version = latest.version,
                "payload conforms after forced refresh"
            );
            Ok(latest)
        }
        Err(ValidationError::Violations(message)) => Err(RefreshOutcome::StillViolating(message)),
        Err(ValidationError::SchemaUnusable(reason)) => Err(RefreshOutcome::RefreshFailed(
            SchemaCacheError::RegistryUnavailable(format!(
                "stored schema does not compile as draft-4: {reason}"
            )),
        )),
    }
}

/// The registry read the cache closures delegate to. A version-less
/// `DescribeSchema` returns the latest version.
async fn fetch_latest_schema(
    clients: &Clients,
    registry: &str,
    schema_name: &SchemaName,
) -> Result<LatestSchema, SchemaCacheError> {
    let started = Instant::now();
    let output = clients
        .schemas
        .describe_schema()
        .registry_name(registry)
        .schema_name(schema_name.to_string())
        .send()
        .await
        .map_err(|e| {
            let service_err = e.into_service_error();
            if service_err.is_not_found_exception() {
                SchemaCacheError::NotFound
            } else {
                SchemaCacheError::RegistryUnavailable(service_err.to_string())
            }
        })?;
    tracing::info!(
        metric = "RegistryFetchLatency",
        latency_ms = started.elapsed().as_secs_f64() * 1000.0,
        "schema fetched from registry"
    );
    let version = output
        .schema_version
        .and_then(|v| v.parse::<i64>().ok())
        .ok_or(SchemaCacheError::VersionUnparseable)?;
    let content = output.content.ok_or(SchemaCacheError::ContentMissing)?;
    Ok(LatestSchema { content, version })
}

async fn require_active_domain(
    clients: &Clients,
    domain_name: &str,
) -> Result<(), ControlPlaneError> {
    let table = env::var("DOMAINS_TABLE")
        .map_err(|_| ControlPlaneError::DynamoDb("DOMAINS_TABLE not set".to_string()))?;
    let output = clients
        .ddb
        .get_item()
        .table_name(table)
        .key("domainName", s(domain_name))
        .send()
        .await
        .map_err(|e| ControlPlaneError::DynamoDb(e.into_service_error().to_string()))?;

    let status = output
        .item
        .as_ref()
        .and_then(|item| get_s(item, "status"))
        .unwrap_or_default();
    if status.parse::<DomainStatus>() != Ok(DomainStatus::Active) {
        return Err(ControlPlaneError::DomainNotFound {
            domain_name: domain_name.to_string(),
        });
    }
    Ok(())
}

fn stage_of(payload: &Value) -> Result<String, ControlPlaneError> {
    payload
        .pointer("/requestContext/stage")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| api::validation_error("request context is missing the API stage"))
}

fn publish_queue_url() -> Option<String> {
    match env::var("PUBLISH_QUEUE_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            tracing::error!("PUBLISH_QUEUE_URL not set");
            None
        }
    }
}

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn unprocessable(message: String) -> Value {
    api::error_response(
        422,
        ApiError {
            code: "UnprocessableEntityException".to_string(),
            message,
            request_id: None,
            reason: None,
            field_list: None,
        },
    )
}

fn temporarily_unavailable() -> Value {
    api::error_response(
        503,
        ApiError {
            code: "ServiceUnavailableException".to_string(),
            message: "Publish admission is temporarily unavailable; retry the request".to_string(),
            request_id: None,
            reason: None,
            field_list: None,
        },
    )
}

fn schema_unusable() -> Value {
    api::error_response(
        503,
        ApiError {
            code: "ServiceUnavailableException".to_string(),
            message: "The registered schema could not be validated; contact the platform team"
                .to_string(),
            request_id: None,
            reason: None,
            field_list: None,
        },
    )
}

fn internal_error(detail: &str) -> Value {
    tracing::error!(detail, "platform configuration error");
    api::from_control_plane_error(&ControlPlaneError::DynamoDb(detail.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use konductor_common::dynamodb::n;
    use konductor_common::grant::{BindingPrincipal, ResourceRole, RoleBinding};
    use konductor_common::grant_engine::InMemoryGrantEngine;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_TestPool";
    const QUEUE_URL: &str =
        "https://sqs.us-east-1.amazonaws.com/111122223333/konductor-publish-queue";
    const STRICT_SCHEMA: &str = r#"{
        "$schema": "http://json-schema.org/draft-04/schema#",
        "type": "object",
        "properties": {"orderId": {"type": "integer"}},
        "required": ["orderId"]
    }"#;
    const RELAXED_SCHEMA: &str =
        r#"{"$schema": "http://json-schema.org/draft-04/schema#", "type": "object"}"#;

    fn set_env() {
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var("DOMAINS_TABLE", "domains-test");
        std::env::set_var("PUBLISH_QUEUE_URL", QUEUE_URL);
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

    fn publish_body(data: Value) -> Value {
        json!({
            "domain": "orders",
            "eventName": "OrderNotification",
            "data": data
        })
    }

    fn oauth_event(body: Value) -> Value {
        json!({
            "httpMethod": "POST",
            "resource": "/publish",
            "requestContext": {
                "stage": "prod",
                "requestId": "req-1",
                "authorizer": {
                    "authenticationMechanism": "oauth",
                    "principalId": "sub-123",
                    "issuer": ISSUER,
                    "clientId": "client-1",
                    "globalRoles": "[]",
                    "teamGroups": "[\"team-orders\"]"
                }
            },
            "body": body.to_string()
        })
    }

    fn iam_event(body: Value) -> Value {
        json!({
            "httpMethod": "POST",
            "resource": "/publish",
            "requestContext": {
                "stage": "prod",
                "requestId": "req-1",
                "identity": {
                    "accountId": "111122223333",
                    "userArn": "arn:aws:iam::111122223333:role/publisher"
                }
            },
            "body": body.to_string()
        })
    }

    fn event(payload: Value) -> LambdaEvent<Value> {
        LambdaEvent::new(payload, Default::default())
    }

    fn status(resp: &Value) -> u16 {
        resp["statusCode"].as_u64().unwrap() as u16
    }

    fn response_body(resp: &Value) -> Value {
        serde_json::from_str(resp["body"].as_str().unwrap()).unwrap()
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

    fn describe_rule(content: &'static str, version: &'static str) -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema)
            .match_requests(move |req| {
                req.registry_name() == Some("acme-prod-orders-schema-registry")
                    && req.schema_name() == Some("acme-orders@OrderNotification")
            })
            .then_output(move || {
                aws_sdk_schemas::operation::describe_schema::DescribeSchemaOutput::builder()
                    .schema_version(version)
                    .content(content)
                    .build()
            })
    }

    fn schema_not_found_rule() -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema).then_error(|| {
            aws_sdk_schemas::operation::describe_schema::DescribeSchemaError::NotFoundException(
                aws_sdk_schemas::types::error::NotFoundException::builder()
                    .message("not found")
                    .build(),
            )
        })
    }

    fn schema_unavailable_rule() -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema).then_error(|| {
            aws_sdk_schemas::operation::describe_schema::DescribeSchemaError::generic(
                aws_sdk_schemas::error::ErrorMetadata::builder()
                    .message("throttled")
                    .build(),
            )
        })
    }

    fn send_message_rule(
        expected_key: &'static str,
        mechanism: &'static str,
        expected_version: &'static str,
    ) -> Rule {
        mock!(aws_sdk_sqs::Client::send_message)
            .match_requests(move |req| {
                if req.queue_url() != Some(QUEUE_URL) {
                    return false;
                }
                let attrs = req.message_attributes().cloned().unwrap_or_default();
                let attr = |name: &str| attrs.get(name).and_then(|a| a.string_value());
                if attr("Environment") != Some("prod")
                    || attr("IdempotencyKey") != Some(expected_key)
                    || attr("AuthMechanism") != Some(mechanism)
                    || attr("PrincipalVersion") != Some("1")
                    || attr("SchemaVersion") != Some(expected_version)
                {
                    return false;
                }
                let body: Value = serde_json::from_str(req.message_body().unwrap_or_default())
                    .unwrap_or_default();
                body["body"]["domain"] == json!("orders")
                    && body["body"]["eventName"] == json!("OrderNotification")
            })
            .then_output(|| {
                aws_sdk_sqs::operation::send_message::SendMessageOutput::builder()
                    .message_id("sqs-msg-1")
                    .build()
            })
    }

    fn clients(mode: RuleMode, ddb: &[Rule], schemas: &[Rule], sqs: &[Rule]) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, mode, ddb),
            schemas: mock_client!(aws_sdk_schemas, mode, schemas),
            sqs: mock_client!(aws_sdk_sqs, mode, sqs),
        }
    }

    fn cache() -> SchemaCache {
        SchemaCache::new(std::time::Duration::from_secs(60))
    }

    fn metadata(rules: &[&Rule]) -> MetadataAdmission {
        MetadataAdmission::new(
            mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, rules),
            "metadata-test",
        )
    }

    fn pointer_rule(revision: i64, generation: i64) -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.table_name() == Some("metadata-test")
                    && req.key().and_then(|k| k.get("PK")) == Some(&s("ENV"))
                    && req.key().and_then(|k| k.get("SK")) == Some(&s("prod"))
            })
            .then_output(move || {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .item("PK", s("ENV"))
                    .item("SK", s("prod"))
                    .item("activeRevision", n(revision))
                    .item("generation", n(generation))
                    .item("definitionDigest", s("digest-2"))
                    .item("activatedBy", s("admin-sub"))
                    .item("activatedAt", s("2026-01-01T00:00:00.000Z"))
                    .build()
            })
    }

    fn legacy_pointer_rule() -> Rule {
        pointer_rule(0, 0)
    }

    fn revision_rule(revision: i64) -> Rule {
        let sort_key = format!("r{revision:010}");
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(move |req| {
                req.table_name() == Some("metadata-test")
                    && req.key().and_then(|k| k.get("PK")) == Some(&s("REVISION"))
                    && req.key().and_then(|k| k.get("SK")) == Some(&s(&sort_key))
            })
            .then_output(move || {
                let sort_key = format!("r{revision:010}");
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .item("PK", s("REVISION"))
                    .item("SK", s(&sort_key))
                    .item("revision", n(revision))
                    .item("predecessor", n(revision - 1))
                    .item("definition", s("{}"))
                    .item("metadataSchema", s("{}"))
                    .item("definitionDigest", s("digest"))
                    .item("compilerFormat", n(1))
                    .item("createdBy", s("admin-sub"))
                    .item("createdAt", s("2026-01-01T00:00:00.000Z"))
                    .item("operationId", s("op"))
                    .build()
            })
    }

    #[tokio::test]
    async fn conforming_publish_is_admitted_and_queued() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let mut body = publish_body(json!({"orderId": 123}));
        body["idempotencyKey"] = json!("key-1");
        let clients = clients(
            RuleMode::MatchAny,
            &[active_domain_rule()],
            &[describe_rule(STRICT_SCHEMA, "3")],
            &[send_message_rule("key-1", "oauth", "3")],
        );

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[&legacy_pointer_rule()]),
            event(oauth_event(body)),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 202);
        assert_eq!(response_body(&resp)["requestId"], "sqs-msg-1");
    }

    #[tokio::test]
    async fn missing_idempotency_key_falls_back_to_the_request_id() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(
            RuleMode::MatchAny,
            &[active_domain_rule()],
            &[describe_rule(STRICT_SCHEMA, "3")],
            &[send_message_rule("req-1", "oauth", "3")],
        );

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[&legacy_pointer_rule()]),
            event(oauth_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 202);
    }

    #[tokio::test]
    async fn non_publisher_is_denied_without_any_resource_access() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![]);
        // Zero rules on every client, including the metadata resolver: any
        // DDB, Schemas, SQS, or metadata-configuration call panics, proving
        // authorization precedes all resource access.
        let clients = clients(RuleMode::MatchAny, &[], &[], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[]),
            event(oauth_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 403);
        assert_eq!(response_body(&resp)["code"], "AccessDeniedException");
    }

    #[tokio::test]
    async fn inactive_domain_is_a_404_before_schema_resolution() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let inactive = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("domainName", s("orders"))
                .item("status", s("PROVISIONING"))
                .build()
        });
        let clients = clients(RuleMode::MatchAny, &[inactive], &[], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[]),
            event(oauth_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 404);
    }

    #[tokio::test]
    async fn unregistered_schema_is_a_404_without_enqueue() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        // No SQS rule: an enqueue attempt panics.
        let clients = clients(
            RuleMode::MatchAny,
            &[active_domain_rule()],
            &[schema_not_found_rule()],
            &[],
        );

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[&legacy_pointer_rule()]),
            event(oauth_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 404);
        assert_eq!(response_body(&resp)["code"], "ResourceNotFoundException");
    }

    #[tokio::test]
    async fn violating_payload_is_a_422_after_a_forced_refresh() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        // MatchAny: the initial fetch and the forced refresh both serve the
        // strict schema, so the rejection is certain. No SQS rule.
        let clients = clients(
            RuleMode::MatchAny,
            &[active_domain_rule()],
            &[describe_rule(STRICT_SCHEMA, "3")],
            &[],
        );

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[&legacy_pointer_rule()]),
            event(oauth_event(publish_body(json!({"amount": 9.5})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 422);
        assert_eq!(response_body(&resp)["code"], "UnprocessableEntityException");
        assert!(
            response_body(&resp)["message"]
                .as_str()
                .unwrap()
                .contains("orderId"),
            "422 message must identify the violation"
        );
    }

    #[tokio::test]
    async fn violation_resolved_by_a_newer_schema_is_admitted() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        // Sequential: first fetch serves strict v3, the forced refresh serves
        // relaxed v4 (the append raced the publish) — admission proceeds.
        let clients = clients(
            RuleMode::Sequential,
            &[active_domain_rule()],
            &[
                describe_rule(STRICT_SCHEMA, "3"),
                describe_rule(RELAXED_SCHEMA, "4"),
            ],
            &[send_message_rule("key-1", "oauth", "4")],
        );
        let mut body = publish_body(json!({"amount": 9.5}));
        body["idempotencyKey"] = json!("key-1");

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[&legacy_pointer_rule()]),
            event(oauth_event(body)),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 202);
    }

    #[tokio::test]
    async fn registry_outage_is_a_503_without_disclosure() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(
            RuleMode::MatchAny,
            &[active_domain_rule()],
            &[schema_unavailable_rule()],
            &[],
        );

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[&legacy_pointer_rule()]),
            event(oauth_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 503);
        assert_eq!(response_body(&resp)["code"], "ServiceUnavailableException");
        assert!(
            !response_body(&resp)["message"]
                .as_str()
                .unwrap()
                .contains("throttled"),
            "503 must not disclose registry internals"
        );
    }

    #[tokio::test]
    async fn unusable_stored_schema_is_a_503() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(
            RuleMode::MatchAny,
            &[active_domain_rule()],
            &[describe_rule("not json", "3")],
            &[],
        );

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[&legacy_pointer_rule()]),
            event(oauth_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 503);
    }

    #[tokio::test]
    async fn enqueue_failure_after_validation_is_a_503() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let send_error = mock!(aws_sdk_sqs::Client::send_message).then_error(|| {
            aws_sdk_sqs::operation::send_message::SendMessageError::generic(
                aws_sdk_sqs::error::ErrorMetadata::builder()
                    .message("queue unavailable")
                    .build(),
            )
        });
        let clients = clients(
            RuleMode::MatchAny,
            &[active_domain_rule()],
            &[describe_rule(STRICT_SCHEMA, "3")],
            &[send_error],
        );

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[&legacy_pointer_rule()]),
            event(oauth_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 503);
    }

    #[tokio::test]
    async fn missing_stage_is_a_400() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let mut payload = oauth_event(publish_body(json!({"orderId": 1})));
        payload["requestContext"]["stage"] = json!("");
        let clients = clients(RuleMode::MatchAny, &[active_domain_rule()], &[], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[]),
            event(payload),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn iam_caller_is_admitted_with_account_identity() {
        set_env();
        let authorizer =
            InMemoryGrantEngine::new(vec![iam_publisher_binding("111122223333", "orders")]);
        let send = mock!(aws_sdk_sqs::Client::send_message)
            .match_requests(|req| {
                let attrs = req.message_attributes().cloned().unwrap_or_default();
                let attr = |name: &str| attrs.get(name).and_then(|a| a.string_value());
                if attr("AuthMechanism") != Some("iam") || attr("AccountId") != Some("111122223333")
                {
                    return false;
                }
                let body: Value = serde_json::from_str(req.message_body().unwrap_or_default())
                    .unwrap_or_default();
                body["callerAccountId"] == json!("111122223333")
            })
            .then_output(|| {
                aws_sdk_sqs::operation::send_message::SendMessageOutput::builder()
                    .message_id("sqs-msg-9")
                    .build()
            });
        let clients = clients(
            RuleMode::MatchAny,
            &[active_domain_rule()],
            &[describe_rule(STRICT_SCHEMA, "3")],
            &[send],
        );

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[&legacy_pointer_rule()]),
            event(iam_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 202);
    }

    #[tokio::test]
    async fn oauth_message_attributes_stay_within_the_sqs_limit() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        // SQS allows at most 10 message attributes per message; the OAuth
        // principal envelope carries 7, so the rule matches only while the
        // total stays within the limit.
        let send = mock!(aws_sdk_sqs::Client::send_message)
            .match_requests(|req| req.message_attributes().is_some_and(|a| a.len() <= 10))
            .then_output(|| {
                aws_sdk_sqs::operation::send_message::SendMessageOutput::builder()
                    .message_id("sqs-msg-1")
                    .build()
            });
        let clients = clients(
            RuleMode::MatchAny,
            &[active_domain_rule()],
            &[describe_rule(STRICT_SCHEMA, "3")],
            &[send],
        );

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[&legacy_pointer_rule()]),
            event(oauth_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 202);
    }

    #[tokio::test]
    async fn an_undeliverable_envelope_is_a_400_without_queueing() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        // No SQS rule: an enqueue attempt panics. The relaxed schema accepts
        // any object, so only the envelope size check can reject.
        let clients = clients(
            RuleMode::MatchAny,
            &[active_domain_rule()],
            &[describe_rule(RELAXED_SCHEMA, "3")],
            &[],
        );
        let big = publish_body(json!({"blob": "x".repeat(270 * 1024)}));

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[&legacy_pointer_rule()]),
            event(oauth_event(big)),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
        assert!(
            response_body(&resp)["message"]
                .as_str()
                .unwrap()
                .contains("at most"),
            "400 must name the EventBridge size limit"
        );
    }

    #[tokio::test]
    async fn domain_store_outage_is_a_503() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let ddb_error = mock!(aws_sdk_dynamodb::Client::get_item).then_error(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemError::generic(
                aws_sdk_dynamodb::error::ErrorMetadata::builder()
                    .message("throttled")
                    .build(),
            )
        });
        let clients = clients(RuleMode::MatchAny, &[ddb_error], &[], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata(&[]),
            event(oauth_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 503);
        assert_eq!(response_body(&resp)["code"], "ServiceUnavailableException");
    }

    #[tokio::test]
    async fn a_warm_cache_serves_the_second_publish_without_a_registry_call() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        // Sequential DescribeSchema with a single rule: a second registry call
        // would have no rule left and panic, proving the cache served it. The
        // domain and queue mocks stay MatchAny for both invocations.
        let clients = Clients {
            ddb: mock_client!(
                aws_sdk_dynamodb,
                RuleMode::MatchAny,
                &[active_domain_rule()]
            ),
            schemas: mock_client!(
                aws_sdk_schemas,
                RuleMode::Sequential,
                &[describe_rule(STRICT_SCHEMA, "3")]
            ),
            sqs: mock_client!(
                aws_sdk_sqs,
                RuleMode::MatchAny,
                &[send_message_rule("key-1", "oauth", "3")]
            ),
        };
        let cache = cache();

        for _ in 0..2 {
            let mut body = publish_body(json!({"orderId": 1}));
            body["idempotencyKey"] = json!("key-1");
            let resp = handle(
                &clients,
                &authorizer,
                &cache,
                &metadata(&[&legacy_pointer_rule()]),
                event(oauth_event(body)),
            )
            .await
            .unwrap();
            assert_eq!(status(&resp), 202);
        }
    }

    #[tokio::test]
    async fn authorized_publish_resolves_the_active_metadata_revision_with_a_fresh_pointer_read() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let pointer = pointer_rule(2, 1);
        let revision = revision_rule(2);
        let clients = clients(
            RuleMode::MatchAny,
            &[active_domain_rule()],
            &[describe_rule(RELAXED_SCHEMA, "3")],
            &[
                send_message_rule("key-1", "oauth", "3"),
                send_message_rule("key-2", "oauth", "3"),
            ],
        );
        let metadata = metadata(&[&pointer, &revision]);

        let mut body_one = publish_body(json!({"orderId": 123}));
        body_one["idempotencyKey"] = json!("key-1");
        let mut body_two = publish_body(json!({"orderId": 123}));
        body_two["idempotencyKey"] = json!("key-2");

        handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata,
            event(oauth_event(body_one)),
        )
        .await
        .unwrap();
        handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata,
            event(oauth_event(body_two)),
        )
        .await
        .unwrap();

        assert_eq!(
            pointer.num_calls(),
            2,
            "the pointer is re-read on every publish"
        );
        assert_eq!(
            revision.num_calls(),
            1,
            "the immutable revision record is cached"
        );
    }

    #[tokio::test]
    async fn a_missing_metadata_pointer_is_a_503_without_a_stale_fallback() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let missing = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.table_name() == Some("metadata-test"))
            .then_output(|| {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build()
            });
        // Zero Schemas and SQS rules: the 503 must precede schema resolution
        // and enqueue, and the metadata read must answer 503, never a cached
        // or invented selection.
        let clients = clients(RuleMode::MatchAny, &[active_domain_rule()], &[], &[]);
        let metadata = metadata(&[&missing]);

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata,
            event(oauth_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 503);
    }

    #[tokio::test]
    async fn a_pointer_to_a_missing_revision_is_a_503() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let pointer = pointer_rule(2, 1);
        let missing_revision = mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.table_name() == Some("metadata-test"))
            .then_output(|| {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build()
            });
        let clients = clients(RuleMode::MatchAny, &[active_domain_rule()], &[], &[]);
        let metadata = metadata(&[&pointer, &missing_revision]);

        let resp = handle(
            &clients,
            &authorizer,
            &cache(),
            &metadata,
            event(oauth_event(publish_body(json!({"orderId": 1})))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 503);
    }
}
