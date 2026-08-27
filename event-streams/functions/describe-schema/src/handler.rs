use std::collections::BTreeMap;
use std::env;

use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use konductor_common::api;
use konductor_common::dynamodb::{get_s, get_string_list, s};
use konductor_common::entitlement_store::EntitlementStore;
use konductor_common::error::{ControlPlaneError, ValidationExceptionReason};
use konductor_common::grant::ResourceRole;
use konductor_common::grant_engine::{covers, has_role_binding};
use konductor_common::grant_store::BindingStore;
use konductor_common::models::{ApiError, DomainStatus};
use konductor_common::principal::{GlobalRole, TrustedPrincipal};
use konductor_common::principal_context::log_principal;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    schemas: &aws_sdk_schemas::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let bindings_table = env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    let entitlements_table =
        env::var("ENTITLEMENTS_TABLE").context("ENTITLEMENTS_TABLE not set")?;

    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "describe schema: deny (no trusted principal)");
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ));
        }
    };

    let Some(schema_name) = api::path_param(&event.payload, "schemaName") else {
        return Ok(validation("missing schemaName path parameter"));
    };
    let Some(domain_name) = api::query_param(&event.payload, "domain") else {
        return Ok(validation("missing required query parameter 'domain'"));
    };
    let consumer = api::query_param(&event.payload, "consumerDomain");

    if !authorize(
        ddb,
        &bindings_table,
        &entitlements_table,
        &principal,
        &consumer,
        &domain_name,
        &schema_name,
    )
    .await
    {
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::AccessDenied,
        ));
    }

    let output = ddb
        .get_item()
        .table_name(&domains_table)
        .key("domainName", s(&domain_name))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("failed to get domain record: {e}"))?;
    let environments = match output.item {
        Some(item)
            if get_s(&item, "status")
                .unwrap_or_default()
                .parse::<DomainStatus>()
                == Ok(DomainStatus::Active) =>
        {
            get_string_list(&item, "environments").unwrap_or_default()
        }
        _ => {
            return Ok(not_found(format!(
                "domain '{domain_name}' not found or not ACTIVE"
            )))
        }
    };

    let described = match describe_per_environment(
        schemas,
        &organization,
        &domain_name,
        &schema_name,
        &environments,
    )
    .await
    {
        Ok(d) => d,
        Err(response) => return Ok(response),
    };

    if described.latest_versions.is_empty() {
        return Ok(not_found(format!(
            "schema '{schema_name}' not found in any environment of domain '{domain_name}'"
        )));
    }

    let (source, detail_type) = schema_name
        .split_once('@')
        .unwrap_or((schema_name.as_str(), ""));
    Ok(api::success_response(
        200,
        &json!({
            "schemaName": schema_name,
            "source": source,
            "detailType": detail_type,
            "latestVersions": described.latest_versions,
            "content": described.content,
        }),
    ))
}

struct DescribedSchema {
    latest_versions: BTreeMap<String, i64>,
    content: Option<String>,
}

/// Per-environment latest versions; content comes from the highest latest
/// version so a consumer signing the contract sees what prod will enforce.
/// A schema missing from one environment simply has no entry there.
async fn describe_per_environment(
    schemas: &aws_sdk_schemas::Client,
    organization: &str,
    domain_name: &str,
    schema_name: &str,
    environments: &[String],
) -> Result<DescribedSchema, Value> {
    let mut latest_versions: BTreeMap<String, i64> = BTreeMap::new();
    let mut content: Option<String> = None;
    let mut content_version: i64 = 0;

    for env_name in environments {
        let registry = format!("{organization}-{env_name}-{domain_name}-schema-registry");
        match schemas
            .describe_schema()
            .registry_name(&registry)
            .schema_name(schema_name)
            .send()
            .await
        {
            Ok(described) => {
                let version: i64 = described
                    .schema_version
                    .as_deref()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                latest_versions.insert(env_name.clone(), version);
                if version > content_version {
                    content_version = version;
                    content = described.content;
                }
            }
            Err(e) => {
                let service_err = e.into_service_error();
                if !service_err.is_not_found_exception() {
                    tracing::error!(error = %service_err, registry, "describe schema failed");
                    return Err(api::error_response(
                        500,
                        ApiError {
                            code: "InternalError".to_string(),
                            message: "schema lookup failed".to_string(),
                            request_id: None,
                            reason: None,
                            field_list: None,
                        },
                    ));
                }
            }
        }
    }

    Ok(DescribedSchema {
        latest_versions,
        content,
    })
}

