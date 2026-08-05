//! Publisher schema administration: `POST /schemas` creates a schema
//! (EventBridge `CreateSchema` semantics) and `POST /schemas/versions`
//! appends a version (`UpdateSchema` semantics) on both control-plane
//! surfaces. The grant engine authorizes the publisher action for the target
//! domain before any resource access; Nexus derives the canonical schema
//! name and the stage-selected environment's registry. Schemas are
//! append-only: no delete, retire, or historical-version replacement.

use lambda_runtime::{Error, LambdaEvent};
use serde_json::Value;

use nexus_common::api;
use nexus_common::dynamodb::{get_s, s};
use nexus_common::envelope::{rule_name, SchemaName};
use nexus_common::error::ControlPlaneError;
use nexus_common::grant_engine::actions;
use nexus_common::models::{
    AppendSchemaVersionRequest, CreateSchemaRequest, DomainStatus, SchemaMutationResponse,
};
use nexus_common::namespace::NamespaceResolver;
use nexus_common::principal::{AuthorizationDecision, ResourceAuthorizer, TrustedPrincipal};
use nexus_common::principal_context::log_principal;

use aws_sdk_schemas::error::ProvideErrorMetadata;

const SUPPORTED_SCHEMA_TYPE: &str = "JSONSchemaDraft4";

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub schemas: aws_sdk_schemas::Client,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: aws_sdk_dynamodb::Client::new(config),
            schemas: aws_sdk_schemas::Client::new(config),
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
        Ok(output) => mutation_response(&target.schema_name, output.schema_version),
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
        Ok(output) => mutation_response(&target.schema_name, output.schema_version),
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
    })
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
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use nexus_common::grant::{BindingPrincipal, ResourceRole, RoleBinding};
    use nexus_common::grant_engine::InMemoryGrantEngine;
    use serde_json::json;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_TestPool";
    const CONTENT: &str =
        "{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"type\":\"object\"}";

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

    fn clients(ddb_rules: Vec<Rule>, schemas_rules: Vec<Rule>) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, &ddb_rules),
            schemas: mock_client!(aws_sdk_schemas, RuleMode::MatchAny, &schemas_rules),
        }
    }

    #[tokio::test]
    async fn create_schema_creates_version_1_in_the_stage_registry() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(
            vec![active_domain_rule()],
            vec![create_schema_rule("acme-prod-orders-schema-registry", "1")],
        );

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
        let clients = clients(vec![active_domain_rule()], vec![conflict]);

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
        let clients = clients(vec![], vec![]);

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
        let clients = clients(vec![inactive], vec![]);

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
        let clients = clients(vec![], vec![]);
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
        let clients = clients(vec![], vec![]);
        let mut bad = create_body();
        bad["schemaType"] = json!("Avro");

        let resp = handle(&clients, &authorizer, event(oauth_event("/schemas", bad)))
            .await
            .unwrap();

        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn append_version_creates_the_next_version() {
        set_env();
        let authorizer = InMemoryGrantEngine::new(vec![publisher_binding("team-orders", "orders")]);
        let clients = clients(
            vec![active_domain_rule()],
            vec![update_schema_rule("acme-prod-orders-schema-registry", "5")],
        );

        let resp = handle(
            &clients,
            &authorizer,
            event(oauth_event("/schemas/versions", append_body())),
        )
        .await
        .unwrap();

        assert_eq!(status(&resp), 201);
        assert_eq!(body(&resp)["schemaVersion"], 5);
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
        let clients = clients(vec![active_domain_rule()], vec![not_found]);

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
        let clients = clients(vec![], vec![]);

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
        let clients = clients(
            vec![active_domain_rule()],
            vec![create_schema_rule("acme-prod-orders-schema-registry", "1")],
        );

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
        let authorizer = nexus_common::grant_engine::DynamoGrantEngine::new(
            nexus_common::grant_store::BindingStore::new(store_client, "bindings-test"),
        );
        let clients = clients(vec![], vec![]);

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
        let clients = clients(
            vec![active_domain_rule()],
            vec![update_schema_rule(
                "acme-staging-orders-schema-registry",
                "2",
            )],
        );
        let mut payload = oauth_event("/schemas/versions", append_body());
        payload["requestContext"]["stage"] = json!("staging");

        let resp = handle(&clients, &authorizer, event(payload)).await.unwrap();

        assert_eq!(status(&resp), 201);
    }
}
