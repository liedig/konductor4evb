//! Publisher schema administration: `POST /schemas` creates a schema
//! (EventBridge `CreateSchema` semantics) and `POST /schemas/versions`
//! appends a version (`UpdateSchema` semantics) on both control-plane
//! surfaces. The grant engine authorizes the publisher action for the target
//! domain before any resource access; Konductor derives the canonical schema
//! name and the stage-selected environment's registry. Schemas are
//! append-only: no delete, retire, or historical-version replacement.

use lambda_runtime::{Error, LambdaEvent};
use serde_json::Value;

use konductor_common::api;
use konductor_common::dynamodb::{get_s, s};
use konductor_common::envelope::{rule_name, SchemaName};
use konductor_common::error::ControlPlaneError;
use konductor_common::grant_engine::actions;
use konductor_common::metadata::composition::check_offline_composability;
use konductor_common::models::{
    AppendSchemaVersionRequest, CreateSchemaRequest, DomainStatus, SchemaMutationResponse,
};
use konductor_common::namespace::NamespaceResolver;
use konductor_common::principal::{AuthorizationDecision, ResourceAuthorizer, TrustedPrincipal};
use konductor_common::principal_context::log_principal;
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
            emit_version_published(clients, &target, output.schema_version.as_deref()).await;
            mutation_response(&target.schema_name, output.schema_version)
        }
        Err(e) => {
            use aws_sdk_schemas::operation::create_schema::CreateSchemaError;
            let service_err = e.into_service_error();
            // The service model has no ConflictException for CreateSchema: a
            // duplicate surfaces as BadRequestException ("already exists"),
            // with a code() fallback should the service add a conflict code.
            let mapped = match &service_err {
                CreateSchemaError::BadRequestException(bad_request)
                    if bad_request
                        .message()
                        .unwrap_or_default()
                        .contains("already exists") =>
                {
                    ControlPlaneError::SchemaAlreadyExists {
                        schema_name: target.schema_name.to_string(),
                    }
                }
                CreateSchemaError::BadRequestException(_) => {
                    api::validation_error("create schema rejected the schema document")
                }
                other if other.code() == Some("ConflictException") => {
                    ControlPlaneError::SchemaAlreadyExists {
                        schema_name: target.schema_name.to_string(),
                    }
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
            emit_version_published(clients, &target, output.schema_version.as_deref()).await;
            mutation_response(&target.schema_name, output.schema_version)
        }
        Err(e) => {
            let service_err = e.into_service_error();
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

struct SchemaTarget {
    schema_name: SchemaName,
    registry: String,
    producer_domain: String,
    environment: String,
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

    if authorizer.authorize(principal, action, domain).await != AuthorizationDecision::Allow {
        return Err(ControlPlaneError::AccessDenied);
    }

    require_active_domain(clients, domain).await?;

    let organization = std::env::var("ORGANIZATION")
        .map_err(|_| ControlPlaneError::Schemas("ORGANIZATION not set".to_string()))?;
    let stage = payload
        .pointer("/requestContext/stage")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| api::validation_error("request context is missing the API stage"))?;

    let resolver = NamespaceResolver::default_pattern();
    let schema_name =
        SchemaName::for_domain(&resolver.namespace(&organization, domain), event_name);
    let registry = resolver.schema_registry_name(stage, &organization, domain);
    Ok(SchemaTarget {
        schema_name,
        registry,
        producer_domain: domain.to_string(),
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
) -> Result<Value, Error> {
    match schema_version.and_then(|v| v.parse::<i64>().ok()) {
        Some(version) => Ok(api::success_response(
            201,
            &SchemaMutationResponse {
                schema_name: schema_name.to_string(),
                schema_version: version,
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
    use serde_json::json;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_TestPool";
    const CONTENT: &str =
        "{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"type\":\"object\"}";

    fn set_env() {
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var("DOMAINS_TABLE", "domains-test");
        std::env::set_var("KONDUKTOR_EVENT_BUS", "konductor-bus");
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
                "stage": "prod",
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
                "stage": "prod",
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
        mock!(aws_sdk_schemas::Client::update_schema)
            .match_requests(move |req| {
                req.registry_name() == Some(registry)
                    && req.schema_name() == Some("acme-orders@OrderNotification")
                    && req.content() == Some(CONTENT)
            })
            .then_output(move || {
                aws_sdk_schemas::operation::update_schema::UpdateSchemaOutput::builder()
                    .schema_version(version)
                    .build()
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
        let create = create_schema_rule("acme-prod-orders-schema-registry", "1");
        let published = version_published_rule("prod", 1);
        let clients = clients(&[&domain], &[&create], &[&published]);

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
        let clients = clients(&[&active_domain_rule()], &[&conflict], &[]);

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
    async fn append_version_creates_the_next_version() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let update = update_schema_rule("acme-prod-orders-schema-registry", "5");
        let published = version_published_rule("prod", 5);
        let clients = clients(&[&domain], &[&update], &[&published]);

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
    }

    #[tokio::test]
    async fn append_version_for_missing_schema_returns_404() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let not_found = mock!(aws_sdk_schemas::Client::update_schema).then_error(|| {
            aws_sdk_schemas::operation::update_schema::UpdateSchemaError::NotFoundException(
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
        let create = create_schema_rule("acme-prod-orders-schema-registry", "1");
        let published = version_published_rule("prod", 1);
        let clients = clients(&[&domain], &[&create], &[&published]);

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
    async fn stage_selects_the_environment_registry() {
        set_env();
        std::env::set_var("ORGANIZATION", "acme");
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let update = update_schema_rule("acme-staging-orders-schema-registry", "2");
        let published = version_published_rule("staging", 2);
        let clients = clients(&[&domain], &[&update], &[&published]);
        let mut payload = oauth_event("/schemas/versions", append_body());
        payload["requestContext"]["stage"] = json!("staging");

        let resp = handle(&clients, &authorizer, event(payload)).await.unwrap();

        assert_eq!(status(&resp), 201);
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
                req.registry_name() == Some("acme-prod-orders-schema-registry")
                    && req.schema_name() == Some("acme-orders@OrderNotification")
                    && req.content() == Some(local_ref_content)
            })
            .then_output(move || {
                aws_sdk_schemas::operation::create_schema::CreateSchemaOutput::builder()
                    .schema_version("1")
                    .build()
            });
        let published = version_published_rule("prod", 1);
        let clients = clients(&[&domain], &[&create], &[&published]);
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
    async fn emission_failure_does_not_fail_the_committed_mutation() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let domain = active_domain_rule();
        let update = update_schema_rule("acme-prod-orders-schema-registry", "6");
        let bus_down = mock!(aws_sdk_eventbridge::Client::put_events).then_error(|| {
            aws_sdk_eventbridge::operation::put_events::PutEventsError::InternalException(
                aws_sdk_eventbridge::types::error::InternalException::builder()
                    .message("bus unavailable")
                    .build(),
            )
        });
        let clients = clients(&[&domain], &[&update], &[&bus_down]);

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body())),
        )
        .await
        .unwrap();

        // Appends are not safely retryable: the committed registry write
        // reports success even though the promotion signal could not be sent.
        assert_eq!(status(&resp), 201);
        assert_eq!(body(&resp)["schemaVersion"], 6);
    }
}