/// Consumer path: a `subscriber` binding on the queried consumer domain plus
/// a current covering entitlement (the same decision the engine makes for
/// subscribe); platform roles inspect without either. No consumer domain: a
/// platform role is required. Fail-closed throughout.
async fn authorize(
    ddb: &aws_sdk_dynamodb::Client,
    bindings_table: &str,
    entitlements_table: &str,
    principal: &TrustedPrincipal,
    consumer: &Option<String>,
    producer: &str,
    schema_name: &str,
) -> bool {
    if is_platform(principal) {
        return true;
    }
    let Some(consumer) = consumer else {
        // A publisher inspects its own catalog entries without a consumer
        // context; entitlement coverage is a consumer concept.
        return has_role_binding(
            &BindingStore::new(ddb.clone(), bindings_table),
            principal,
            ResourceRole::Publisher,
            producer,
        )
        .await;
    };
    let bound = has_role_binding(
        &BindingStore::new(ddb.clone(), bindings_table),
        principal,
        ResourceRole::Subscriber,
        consumer,
    )
    .await;
    if !bound {
        return false;
    }
    let store = EntitlementStore::new(ddb.clone(), entitlements_table);
    let detail_type = schema_name
        .split_once('@')
        .map(|(_, dt)| dt)
        .unwrap_or(schema_name);
    match store.entitlements_for_consumer(consumer).await {
        Ok(entitlements) => covers(&entitlements, producer, detail_type),
        Err(e) => {
            tracing::warn!(error = %e, "describe schema: coverage read failed closed");
            false
        }
    }
}

fn is_platform(principal: &TrustedPrincipal) -> bool {
    principal.is_platform_admin()
        || principal
            .global_roles()
            .contains(&GlobalRole::PlatformAuditor)
}

fn validation(message: &str) -> Value {
    api::from_control_plane_error(&ControlPlaneError::ValidationException {
        message: message.to_string(),
        reason: ValidationExceptionReason::FieldValidationFailed,
        field_list: None,
    })
}

