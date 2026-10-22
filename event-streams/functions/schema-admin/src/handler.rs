//! Publisher schema administration: `POST /schemas` creates a schema
//! (EventBridge `CreateSchema` semantics) and `POST /schemas/versions`
//! appends a version (`UpdateSchema` semantics) on both control-plane
//! surfaces. The grant engine authorizes the publisher action for the target
//! domain before any resource access; Konductor derives the canonical schema
//! name and the stage-selected environment's registry. Schemas are
//! append-only: no delete, retire, or historical-version replacement.

use lambda_runtime::{Error, LambdaEvent};
use serde_json::Value;

use crate::record::{complete_record_if_canonical_equal, record_version_content};
use konductor_common::api;
use konductor_common::dynamodb::{get_s, s};
use konductor_common::envelope::{rule_name, SchemaName};
use konductor_common::environments;
use konductor_common::error::ControlPlaneError;
use konductor_common::grant_engine::actions;
use konductor_common::metadata::composition::check_offline_composability;
use konductor_common::models::{
    AppendSchemaVersionRequest, CreateSchemaRequest, DomainStatus, SchemaMutationResponse,
};
use konductor_common::namespace::NamespaceResolver;
use konductor_common::principal::{AuthorizationDecision, ResourceAuthorizer, TrustedPrincipal};
use konductor_common::principal_context::log_principal;
use konductor_common::schema_compatibility::{
    check_compatible, CompatibilityCheck, Incompatibility, RejectionKind,
};
use konductor_common::schema_validation::check_draft4_document;

use aws_sdk_schemas::error::ProvideErrorMetadata;

const SUPPORTED_SCHEMA_TYPE: &str = "JSONSchemaDraft4";

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

pub async fn handle<A: ResourceAuthorizer>(
    clients: &Clients,
    authorizer: &A,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    dispatch(clients, authorizer, &event.payload).await
}

async fn dispatch<A: ResourceAuthorizer>(
    clients: &Clients,
    authorizer: &A,
    payload: &Value,
) -> Result<Value, Error> {
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
        ("POST", "/schemas") => create_schema(clients, authorizer, payload, &principal).await,
        ("POST", "/schemas/versions") => {
            append_schema_version(clients, authorizer, payload, &principal).await
        }
        _ => Ok(api::from_control_plane_error(
            &ControlPlaneError::SchemaNotFound {
                schema_name: format!("{method} {resource}"),
            },
        )),
    }
}