fn not_found(message: String) -> Value {
    api::error_response(
        404,
        ApiError {
            code: "ResourceNotFoundException".to_string(),
            message,
            request_id: None,
            reason: None,
            field_list: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::operation::query::QueryOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_sdk_schemas::operation::describe_schema::DescribeSchemaOutput;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use lambda_runtime::Context as LambdaContext;

    use konductor_common::entitlement::{Entitlement, ScopePattern};
    use konductor_common::entitlement_store::entitlement_to_item;
    use konductor_common::grant::{BindingPrincipal, RoleBinding};
    use konductor_common::grant_store::binding_to_item;

    const SCHEMA: &str = "acme-orders@OrderNotification";

    fn set_env() {
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var("DOMAINS_TABLE", "domains");
        std::env::set_var("BINDINGS_TABLE", "bindings");
        std::env::set_var("ENTITLEMENTS_TABLE", "entitlements");
    }

    fn iam_context(account_id: &str) -> Value {
        json!({
            "identity": {
                "accountId": account_id,
                "userArn": format!("arn:aws:iam::{account_id}:role/operator"),
            }
        })
    }

    fn oauth_platform_context(role: &str) -> Value {
        json!({
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": "user-1",
                "issuer": "https://idp.example.com",
                "clientId": "client-1",
                "globalRoles": format!("[\"{role}\"]"),
                "teamGroups": "[]",
            }
        })
    }

    fn request(context: Value, params: Value) -> LambdaEvent<Value> {
        LambdaEvent::new(
            json!({
                "requestContext": context,
                "pathParameters": { "schemaName": SCHEMA },
                "queryStringParameters": params,
            }),
            LambdaContext::default(),
        )
    }

    fn body(response: &Value) -> Value {
        serde_json::from_str(response["body"].as_str().unwrap()).unwrap()
    }

    fn subscriber_binding_rule(account: &'static str, domain: &'static str) -> Rule {
        let binding = RoleBinding::new(
            "b-1",
            BindingPrincipal::aws_account(account).unwrap(),
            ResourceRole::Subscriber,
            domain,
            Utc::now(),
            "test",
        )
        .unwrap();
        let item = binding_to_item(&binding);
        let expected = item.clone();
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(move |req| {
                let Some(key) = req.key() else { return false };
                key.get("PK").and_then(|v| v.as_s().ok())
                    == expected.get("PK").and_then(|v| v.as_s().ok())
                    && key.get("SK").and_then(|v| v.as_s().ok())
                        == expected.get("SK").and_then(|v| v.as_s().ok())
            })
            .then_output(move || {
                GetItemOutput::builder()
                    .set_item(Some(item.clone()))
                    .build()
            })
    }

    fn entitlements_rule(consumer: &'static str, patterns: Vec<&'static str>) -> Rule {
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(move |req| {
                req.expression_attribute_values()
                    .and_then(|v| v.get(":pk"))
                    .and_then(|v| v.as_s().ok())
                    .map(|v| *v == format!("CONSUMER#{consumer}"))
                    .unwrap_or(false)
            })
            .then_output(move || {
                let items: Vec<HashMap<String, AttributeValue>> = patterns
                    .iter()
                    .map(|p| {
                        let e = Entitlement::new(
                            consumer,
                            "orders",
                            ScopePattern::new(*p).unwrap(),
                            Utc::now(),
                            "test",
                        )
                        .unwrap();
                        entitlement_to_item(&e)
                    })
                    .collect();
                QueryOutput::builder().set_items(Some(items)).build()
            })
    }

    fn domain_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.key()
                    .and_then(|k| k.get("domainName"))
                    .and_then(|v| v.as_s().ok())
                    .map(|v| v == "orders")
                    .unwrap_or(false)
            })
            .then_output(|| {
                GetItemOutput::builder()
                    .item("domainName", konductor_common::dynamodb::s("orders"))
                    .item("status", konductor_common::dynamodb::s("ACTIVE"))
                    .item(
                        "environments",
                        AttributeValue::L(vec![
                            konductor_common::dynamodb::s("dev"),
                            konductor_common::dynamodb::s("prod"),
                        ]),
                    )
                    .build()
            })
    }

    fn describe_rule(env: &'static str, version: i64, content: &'static str) -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema)
            .match_requests(move |req| {
                req.registry_name() == Some(format!("acme-{env}-orders-schema-registry").as_str())
                    && req.schema_name() == Some(SCHEMA)
            })
            .then_output(move || {
                DescribeSchemaOutput::builder()
                    .schema_name(SCHEMA)
                    .schema_version(version.to_string())
                    .content(content)
                    .build()
            })
    }

    fn describe_missing_rule() -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema).then_error(|| {
            aws_sdk_schemas::operation::describe_schema::DescribeSchemaError::NotFoundException(
                aws_sdk_schemas::types::error::NotFoundException::builder()
                    .message("not found")
                    .build(),
            )
        })
    }

    fn clients(
        ddb_rules: &[&Rule],
        schemas_rules: &[&Rule],
    ) -> (aws_sdk_dynamodb::Client, aws_sdk_schemas::Client) {
        (
            mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
            mock_client!(aws_sdk_schemas, RuleMode::MatchAny, schemas_rules),
        )
    }

    fn oauth_team_context(group: &str) -> Value {
        json!({
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": "user-2",
                "issuer": "https://idp.example.com",
                "clientId": "client-1",
                "globalRoles": "[]",
                "teamGroups": format!("[\"{group}\"]"),
            }
        })
    }

    fn publisher_binding_rule(group: &'static str, domain: &'static str) -> Rule {
        let binding = RoleBinding::new(
            "b-pub",
            BindingPrincipal::CognitoGroup {
                issuer: "https://idp.example.com".to_string(),
                group: group.to_string(),
            },
            ResourceRole::Publisher,
            domain,
            Utc::now(),
            "test",
        )
        .unwrap();
        let item = binding_to_item(&binding);
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(move |req| req.table_name() == Some("bindings"))
            .then_output(move || {
                GetItemOutput::builder()
                    .set_item(Some(item.clone()))
                    .build()
            })
    }

    #[tokio::test]
    async fn publisher_reads_own_schema_detail_without_consumer_context() {
        set_env();
        let binding = publisher_binding_rule("team-orders", "orders");
        let domain = domain_rule();
        let dev = describe_rule("dev", 5, "dev-content");
        let prod = describe_rule("prod", 3, "prod-content");
        // No entitlements rule: publisher access must not consult coverage.
        let (ddb, schemas) = clients(&[&binding, &domain], &[&dev, &prod]);

        let resp = handle(
            &ddb,
            &schemas,
            request(
                oauth_team_context("team-orders"),
                json!({ "domain": "orders" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 200);
        assert_eq!(body(&resp)["schemaName"], SCHEMA);
    }

    #[tokio::test]
    async fn non_publisher_without_consumer_context_is_forbidden() {
        set_env();
        let no_binding = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| GetItemOutput::builder().build());
        let (ddb, schemas) = clients(&[&no_binding], &[]);

        let resp = handle(
            &ddb,
            &schemas,
            request(
                oauth_team_context("team-payments"),
                json!({ "domain": "orders" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 403);
    }

    #[tokio::test]
    async fn covered_schema_returns_full_detail_with_skew_visible() {
        set_env();
        let binding = subscriber_binding_rule("111122223333", "checkout");
        let entitlements = entitlements_rule("checkout", vec!["Order*"]);
        let domain = domain_rule();
        let dev = describe_rule("dev", 5, "dev-content");
        let prod = describe_rule("prod", 3, "prod-content");
        let (ddb, schemas) = clients(&[&binding, &entitlements, &domain], &[&dev, &prod]);

        let resp = handle(
            &ddb,
            &schemas,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout", "domain": "orders" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 200);
        let body = body(&resp);
        assert_eq!(body["latestVersions"]["dev"], 5);
        assert_eq!(body["latestVersions"]["prod"], 3);
        assert_eq!(
            body["content"], "dev-content",
            "highest latest version wins"
        );
    }

    #[tokio::test]
    async fn uncovered_schema_is_forbidden() {
        set_env();
        let binding = subscriber_binding_rule("111122223333", "checkout");
        let entitlements = entitlements_rule("checkout", vec!["InvoiceCreated"]);
        let (ddb, schemas) = clients(&[&binding, &entitlements], &[]);

        let resp = handle(
            &ddb,
            &schemas,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout", "domain": "orders" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 403);
        assert_eq!(body(&resp)["code"], "AccessDeniedException");
    }

    #[tokio::test]
    async fn schema_absent_everywhere_is_not_found() {
        set_env();
        let binding = subscriber_binding_rule("111122223333", "checkout");
        let entitlements = entitlements_rule("checkout", vec!["*"]);
        let domain = domain_rule();
        let missing = describe_missing_rule();
        let (ddb, schemas) = clients(&[&binding, &entitlements, &domain], &[&missing]);

        let resp = handle(
            &ddb,
            &schemas,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout", "domain": "orders" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 404);
    }

    #[tokio::test]
    async fn platform_roles_inspect_without_consumer_domain() {
        set_env();
        let domain = domain_rule();
        let dev = describe_rule("dev", 5, "content");
        let prod = describe_rule("prod", 3, "content");
        let (ddb, schemas) = clients(&[&domain], &[&dev, &prod]);

        let resp = handle(
            &ddb,
            &schemas,
            request(
                oauth_platform_context("platform-auditor"),
                json!({ "domain": "orders" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 200);
    }

    #[tokio::test]
    async fn non_platform_without_consumer_domain_is_forbidden() {
        set_env();
        // No publisher binding either: the detail route denies.
        let no_binding = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| GetItemOutput::builder().build());
        let (ddb, schemas) = clients(&[&no_binding], &[]);
        let resp = handle(
            &ddb,
            &schemas,
            request(iam_context("111122223333"), json!({ "domain": "orders" })),
        )
        .await
        .unwrap();
        assert_eq!(resp["statusCode"], 403);
    }

    #[tokio::test]
    async fn no_subscriber_binding_is_forbidden() {
        set_env();
        let no_binding = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| GetItemOutput::builder().build());
        let (ddb, schemas) = clients(&[&no_binding], &[]);

        let resp = handle(
            &ddb,
            &schemas,
            request(
                iam_context("999900001111"),
                json!({ "consumerDomain": "checkout", "domain": "orders" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 403);
    }
}