async fn create_schema<A: ResourceAuthorizer>(
    clients: &Clients,
    authorizer: &A,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let request: CreateSchemaRequest = match api::parse_body(payload) {
        Ok(r) => r,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    if request.schema_type != SUPPORTED_SCHEMA_TYPE {
        return Ok(api::from_control_plane_error(&api::validation_error(
            format!("schemaType must be {SUPPORTED_SCHEMA_TYPE}"),
        )));
    }
    if let Err(e) = check_draft4_document(&request.content) {
        return Ok(api::from_control_plane_error(&api::validation_error(
            format!("content is not a valid JSON Schema draft-4 document: {e}"),
        )));
    }
    if let Err(e) = check_offline_composability(&request.content) {
        return Ok(api::from_control_plane_error(&api::validation_error(
            format!("content cannot participate in an offline composed contract: {e}"),
        )));
    }
    let target = match prepare(
        clients,
        authorizer,
        payload,
        principal,
        &request.domain,
        &request.event_name,
        actions::SCHEMA_CREATE,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    let result = clients
        .schemas
        .create_schema()
        .registry_name(&target.registry)
        .schema_name(target.schema_name.to_string())
        .r#type(aws_sdk_schemas::types::Type::JsonSchemaDraft4)
        .content(&request.content)
        .send()
        .await;

    match result {
        Ok(output) => {
            let version = output
                .schema_version
                .as_deref()
                .and_then(|v| v.parse::<i64>().ok());
            if let Some(version) = version {
                if let Err(e) =
                    record_version_content(clients, &target, version, &request.content).await
                {
                    return Ok(api::from_control_plane_error(&e));
                }
            }
            emit_version_published(clients, &target, output.schema_version.as_deref()).await;
            mutation_response(&target.schema_name, output.schema_version, false)
        }
        Err(e) => {
            use aws_sdk_schemas::operation::create_schema::CreateSchemaError;
            let service_err = e.into_service_error();
            // The service model has no ConflictException for CreateSchema: a
            // duplicate surfaces as BadRequestException ("already exists"),
            // with a code() fallback should the service add a conflict code.
            let already_exists = matches!(&service_err,
                    CreateSchemaError::BadRequestException(bad_request)
                        if bad_request.message().unwrap_or_default().contains("already exists"))
                || service_err.code() == Some("ConflictException");
            if already_exists {
                // An earlier attempt may have committed the registry write
                // without its content record; repair before answering.
                if let Err(e) =
                    complete_record_if_canonical_equal(clients, &target, &request.content).await
                {
                    return Ok(api::from_control_plane_error(&e));
                }
                return Ok(api::from_control_plane_error(
                    &ControlPlaneError::SchemaAlreadyExists {
                        schema_name: target.schema_name.to_string(),
                    },
                ));
            }
            let mapped = match &service_err {
                CreateSchemaError::BadRequestException(_) => {
                    api::validation_error("create schema rejected the schema document")
                }
                other => {
                    tracing::error!(error = %other, "create schema failed");
                    ControlPlaneError::Schemas(other.to_string())
                }
            };
            Ok(api::from_control_plane_error(&mapped))
        }
    }
}

async fn append_schema_version<A: ResourceAuthorizer>(
    clients: &Clients,
    authorizer: &A,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let request: AppendSchemaVersionRequest = match api::parse_body(payload) {
        Ok(r) => r,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    if let Err(e) = check_draft4_document(&request.content) {
        return Ok(api::from_control_plane_error(&api::validation_error(
            format!("content is not a valid JSON Schema draft-4 document: {e}"),
        )));
    }
    if let Err(e) = check_offline_composability(&request.content) {
        return Ok(api::from_control_plane_error(&api::validation_error(
            format!("content cannot participate in an offline composed contract: {e}"),
        )));
    }
    let target = match prepare(
        clients,
        authorizer,
        payload,
        principal,
        &request.domain,
        &request.event_name,
        actions::SCHEMA_VERSION_APPEND,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    if let Err(e) = check_append_compatibility(clients, &target, &request.content).await {
        return Ok(api::from_control_plane_error(&e));
    }

    let result = clients
        .schemas
        .update_schema()
        .registry_name(&target.registry)
        .schema_name(target.schema_name.to_string())
        .r#type(aws_sdk_schemas::types::Type::JsonSchemaDraft4)
        .content(&request.content)
        .send()
        .await;

    match result {
        Ok(output) => {
            let version = output
                .schema_version
                .as_deref()
                .and_then(|v| v.parse::<i64>().ok());
            if let Some(version) = version {
                if let Err(e) =
                    record_version_content(clients, &target, version, &request.content).await
                {
                    return Ok(api::from_control_plane_error(&e));
                }
            }
            emit_version_published(clients, &target, output.schema_version.as_deref()).await;
            mutation_response(&target.schema_name, output.schema_version, false)
        }
        Err(e) => {
            let service_err = e.into_service_error();
            // The SDK model has no typed ConflictException for UpdateSchema;
            // the service's no-change conflict arrives unhandled by code.
            if service_err.code() == Some("ConflictException") {
                return match complete_record_if_canonical_equal(clients, &target, &request.content)
                    .await
                {
                    Ok(Some(version)) => {
                        mutation_response(&target.schema_name, Some(version.to_string()), true)
                    }
                    Ok(None) => Ok(api::from_control_plane_error(&ControlPlaneError::Schemas(
                        "no-change conflict but the latest content differs from the request"
                            .to_string(),
                    ))),
                    Err(e) => Ok(api::from_control_plane_error(&e)),
                };
            }
            let mapped = match &service_err {
                aws_sdk_schemas::operation::update_schema::UpdateSchemaError::NotFoundException(_) => {
                    ControlPlaneError::SchemaNotFound {
                        schema_name: target.schema_name.to_string(),
                    }
                }
                aws_sdk_schemas::operation::update_schema::UpdateSchemaError::BadRequestException(_) => {
                    api::validation_error("append schema version rejected the schema document")
                }
                other => {
                    tracing::error!(error = %other, "append schema version failed");
                    ControlPlaneError::Schemas(other.to_string())
                }
            };
            Ok(api::from_control_plane_error(&mapped))
        }
    }
}

/// The promotion gate: the append may only loosen the current latest
/// version, never tighten it. The version-less describe doubles as the
/// existence check; `UpdateSchema`'s NotFound mapping stays as defense.
async fn check_append_compatibility(
    clients: &Clients,
    target: &SchemaTarget,
    content: &str,
) -> Result<(), ControlPlaneError> {
    let predecessor = latest_schema_document(clients, target).await?;
    let candidate: Value = serde_json::from_str(content).map_err(|e| {
        ControlPlaneError::Schemas(format!("validated content failed to re-parse: {e}"))
    })?;
    if let CompatibilityCheck::NotEstablished(reasons) = check_compatible(&predecessor, &candidate)
    {
        let restrictions = reasons
            .iter()
            .filter(|r| r.rule.kind() == RejectionKind::LocalRestriction)
            .count();
        tracing::info!(
            metric = "SchemaPromotionRejected",
            schema = %target.schema_name,
            environment = %target.environment,
            count = reasons.len(),
            restrictions,
            unsupported = reasons.len() - restrictions,
            "schema version append rejected by the compatibility gate"
        );
        return Err(api::validation_error(incompatibility_message(&reasons)));
    }
    Ok(())
}

/// How many incompatibilities make it into a `400` message before truncation.
const MAX_REPORTED_INCOMPATIBILITIES: usize = 10;

/// Fetches the current latest version's document from the stage-selected
/// registry. The gate never caches: a stale predecessor read would weaken it,
/// and appends are rare.
async fn latest_schema_document(
    clients: &Clients,
    target: &SchemaTarget,
) -> Result<Value, ControlPlaneError> {
    let (_, content) = describe_latest(clients, target).await?;
    serde_json::from_str(&content).map_err(|e| {
        ControlPlaneError::Schemas(format!("stored latest schema is not parseable JSON: {e}"))
    })
}

/// The raw latest-version read: version number and unparsed content.
pub(crate) async fn describe_latest(
    clients: &Clients,
    target: &SchemaTarget,
) -> Result<(Option<i64>, String), ControlPlaneError> {
    let output = clients
        .schemas
        .describe_schema()
        .registry_name(&target.registry)
        .schema_name(target.schema_name.to_string())
        .send()
        .await
        .map_err(|e| {
            use aws_sdk_schemas::operation::describe_schema::DescribeSchemaError;
            let service_err = e.into_service_error();
            match &service_err {
                DescribeSchemaError::NotFoundException(_) => ControlPlaneError::SchemaNotFound {
                    schema_name: target.schema_name.to_string(),
                },
                other => {
                    tracing::error!(error = %other, "describe latest schema failed");
                    ControlPlaneError::Schemas(other.to_string())
                }
            }
        })?;
    let version = output.schema_version().and_then(|v| v.parse::<i64>().ok());
    let content = output.content().ok_or_else(|| {
        ControlPlaneError::Schemas("describe schema carried no content".to_string())
    })?;
    Ok((version, content.to_string()))
}

/// Renders the rejection message: each relevant path with its reason,
/// capped so a wholesale rewrite cannot produce an unbounded body. Local
/// restrictions sort ahead of unsupported proofs so the actionable
/// tightenings are never crowded out of the cap. The prefix stays
/// neutral — the gate reports that compatibility could not be established,
/// which covers both observed local restrictions and changes the bounded
/// proof does not support.
fn incompatibility_message(incompatibilities: &[Incompatibility]) -> String {
    use konductor_common::schema_compatibility::RejectionKind;

    let mut message = String::from(
        "backward compatibility with the current latest version could not be established: ",
    );
    let mut ordered: Vec<&Incompatibility> = incompatibilities
        .iter()
        .filter(|i| i.rule.kind() == RejectionKind::LocalRestriction)
        .chain(
            incompatibilities
                .iter()
                .filter(|i| i.rule.kind() == RejectionKind::UnsupportedProof),
        )
        .collect();
    let shown: Vec<&Incompatibility> = ordered
        .drain(..)
        .take(MAX_REPORTED_INCOMPATIBILITIES)
        .collect();
    if let Some(first) = shown.first() {
        message.push_str(&format!("{}: {}", display_path(&first.path), first.rule));
        for entry in &shown[1..] {
            message.push_str(&format!("; {}: {}", display_path(&entry.path), entry.rule));
        }
    }
    if incompatibilities.len() > MAX_REPORTED_INCOMPATIBILITIES {
        message.push_str(&format!(
            "; … and {} more",
            incompatibilities.len() - MAX_REPORTED_INCOMPATIBILITIES
        ));
    }
    message
}

fn display_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

pub(crate) struct SchemaTarget {
    pub(crate) schema_name: SchemaName,
    pub(crate) registry: String,
    pub(crate) producer_domain: String,
    pub(crate) event_name: String,
    pub(crate) environment: String,
}

/// The shared gate for both mutations: event-name feasibility, publisher
/// authorization, and an ACTIVE domain — all before any registry access.
async fn prepare<A: ResourceAuthorizer>(
    clients: &Clients,
    authorizer: &A,
    payload: &Value,
    principal: &TrustedPrincipal,
    domain: &str,
    event_name: &str,
    action: &'static str,
) -> Result<SchemaTarget, ControlPlaneError> {
    rule_name("aa", event_name)?;

    let stage = payload
        .pointer("/requestContext/stage")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| api::validation_error("request context is missing the API stage"))?;
    let authoring = environments::authoring_environment()?;
    if stage != authoring {
        return Err(ControlPlaneError::AuthoringEnvironmentOnly {
            stage: stage.to_string(),
            authoring_environment: authoring,
        });
    }

    if authorizer.authorize(principal, action, domain).await != AuthorizationDecision::Allow {
        return Err(ControlPlaneError::AccessDenied);
    }

    require_active_domain(clients, domain).await?;

    let organization = std::env::var("ORGANIZATION")
        .map_err(|_| ControlPlaneError::Schemas("ORGANIZATION not set".to_string()))?;

    let resolver = NamespaceResolver::default_pattern();
    let schema_name =
        SchemaName::for_domain(&resolver.namespace(&organization, domain), event_name);
    let registry = resolver.schema_registry_name(stage, &organization, domain);
    Ok(SchemaTarget {
        schema_name,
        registry,
        producer_domain: domain.to_string(),
        event_name: event_name.to_string(),
        environment: stage.to_string(),
    })
}

/// Promotion-following extension hinges on this signal: every committed
/// registry write announces itself on the control-plane bus. `UpdateSchema`
/// appends are not safely retryable (a retry would append yet another
/// version), so an emission failure is logged, never surfaced — a 5xx would
/// invite exactly that retry.
async fn emit_version_published(
    clients: &Clients,
    target: &SchemaTarget,
    schema_version: Option<&str>,
) {
    let bus = match std::env::var("KONDUKTOR_EVENT_BUS") {
        Ok(bus) => bus,
        Err(_) => {
            log_missing_bus(target);
            return;
        }
    };
    let entry = version_published_entry(bus, target, schema_version);
    match clients.eb.put_events().entries(entry).send().await {
        Ok(output) if output.failed_entry_count() > 0 => log_rejected(&output, target),
        Ok(_) => log_published(target),
        Err(error) => log_failed(&error, target),
    }
}

fn log_missing_bus(target: &SchemaTarget) {
    tracing::error!(
        schema = %target.schema_name,
        "KONDUKTOR_EVENT_BUS not set — SchemaVersionPublished not emitted"
    );
}

fn log_published(target: &SchemaTarget) {
    tracing::info!(
        schema = %target.schema_name,
        environment = %target.environment,
        "SchemaVersionPublished emitted"
    );
}

fn log_rejected(
    output: &aws_sdk_eventbridge::operation::put_events::PutEventsOutput,
    target: &SchemaTarget,
) {
    let cause = output
        .entries()
        .first()
        .and_then(|e| e.error_message())
        .unwrap_or("unknown");
    tracing::error!(cause, schema = %target.schema_name, "SchemaVersionPublished entry rejected");
}

fn log_failed(
    error: &aws_sdk_eventbridge::error::SdkError<
        aws_sdk_eventbridge::operation::put_events::PutEventsError,
    >,
    target: &SchemaTarget,
) {
    tracing::error!(error = %error, schema = %target.schema_name, "SchemaVersionPublished emission failed");
}

fn version_published_entry(
    bus: String,
    target: &SchemaTarget,
    schema_version: Option<&str>,
) -> aws_sdk_eventbridge::types::PutEventsRequestEntry {
    let detail = serde_json::json!({
        "producerDomain": target.producer_domain,
        "schemaName": target.schema_name.to_string(),
        "environment": target.environment,
        "version": schema_version.and_then(|v| v.parse::<i64>().ok()),
        "publishedAt": chrono::Utc::now().to_rfc3339(),
    });
    aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
        .event_bus_name(bus)
        .source("konductor.schema-management")
        .detail_type("SchemaVersionPublished")
        .detail(detail.to_string())
        .build()
}

async fn require_active_domain(
    clients: &Clients,
    domain_name: &str,
) -> Result<(), ControlPlaneError> {
    let table = std::env::var("DOMAINS_TABLE")
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

fn mutation_response(
    schema_name: &SchemaName,
    schema_version: Option<String>,
    duplicate: bool,
) -> Result<Value, Error> {
    match schema_version.and_then(|v| v.parse::<i64>().ok()) {
        Some(version) => Ok(api::success_response(
            if duplicate { 200 } else { 201 },
            &SchemaMutationResponse {
                schema_name: schema_name.to_string(),
                schema_version: version,
                duplicate: duplicate.then_some(true),
            },
        )),
        None => Ok(api::from_control_plane_error(&ControlPlaneError::Schemas(
            "service response carried no usable schemaVersion".to_string(),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_eventbridge::operation::put_events::PutEventsOutput;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use konductor_common::grant::{BindingPrincipal, ResourceRole, RoleBinding};
    use konductor_common::grant_engine::InMemoryGrantEngine;
    use konductor_common::promotion::version_sk;
    use serde_json::json;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_TestPool";
    const CONTENT: &str =
        "{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"type\":\"object\"}";

    fn set_env() {
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var("DOMAINS_TABLE", "domains-test");
        std::env::set_var("KONDUKTOR_EVENT_BUS", "konductor-bus");
        std::env::set_var("ENVIRONMENTS", "dev,staging,prod");
        std::env::set_var("SCHEMA_VERSIONS_TABLE", "versions-test");
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

    fn oauth_event(resource: &str, body: Value) -> Value {
        json!({
            "httpMethod": "POST",
            "resource": resource,
            "requestContext": {
                "stage": "dev",
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

    fn iam_event(resource: &str, body: Value) -> Value {
        json!({
            "httpMethod": "POST",
            "resource": resource,
            "requestContext": {
                "stage": "dev",
                "identity": {
                    "accountId": "111122223333",
                    "userArn": "arn:aws:iam::111122223333:role/publisher"
                }
            },
            "body": body.to_string()
        })
    }

    fn create_body() -> Value {
        json!({
            "domain": "orders",
            "eventName": "OrderNotification",
            "schemaType": "JSONSchemaDraft4",
            "content": CONTENT
        })
    }

    fn append_body() -> Value {
        json!({
            "domain": "orders",
            "eventName": "OrderNotification",
            "content": CONTENT
        })
    }

    fn event(payload: Value) -> LambdaEvent<Value> {
        LambdaEvent::new(payload, Default::default())
    }

    fn status(resp: &Value) -> u16 {
        resp["statusCode"].as_u64().unwrap() as u16
    }

    fn body(resp: &Value) -> Value {
        serde_json::from_str(resp["body"].as_str().unwrap()).unwrap()
    }

    fn active_domain_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("domainName", s("orders"))
                .item("status", s("ACTIVE"))
                .build()
        })
    }

    fn create_schema_rule(registry: &'static str, version: &'static str) -> Rule {
        mock!(aws_sdk_schemas::Client::create_schema)
            .match_requests(move |req| {
                req.registry_name() == Some(registry)
                    && req.schema_name() == Some("acme-orders@OrderNotification")
                    && req.r#type() == Some(&aws_sdk_schemas::types::Type::JsonSchemaDraft4)
                    && req.content() == Some(CONTENT)
            })
            .then_output(move || {
                aws_sdk_schemas::operation::create_schema::CreateSchemaOutput::builder()
                    .schema_version(version)
                    .build()
            })
    }

    fn update_schema_rule(registry: &'static str, version: &'static str) -> Rule {
        update_schema_rule_for(registry, CONTENT, version)
    }

    fn update_schema_rule_for(
        registry: &'static str,
        content: &'static str,
        version: &'static str,
    ) -> Rule {
        mock!(aws_sdk_schemas::Client::update_schema)
            .match_requests(move |req| {
                req.registry_name() == Some(registry)
                    && req.schema_name() == Some("acme-orders@OrderNotification")
                    && req.content() == Some(content)
            })
            .then_output(move || {
                aws_sdk_schemas::operation::update_schema::UpdateSchemaOutput::builder()
                    .schema_version(version)
                    .build()
            })
    }

    fn describe_latest_rule(registry: &'static str, content: &'static str) -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema)
            .match_requests(move |req| {
                req.registry_name() == Some(registry)
                    && req.schema_name() == Some("acme-orders@OrderNotification")
                    && req.schema_version().is_none()
            })
            .then_output(move || {
                aws_sdk_schemas::operation::describe_schema::DescribeSchemaOutput::builder()
                    .content(content)
                    .schema_version("4")
                    .build()
            })
    }

    fn record_version_rule(version: i64) -> Rule {
        let sk = version_sk(version);
        mock!(aws_sdk_dynamodb::Client::put_item)
            .match_requests(move |req| {
                let Some(item) = req.item() else { return false };
                let get = |key: &str| {
                    item.get(key)
                        .and_then(|v| v.as_s().ok())
                        .map(|s| s.as_str())
                };
                req.table_name() == Some("versions-test")
                    && get("pk") == Some("orders#OrderNotification#dev")
                    && get("sk") == Some(sk.as_str())
                    && get("writtenBy") == Some("schema-admin")
                    && get("contentHash").is_some_and(|h| h.len() == 64)
            })
            .then_output(|| aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build())
    }

    fn record_version_failure_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::put_item).then_error(|| {
            aws_sdk_dynamodb::operation::put_item::PutItemError::generic(
                aws_sdk_dynamodb::error::ErrorMetadata::builder()
                    .message("simulated version store outage")
                    .build(),
            )
        })
    }

    fn update_no_change_conflict_rule() -> Rule {
        mock!(aws_sdk_schemas::Client::update_schema).then_error(|| {
            aws_sdk_schemas::operation::update_schema::UpdateSchemaError::generic(
                aws_sdk_schemas::error::ErrorMetadata::builder()
                    .code("ConflictException")
                    .message("no change since the previous schema version")
                    .build(),
            )
        })
    }

    fn version_published_rule(environment: &'static str, expected_version: i64) -> Rule {
        let needle = format!("\"version\":{expected_version}");
        let env_needle = format!("\"environment\":\"{environment}\"");
        mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(move |req| {
                req.entries().iter().any(|e| {
                    e.detail_type() == Some("SchemaVersionPublished")
                        && e.source() == Some("konductor.schema-management")
                        && e.event_bus_name() == Some("konductor-bus")
                        && e.detail().is_some_and(|d| {
                            d.contains("\"producerDomain\":\"orders\"")
                                && d.contains(&env_needle)
                                && d.contains("\"schemaName\":\"acme-orders@OrderNotification\"")
                                && d.contains(&needle)
                        })
                })
            })
            .then_output(|| PutEventsOutput::builder().failed_entry_count(0).build())
    }

    fn clients(ddb_rules: &[&Rule], schemas_rules: &[&Rule], eb_rules: &[&Rule]) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
            schemas: mock_client!(aws_sdk_schemas, RuleMode::MatchAny, schemas_rules),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, eb_rules),
        }
    }

    #[tokio::test]
    async fn create_schema_creates_version_1_in_the_stage_registry() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let create = create_schema_rule("acme-dev-orders-schema-registry", "1");
        let record = record_version_rule(1);
        let published = version_published_rule("dev", 1);
        let clients = clients(&[&domain, &record], &[&create], &[&published]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas", create_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 201);
        let body = body(&resp);
        assert_eq!(body["schemaName"], "acme-orders@OrderNotification");
        assert_eq!(body["schemaVersion"], 1);
        assert_eq!(published.num_calls(), 1, "create must announce the version");
        assert_eq!(record.num_calls(), 1, "create must record the content");
    }

    #[tokio::test]
    async fn create_schema_duplicate_returns_409() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let conflict = mock!(aws_sdk_schemas::Client::create_schema).then_error(|| {
            aws_sdk_schemas::operation::create_schema::CreateSchemaError::BadRequestException(
                aws_sdk_schemas::types::error::BadRequestException::builder()
                    .message("Schema acme-orders@OrderNotification already exists")
                    .build(),
            )
        });
        // The existing schema holds different content, so the ambiguous-retry
        // repair does not apply and no record write happens.
        let describe =
            describe_latest_rule("acme-dev-orders-schema-registry", "{\"type\":\"string\"}");
        let clients = clients(&[&active_domain_rule()], &[&conflict, &describe], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas", create_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 409);
        assert_eq!(body(&resp)["code"], "ConflictException");
    }

    #[tokio::test]
    async fn create_schema_without_binding_returns_403_without_resource_access() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![]);
        let clients = clients(&[], &[], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas", create_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 403);
    }

    #[tokio::test]
    async fn create_schema_for_inactive_domain_returns_404() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let inactive = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .item("domainName", s("orders"))
                .item("status", s("PROVISIONING"))
                .build()
        });
        let clients = clients(&[&inactive], &[], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas", create_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 404);
    }

    #[tokio::test]
    async fn create_schema_with_infeasible_event_name_returns_400() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[], &[]);
        let mut bad = create_body();
        bad["eventName"] = json!("Order Notification");

        let resp = handle(&clients, &authorizer, event(oauth_event("/schemas", bad)))
            .await
            .unwrap();

        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn create_schema_with_unsupported_type_returns_400() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[], &[]);
        let mut bad = create_body();
        bad["schemaType"] = json!("Avro");

        let resp = handle(&clients, &authorizer, event(oauth_event("/schemas", bad)))
            .await
            .unwrap();

        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn create_schema_with_malformed_draft4_content_returns_400() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        // Zero rules: content validation precedes any resource access.
        let clients = clients(&[], &[], &[]);
        let mut bad = create_body();
        bad["content"] = json!("not json");

        let resp = handle(&clients, &authorizer, event(oauth_event("/schemas", bad)))
            .await
            .unwrap();

        assert_eq!(status(&resp), 400);
        assert!(
            body(&resp)["message"].as_str().unwrap().contains("draft-4"),
            "400 must name the draft-4 constraint"
        );
    }

    #[tokio::test]
    async fn append_version_with_malformed_draft4_content_returns_400() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[], &[]);
        let mut bad = append_body();
        bad["content"] = json!(r#"{"type": "nonsense"}"#);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", bad)),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn create_schema_with_an_inexact_multiple_of_divisor_returns_400() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[], &[]);
        let mut bad = create_body();
        bad["content"] = json!(
            r#"{"type": "object", "properties": {"units": {"type": "integer", "multipleOf": 9007199254740993}}}"#
        );

        let resp = handle(&clients, &authorizer, event(oauth_event("/schemas", bad)))
            .await
            .unwrap();

        assert_eq!(status(&resp), 400);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(
            message.contains("multipleOf") && message.contains("'#/properties/units'"),
            "400 must name the keyword and its location: {message}"
        );
    }

    #[tokio::test]
    async fn append_version_with_an_inexact_multiple_of_divisor_returns_400() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[], &[]);
        let mut bad = append_body();
        bad["content"] = json!(r#"{"type": "integer", "multipleOf": 1e20}"#);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", bad)),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(
            message.contains("multipleOf") && message.contains("'#'"),
            "400 must name the keyword and its location: {message}"
        );
    }

    #[tokio::test]
    async fn append_version_creates_the_next_version() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", CONTENT);
        let update = update_schema_rule("acme-dev-orders-schema-registry", "5");
        let record = record_version_rule(5);
        let published = version_published_rule("dev", 5);
        let clients = clients(&[&domain, &record], &[&describe, &update], &[&published]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 201);
        assert_eq!(body(&resp)["schemaVersion"], 5);
        assert_eq!(
            published.num_calls(),
            1,
            "append must announce the new version"
        );
        assert_eq!(record.num_calls(), 1, "append must record the content");
    }

    #[tokio::test]
    async fn append_emission_carries_the_first_configured_environment() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", CONTENT);
        let update = update_schema_rule("acme-dev-orders-schema-registry", "5");
        let record = record_version_rule(5);
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = captured.clone();
        let published = mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(move |req| {
                for entry in req.entries() {
                    if let Some(detail) = entry.detail() {
                        sink.lock().unwrap().push(detail.to_string());
                    }
                }
                true
            })
            .then_output(|| PutEventsOutput::builder().failed_entry_count(0).build());
        let clients = clients(&[&domain, &record], &[&describe, &update], &[&published]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 201);
        let details = captured.lock().unwrap();
        assert_eq!(
            details.len(),
            1,
            "append must emit exactly one announcement"
        );
        let detail: serde_json::Value = serde_json::from_str(&details[0]).unwrap();
        assert_eq!(
            detail["environment"], "dev",
            "the announcement must name the authoring environment (environments[0])"
        );
    }

    #[tokio::test]
    async fn append_version_for_missing_schema_returns_404() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let not_found = mock!(aws_sdk_schemas::Client::describe_schema).then_error(|| {
            aws_sdk_schemas::operation::describe_schema::DescribeSchemaError::NotFoundException(
                aws_sdk_schemas::types::error::NotFoundException::builder()
                    .message("not found")
                    .build(),
            )
        });
        let clients = clients(&[&active_domain_rule()], &[&not_found], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 404);
    }

    #[tokio::test]
    async fn append_version_update_not_found_still_maps_to_404() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", CONTENT);
        let not_found = mock!(aws_sdk_schemas::Client::update_schema).then_error(|| {
            aws_sdk_schemas::operation::update_schema::UpdateSchemaError::NotFoundException(
                aws_sdk_schemas::types::error::NotFoundException::builder()
                    .message("not found")
                    .build(),
            )
        });
        let clients = clients(&[&active_domain_rule()], &[&describe, &not_found], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 404);
    }

    #[tokio::test]
    async fn append_version_without_binding_returns_403() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![]);
        let clients = clients(&[], &[], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 403);
    }

    #[tokio::test]
    async fn iam_caller_with_account_binding_uses_identical_semantics() {
        set_env();
        let authorizer =
            InMemoryGrantEngine::new(vec![iam_publisher_binding("111122223333", "orders")]);
        let domain = active_domain_rule();
        let create = create_schema_rule("acme-dev-orders-schema-registry", "1");
        let record = record_version_rule(1);
        let published = version_published_rule("dev", 1);
        let clients = clients(&[&domain, &record], &[&create], &[&published]);

        let resp = handle(
            &clients,
            &authorizer,
            event(iam_event("/schemas", create_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 201);
        assert_eq!(body(&resp)["schemaName"], "acme-orders@OrderNotification");
    }

    #[tokio::test]
    async fn grant_engine_store_error_fails_closed_with_403() {
        set_env();
        let store_error = mock!(aws_sdk_dynamodb::Client::get_item).then_error(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemError::generic(
                aws_sdk_dynamodb::error::ErrorMetadata::builder()
                    .message("simulated bindings store outage")
                    .build(),
            )
        });
        let store_client = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&store_error]);
        let authorizer = konductor_common::grant_engine::DynamoGrantEngine::new(
            konductor_common::grant_store::BindingStore::new(store_client.clone(), "bindings-test"),
            konductor_common::entitlement_store::EntitlementStore::new(
                store_client,
                "bindings-test",
            ),
        );
        let clients = clients(&[], &[], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas", create_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 403);
    }

    #[tokio::test]
    async fn append_on_a_later_stage_is_rejected_before_any_resource_access() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[], &[]);
        let mut payload = oauth_event("/schemas/versions", append_body());
        payload["requestContext"]["stage"] = json!("staging");

        let resp = handle(&clients, &authorizer, event(payload)).await.unwrap();

        assert_eq!(status(&resp), 400);
        let body = body(&resp);
        assert_eq!(body["code"], "AuthoringEnvironmentOnly");
        let message = body["message"].as_str().unwrap();
        assert!(
            message.contains("dev"),
            "names the authoring environment: {message}"
        );
        assert!(
            message.contains("promotion"),
            "points at promotion: {message}"
        );
    }

    #[tokio::test]
    async fn create_on_a_later_stage_is_rejected() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[], &[]);
        let mut payload = oauth_event("/schemas", create_body());
        payload["requestContext"]["stage"] = json!("prod");

        let resp = handle(&clients, &authorizer, event(payload)).await.unwrap();

        assert_eq!(status(&resp), 400);
        assert_eq!(body(&resp)["code"], "AuthoringEnvironmentOnly");
    }

    #[tokio::test]
    async fn platform_admin_append_on_a_later_stage_is_rejected() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![]);
        let clients = clients(&[], &[], &[]);
        let mut payload = oauth_event("/schemas/versions", append_body());
        payload["requestContext"]["stage"] = json!("staging");
        payload["requestContext"]["authorizer"]["globalRoles"] = json!("[\"platform-admin\"]");

        let resp = handle(&clients, &authorizer, event(payload)).await.unwrap();

        assert_eq!(status(&resp), 400);
        assert_eq!(body(&resp)["code"], "AuthoringEnvironmentOnly");
    }

    #[tokio::test]
    async fn iam_append_on_a_later_stage_is_rejected() {
        set_env();
        let authorizer =
            InMemoryGrantEngine::new(vec![iam_publisher_binding("111122223333", "orders")]);
        let clients = clients(&[], &[], &[]);
        let mut payload = iam_event("/schemas/versions", append_body());
        payload["requestContext"]["stage"] = json!("staging");

        let resp = handle(&clients, &authorizer, event(payload)).await.unwrap();

        assert_eq!(status(&resp), 400);
        assert_eq!(body(&resp)["code"], "AuthoringEnvironmentOnly");
    }

    #[tokio::test]
    async fn create_schema_with_local_references_composes_and_is_stored_unchanged() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let local_ref_content = r##"{
            "$schema": "http://json-schema.org/draft-04/schema#",
            "type": "object",
            "properties": {"item": {"$ref": "#/definitions/item"}},
            "definitions": {"item": {"type": "object", "properties": {"sku": {"type": "string"}}, "required": ["sku"]}}
        }"##;
        let domain = active_domain_rule();
        let create = mock!(aws_sdk_schemas::Client::create_schema)
            .match_requests(move |req| {
                req.registry_name() == Some("acme-dev-orders-schema-registry")
                    && req.schema_name() == Some("acme-orders@OrderNotification")
                    && req.content() == Some(local_ref_content)
            })
            .then_output(move || {
                aws_sdk_schemas::operation::create_schema::CreateSchemaOutput::builder()
                    .schema_version("1")
                    .build()
            });
        let record = record_version_rule(1);
        let published = version_published_rule("dev", 1);
        let clients = clients(&[&domain, &record], &[&create], &[&published]);
        let mut body = create_body();
        body["content"] = json!(local_ref_content);

        let resp = handle(&clients, &authorizer, event(oauth_event("/schemas", body)))
            .await
            .unwrap();

        // The mock rule only matches when the stored content is exactly the
        // submitted document: local references survive authoring.
        assert_eq!(status(&resp), 201);
    }

    #[tokio::test]
    async fn create_schema_with_external_references_returns_400_before_registry_access() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        // Zero rules: the composability gate precedes any SDK access.
        let clients = clients(&[], &[], &[]);
        let mut bad = create_body();
        bad["content"] = json!(
            r#"{"$schema": "http://json-schema.org/draft-04/schema#", "type": "object", "properties": {"x": {"$ref": "https://example.com/remote.json"}}}"#
        );

        let resp = handle(&clients, &authorizer, event(oauth_event("/schemas", bad)))
            .await
            .unwrap();

        assert_eq!(status(&resp), 400);
        let response_body = body(&resp);
        let message = response_body["message"].as_str().unwrap();
        // External references fail closed at authoring time — either the
        // draft-4 compile gate or the composability gate rejects them, and
        // both name the offending reference without any registry access.
        assert!(
            message.contains("example.com"),
            "400 must name the offending reference: {message}"
        );
    }

    #[tokio::test]
    async fn append_version_with_an_offline_unresolvable_reference_fails_the_composability_gate() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(&[], &[], &[]);
        let mut bad = append_body();
        // Compiles standalone — the vendor extension is ignored by the
        // draft-4 compiler — but the composability gate conservatively
        // rejects any reference that cannot resolve offline, wherever it
        // appears in the document.
        bad["content"] = json!(
            r#"{"$schema": "http://json-schema.org/draft-04/schema#", "type": "object", "properties": {"orderId": {"type": "integer"}}, "x-examples": {"$ref": "https://example.com/dead-space.json"}}"#
        );

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", bad)),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
        let response_body = body(&resp);
        let message = response_body["message"].as_str().unwrap();
        assert!(
            message.contains("offline composed contract"),
            "400 must name the composability constraint: {message}"
        );
    }

    #[tokio::test]
    async fn append_version_with_identical_content_completes_the_record_as_duplicate() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        // The gate's describe and the repair's describe both hit this rule.
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", CONTENT);
        let conflict = update_no_change_conflict_rule();
        let record = record_version_rule(4);
        let clients = clients(&[&domain, &record], &[&describe, &conflict], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 200);
        let body = body(&resp);
        assert_eq!(body["schemaVersion"], 4);
        assert_eq!(body["duplicate"], true);
        assert_eq!(
            record.num_calls(),
            1,
            "the missing record must be completed"
        );
    }

    #[tokio::test]
    async fn create_schema_duplicate_with_equal_content_completes_the_record() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let conflict = mock!(aws_sdk_schemas::Client::create_schema).then_error(|| {
            aws_sdk_schemas::operation::create_schema::CreateSchemaError::BadRequestException(
                aws_sdk_schemas::types::error::BadRequestException::builder()
                    .message("Schema acme-orders@OrderNotification already exists")
                    .build(),
            )
        });
        // The registry normalizes whitespace: canonically equal, bytes differ.
        let describe = describe_latest_rule(
            "acme-dev-orders-schema-registry",
            "{ \"$schema\": \"http://json-schema.org/draft-04/schema#\", \"type\": \"object\" }",
        );
        let record = record_version_rule(4);
        let clients = clients(
            &[&active_domain_rule(), &record],
            &[&conflict, &describe],
            &[],
        );

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas", create_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 409);
        assert_eq!(body(&resp)["code"], "ConflictException");
        assert_eq!(record.num_calls(), 1, "the repair must complete the record");
    }

    #[tokio::test]
    async fn append_version_record_write_failure_returns_an_error_for_safe_retry() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", CONTENT);
        let update = update_schema_rule("acme-dev-orders-schema-registry", "5");
        let record_fail = record_version_failure_rule();
        // No put_events rule: the version is not announced when its content
        // record failed, and the safe retry republishes it.
        let clients = clients(&[&domain, &record_fail], &[&describe, &update], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 500);
    }

    #[tokio::test]
    async fn emission_failure_does_not_fail_the_committed_mutation() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", CONTENT);
        let update = update_schema_rule("acme-dev-orders-schema-registry", "6");
        let record = record_version_rule(6);
        let bus_down = mock!(aws_sdk_eventbridge::Client::put_events).then_error(|| {
            aws_sdk_eventbridge::operation::put_events::PutEventsError::InternalException(
                aws_sdk_eventbridge::types::error::InternalException::builder()
                    .message("bus unavailable")
                    .build(),
            )
        });
        let clients = clients(&[&domain, &record], &[&describe, &update], &[&bus_down]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body())),
        )
        .await
        .unwrap();

        // The committed registry write reports success even though the
        // promotion signal could not be sent: a surfaced 5xx would invite a
        // retry, and only an identical retry converges (no-change conflict).
        assert_eq!(status(&resp), 201);
        assert_eq!(body(&resp)["schemaVersion"], 6);
    }

    const LATEST_V4: &str = "{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"type\":\"object\",\"properties\":{\"sku\":{\"type\":\"string\"}}}";

    fn append_body_with(content: &str) -> Value {
        json!({
            "domain": "orders",
            "eventName": "OrderNotification",
            "content": content
        })
    }

    #[tokio::test]
    async fn append_version_newly_requiring_a_property_returns_400_naming_the_path() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", LATEST_V4);
        let clients = clients(&[&active_domain_rule()], &[&describe], &[]);
        let tightening = "{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"type\":\"object\",\"properties\":{\"sku\":{\"type\":\"string\"}},\"required\":[\"sku\"]}";

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event(
                "/schemas/versions",
                append_body_with(tightening),
            )),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(
            message.contains("/sku"),
            "400 must name the offending path: {message}"
        );
        assert!(
            message.contains("newly required"),
            "400 must name the violated rule: {message}"
        );
    }

    #[tokio::test]
    async fn append_version_removing_a_declared_property_returns_400() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let with_legacy = "{\"type\":\"object\",\"properties\":{\"sku\":{\"type\":\"string\"},\"legacy\":{\"type\":\"string\"}}}";
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", with_legacy);
        let clients = clients(&[&active_domain_rule()], &[&describe], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event(
                "/schemas/versions",
                append_body_with(LATEST_V4),
            )),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
        assert!(body(&resp)["message"].as_str().unwrap().contains("/legacy"));
    }

    #[tokio::test]
    async fn append_version_adding_an_optional_property_to_a_closed_object_appends() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let closed = "{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"type\":\"object\",\"properties\":{\"sku\":{\"type\":\"string\"}},\"additionalProperties\":false}";
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", closed);
        let additive = "{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"type\":\"object\",\"properties\":{\"sku\":{\"type\":\"string\"},\"quantity\":{\"type\":\"integer\"}},\"additionalProperties\":false}";
        let update = update_schema_rule_for("acme-dev-orders-schema-registry", additive, "5");
        let record = record_version_rule(5);
        let published = version_published_rule("dev", 5);
        let clients = clients(
            &[&active_domain_rule(), &record],
            &[&describe, &update],
            &[&published],
        );

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body_with(additive))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 201);
        assert_eq!(body(&resp)["schemaVersion"], 5);
    }

    #[tokio::test]
    async fn append_version_optional_property_constraint_in_open_object_returns_400() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", LATEST_V4);
        // No update or event rules: a rejected append must never reach
        // UpdateSchema or emit a publication event.
        let clients = clients(&[&active_domain_rule()], &[&describe], &[]);
        let additive = "{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"type\":\"object\",\"properties\":{\"sku\":{\"type\":\"string\"},\"quantity\":{\"type\":\"integer\"}}}";

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body_with(additive))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(
            message.contains("/quantity"),
            "the rejection must name the new property: {message}"
        );
        assert!(
            message.contains("not proven to accept previously allowed values"),
            "the rejection must explain the unproven constraint: {message}"
        );
    }

    #[tokio::test]
    async fn append_version_beyond_the_proof_budget_returns_400() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        // A wide flat document stays under the JSON parser's recursion
        // limit but exceeds the checker's node-visit budget.
        let mut wide = String::from("{\"type\":\"object\",\"properties\":{");
        for i in 0..4_500 {
            if i > 0 {
                wide.push(',');
            }
            wide.push_str(&format!("\"p{i}\":{{}}"));
        }
        wide.push_str("}}");
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", LATEST_V4);
        let clients = clients(&[&active_domain_rule()], &[&describe], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body_with(&wide))),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(
            message.contains("could not be established"),
            "budget exhaustion must not claim demonstrated incompatibility: {message}"
        );
    }

    #[tokio::test]
    async fn append_version_gate_admits_no_platform_admin_override() {
        set_env();
        let admin = json!({
            "httpMethod": "POST",
            "resource": "/schemas/versions",
            "requestContext": {
                "stage": "dev",
                "authorizer": {
                    "authenticationMechanism": "oauth",
                    "principalId": "sub-admin",
                    "issuer": ISSUER,
                    "clientId": "client-1",
                    "globalRoles": "[\"platform-admin\"]",
                    "teamGroups": "[\"team-orders\"]"
                }
            },
            "body": append_body_with(
                "{\"type\":\"object\",\"properties\":{\"sku\":{\"type\":\"string\"}},\"required\":[\"sku\"]}"
            ).to_string()
        });
        // Authorized (publisher binding) and platform-admin: neither
        // standing bypasses the gate.
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", LATEST_V4);
        let clients = clients(&[&active_domain_rule()], &[&describe], &[]);

        let resp = handle(&clients, &authorizer, event(admin)).await.unwrap();

        assert_eq!(
            status(&resp),
            400,
            "no role bypasses the compatibility gate: {}",
            body(&resp)["message"]
        );
    }

    #[tokio::test]
    async fn append_version_rejection_surfaces_local_restrictions_first() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        // Twelve unsupported proof cases (pattern changes) crowd the
        // traversal-order reasons past the message cap, while one
        // actionable local tightening (a newly required property) must
        // still be reported first.
        let mut candidate =
            String::from("{\"type\":\"object\",\"properties\":{\"sku\":{\"type\":\"string\"}");
        for i in 0..12 {
            candidate.push_str(&format!(
                ",\"p{i}\":{{\"type\":\"string\",\"pattern\":\"^{i}\"}}"
            ));
        }
        candidate.push_str("},\"required\":[\"sku\"]}");
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", LATEST_V4);
        let clients = clients(&[&active_domain_rule()], &[&describe], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event(
                "/schemas/versions",
                append_body_with(&candidate),
            )),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(
            message.contains("property newly required"),
            "the local restriction must survive the message cap: {message}"
        );
        assert!(
            message.contains("… and 3 more"),
            "10 of the 13 reasons are reported: {message}"
        );
    }

    #[tokio::test]
    async fn append_version_is_judged_against_the_latest_version_only() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", LATEST_V4);
        let update = update_schema_rule_for("acme-dev-orders-schema-registry", LATEST_V4, "5");
        let record = record_version_rule(5);
        let published = version_published_rule("dev", 5);
        let clients = clients(
            &[&active_domain_rule(), &record],
            &[&describe, &update],
            &[&published],
        );

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event(
                "/schemas/versions",
                append_body_with(LATEST_V4),
            )),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 201);
        assert_eq!(
            describe.num_calls(),
            1,
            "exactly one version-less describe fetches the latest"
        );
    }

    #[tokio::test]
    async fn append_version_rejection_emits_the_promotion_rejected_metric() {
        use std::io::Write;
        use std::sync::{Arc, Mutex, Once, OnceLock};

        #[derive(Clone, Default)]
        struct Capture(Arc<Mutex<Vec<u8>>>);
        impl Write for Capture {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
            type Writer = Capture;
            fn make_writer(&'a self) -> Capture {
                self.clone()
            }
        }

        // Thread-local subscribers race with parallel tests: callsite
        // interest is cached process-wide, so a thread running without a
        // subscriber can mark the rejection callsite disabled mid-test.
        // A global default is set exactly once and never removed, keeping
        // every callsite enabled; the unique event name attributes the
        // metric line to this test.
        static LOGS: OnceLock<Capture> = OnceLock::new();
        static INIT: Once = Once::new();
        let capture = LOGS.get_or_init(Capture::default).clone();
        INIT.call_once(|| {
            let _ = tracing::subscriber::set_global_default(
                tracing_subscriber::fmt()
                    .with_writer(LOGS.get().unwrap().clone())
                    .with_ansi(false)
                    .finish(),
            );
        });

        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let latest = "{\"type\":\"object\",\"properties\":{\"sku\":{\"type\":\"string\"}}}";
        let describe = mock!(aws_sdk_schemas::Client::describe_schema)
            .match_requests(|req| {
                req.registry_name() == Some("acme-dev-orders-schema-registry")
                    && req.schema_name() == Some("acme-orders@MetricProbeEvent")
                    && req.schema_version().is_none()
            })
            .then_output(move || {
                aws_sdk_schemas::operation::describe_schema::DescribeSchemaOutput::builder()
                    .content(latest)
                    .schema_version("4")
                    .build()
            });
        let clients = clients(&[&active_domain_rule()], &[&describe], &[]);
        let tightening = json!({
            "domain": "orders",
            "eventName": "MetricProbeEvent",
            "content": "{\"type\":\"object\",\"properties\":{\"sku\":{\"type\":\"string\"}},\"required\":[\"sku\"]}"
        });

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", tightening)),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
        let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert!(
            logs.lines()
                .any(|line| line.contains("SchemaPromotionRejected")
                    && line.contains("MetricProbeEvent")),
            "rejection must carry the metric field: {logs}"
        );
    }

    // add-platform-metadata-composition task 2.3: with the metadata
    // composability check sharing this handler, the business compatibility
    // gate must still reject tightenings — including in documents that use
    // the local-reference style the composer supports. Reference-bearing
    // changes fail closed on the gate's reference guard before local rules.
    #[tokio::test]
    async fn business_tightening_still_rejects_for_metadata_composable_content() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let latest = "{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"type\":\"object\",\"definitions\":{\"sku\":{\"type\":\"string\"}},\"properties\":{\"sku\":{\"$ref\":\"#/definitions/sku\"}}}";
        let tightening = "{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"type\":\"object\",\"definitions\":{\"sku\":{\"type\":\"string\"}},\"properties\":{\"sku\":{\"$ref\":\"#/definitions/sku\"}},\"required\":[\"sku\"]}";
        let describe = describe_latest_rule("acme-dev-orders-schema-registry", latest);
        // No update rule: a rejected append must never reach UpdateSchema.
        let clients = clients(&[&active_domain_rule()], &[&describe], &[]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event(
                "/schemas/versions",
                append_body_with(tightening),
            )),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 400);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(
            message.contains("reference-dependent change is unsupported"),
            "the business gate, not the composability check, must reject the change: {message}"
        );
    }
}
