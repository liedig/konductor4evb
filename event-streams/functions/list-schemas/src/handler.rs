use std::env;

use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use nexus_common::api;
use nexus_common::error::{ControlPlaneError, ValidationExceptionReason};
use nexus_common::grant::ResourceRole;
use nexus_common::grant_engine::has_role_binding;
use nexus_common::grant_store::BindingStore;
use nexus_common::models::ApiError;
use nexus_common::principal::{GlobalRole, TrustedPrincipal};
use nexus_common::principal_context::log_principal;

use crate::catalog::{self, CatalogQuery, Cursor, Scope};
use crate::sources::CallerSources;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    schemas: &aws_sdk_schemas::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let organization = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let bindings_table = env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    let entitlements_table =
        env::var("ENTITLEMENTS_TABLE").context("ENTITLEMENTS_TABLE not set")?;

    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "list schemas: deny (no trusted principal)");
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ));
        }
    };

    let consumer = api::query_param(&event.payload, "consumerDomain");
    let producer = api::query_param(&event.payload, "domain");
    let environment = api::query_param(&event.payload, "environment");
    let page_size = api::query_param(&event.payload, "maxResults")
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(catalog::MAX_PAGE_SIZE)
        .min(catalog::MAX_PAGE_SIZE);

    let scope = match resolve_scope(
        ddb,
        &bindings_table,
        &principal,
        &consumer,
        producer.as_deref(),
    )
    .await
    {
        Ok(scope) => scope,
        Err(response) => return Ok(response),
    };

    let filter_hash = catalog::filter_hash(
        consumer.as_deref(),
        producer.as_deref(),
        environment.as_deref(),
    );
    let watermark = match parse_cursor(&event.payload, &filter_hash) {
        Ok(w) => w,
        Err(response) => return Ok(response),
    };

    let sources = match resolve_sources(ddb, &entitlements_table, &scope, producer.as_deref()).await
    {
        Ok(sources) => sources,
        Err(response) => return Ok(response),
    };

    let query = CatalogQuery {
        sources,
        environment,
        limit: page_size + 1,
        watermark,
    };
    Ok(
        match catalog::list_catalog(ddb, schemas, &organization, &scope, &query).await {
            Ok(page) => render_page(&page, filter_hash),
            Err(e) => {
                tracing::error!(error = %e, "catalog expansion failed");
                api::error_response(
                    500,
                    ApiError {
                        code: "InternalError".to_string(),
                        message: "schema discovery failed".to_string(),
                        request_id: None,
                        reason: None,
                        field_list: None,
                    },
                )
            }
        },
    )
}

async fn resolve_sources(
    ddb: &aws_sdk_dynamodb::Client,
    entitlements_table: &str,
    scope: &Scope,
    producer: Option<&str>,
) -> Result<std::collections::BTreeMap<String, Vec<nexus_common::entitlement::ScopePattern>>, Value>
{
    let result = match scope {
        Scope::Consumer(consumer) => {
            CallerSources::for_consumer(ddb, entitlements_table, consumer, producer).await
        }
        Scope::Producer(domain) => Ok(CallerSources::for_producer(domain)),
        Scope::Platform => CallerSources::platform_wide(ddb, producer).await,
    };
    result.map_err(|e| {
        tracing::error!(error = %e, "catalog source resolution failed");
        api::from_control_plane_error(&ControlPlaneError::DynamoDb(e.to_string()))
    })
}

fn render_page(page: &catalog::CatalogPage, filter_hash: String) -> Value {
    let items: Vec<Value> = page.entries.iter().map(catalog_item).collect();
    let next_token = page.watermark.as_ref().map(|(producer, schema)| {
        Cursor {
            producer: producer.clone(),
            schema: schema.clone(),
            filter_hash,
        }
        .encode()
    });
    api::success_response(200, &json!({ "items": items, "nextToken": next_token }))
}

/// Decodes the continuation token and binds it to this request's filter
/// set. A mismatched or malformed token is a 400 — the cursor must never
/// silently restart or jump sequences.
fn parse_cursor(payload: &Value, filter_hash: &str) -> Result<Option<(String, String)>, Value> {
    let Some(token) = api::query_param(payload, "nextToken") else {
        return Ok(None);
    };
    match Cursor::decode(&token) {
        Some(cursor) if cursor.filter_hash == filter_hash => {
            Ok(Some((cursor.producer, cursor.schema)))
        }
        Some(_) => Err(validation(
            "continuation token does not match this request's filter set",
        )),
        None => Err(validation("malformed continuation token")),
    }
}

/// Consumer path: a `subscriber` binding on the queried consumer domain (or
/// a platform role inspecting a consumer's view). Producer path: a
/// `publisher` binding on the queried domain yields that domain's catalog.
/// Neither parameter: the platform-wide catalog, platform roles only.
async fn resolve_scope(
    ddb: &aws_sdk_dynamodb::Client,
    bindings_table: &str,
    principal: &TrustedPrincipal,
    consumer: &Option<String>,
    producer: Option<&str>,
) -> Result<Scope, Value> {
    match consumer {
        Some(domain) => {
            let allowed = is_platform(principal)
                || has_role_binding(
                    &BindingStore::new(ddb.clone(), bindings_table),
                    principal,
                    ResourceRole::Subscriber,
                    domain,
                )
                .await;
            if allowed {
                Ok(Scope::Consumer(domain.clone()))
            } else {
                tracing::warn!(domain, "list schemas: deny (no subscriber binding)");
                Err(api::from_control_plane_error(
                    &ControlPlaneError::AccessDenied,
                ))
            }
        }
        None if is_platform(principal) => Ok(Scope::Platform),
        None => match producer {
            Some(domain)
                if has_role_binding(
                    &BindingStore::new(ddb.clone(), bindings_table),
                    principal,
                    ResourceRole::Publisher,
                    domain,
                )
                .await =>
            {
                Ok(Scope::Producer(domain.to_string()))
            }
            _ => {
                tracing::warn!(
                    "list schemas: deny (catalog requires consumer, publisher, or platform scope)"
                );
                Err(api::from_control_plane_error(
                    &ControlPlaneError::AccessDenied,
                ))
            }
        },
    }
}

fn is_platform(principal: &TrustedPrincipal) -> bool {
    principal.is_platform_admin()
        || principal
            .global_roles()
            .contains(&GlobalRole::PlatformAuditor)
}

fn catalog_item(entry: &catalog::CatalogEntry) -> Value {
    let (source, detail_type) = entry
        .schema_name
        .split_once('@')
        .unwrap_or((entry.schema_name.as_str(), ""));
    json!({
        "schemaName": entry.schema_name,
        "producerDomain": entry.producer_domain,
        "source": source,
        "detailType": detail_type,
        "latestVersions": entry.latest_versions,
    })
}

fn validation(message: &str) -> Value {
    api::from_control_plane_error(&ControlPlaneError::ValidationException {
        message: message.to_string(),
        reason: ValidationExceptionReason::FieldValidationFailed,
        field_list: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::operation::query::QueryOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_sdk_schemas::operation::describe_schema::DescribeSchemaOutput;
    use aws_sdk_schemas::operation::list_schemas::ListSchemasOutput;
    use aws_sdk_schemas::types::SchemaSummary;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use lambda_runtime::Context as LambdaContext;

    use nexus_common::entitlement::{Entitlement, ScopePattern};
    use nexus_common::entitlement_store::entitlement_to_item;
    use nexus_common::grant::{BindingPrincipal, RoleBinding};
    use nexus_common::grant_store::binding_to_item;

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

    fn domain_rule(domain: &'static str, envs: Vec<&'static str>) -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(move |req| {
                req.key()
                    .and_then(|k| k.get("domainName"))
                    .and_then(|v| v.as_s().ok())
                    .map(|v| v == domain)
                    .unwrap_or(false)
            })
            .then_output(move || {
                GetItemOutput::builder()
                    .item("domainName", nexus_common::dynamodb::s(domain))
                    .item("status", nexus_common::dynamodb::s("ACTIVE"))
                    .item(
                        "environments",
                        AttributeValue::L(
                            envs.iter().map(|e| nexus_common::dynamodb::s(e)).collect(),
                        ),
                    )
                    .build()
            })
    }

    fn registry_rule(registry: &'static str, names: Vec<&'static str>) -> Rule {
        mock!(aws_sdk_schemas::Client::list_schemas)
            .match_requests(move |req| req.registry_name() == Some(registry))
            .then_output(move || {
                let summaries: Vec<SchemaSummary> = names
                    .iter()
                    .map(|n| SchemaSummary::builder().schema_name(*n).build())
                    .collect();
                ListSchemasOutput::builder()
                    .set_schemas(Some(summaries))
                    .build()
            })
    }

    fn describe_rule(registry: &'static str, name: &'static str, version: i64) -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema)
            .match_requests(move |req| {
                req.registry_name() == Some(registry) && req.schema_name() == Some(name)
            })
            .then_output(move || {
                DescribeSchemaOutput::builder()
                    .schema_name(name)
                    .schema_version(version.to_string())
                    .build()
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

    #[tokio::test]
    async fn catalog_expands_entitlement_union_and_reports_latest_versions() {
        set_env();
        let binding = subscriber_binding_rule("111122223333", "checkout");
        let entitlements = entitlements_rule("checkout", vec!["Order*", "InvoiceCreated"]);
        let domain = domain_rule("orders", vec!["prod"]);
        let registry = registry_rule(
            "acme-prod-orders-schema-registry",
            vec![
                "acme-orders@OrderNotification",
                "acme-orders@OrderRefunded",
                "acme-orders@InvoiceCreated",
                "acme-orders@CustomerMerged",
            ],
        );
        let d1 = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@OrderNotification",
            3,
        );
        let d2 = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@OrderRefunded",
            1,
        );
        let d3 = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@InvoiceCreated",
            2,
        );
        let (ddb, schemas) = clients(
            &[&binding, &entitlements, &domain],
            &[&registry, &d1, &d2, &d3],
        );

        let resp = handle(
            &ddb,
            &schemas,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 200);
        let items = body(&resp)["items"].clone();
        let names: Vec<&str> = items
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["schemaName"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "acme-orders@InvoiceCreated",
                "acme-orders@OrderNotification",
                "acme-orders@OrderRefunded"
            ],
            "union of Order* and InvoiceCreated, deduplicated and sorted; CustomerMerged excluded"
        );
        assert_eq!(items[0]["latestVersions"]["prod"], 2);
        assert_eq!(items[1]["latestVersions"]["prod"], 3);
    }

    #[tokio::test]
    async fn domain_wide_grant_covers_schemas_created_after_the_grant() {
        set_env();
        let binding = subscriber_binding_rule("111122223333", "checkout");
        let entitlements = entitlements_rule("checkout", vec!["*"]);
        let domain = domain_rule("orders", vec!["prod"]);
        let registry = registry_rule(
            "acme-prod-orders-schema-registry",
            vec!["acme-orders@BrandNew"],
        );
        let describe = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@BrandNew",
            7,
        );
        let (ddb, schemas) = clients(&[&binding, &entitlements, &domain], &[&registry, &describe]);

        let resp = handle(
            &ddb,
            &schemas,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout" }),
            ),
        )
        .await
        .unwrap();

        let items = body(&resp)["items"].clone();
        assert_eq!(items.as_array().unwrap().len(), 1);
        assert_eq!(items[0]["latestVersions"]["prod"], 7);
    }

    #[tokio::test]
    async fn environment_skew_is_reported_per_environment() {
        set_env();
        let binding = subscriber_binding_rule("111122223333", "checkout");
        let entitlements = entitlements_rule("checkout", vec!["OrderNotification"]);
        let domain = domain_rule("orders", vec!["dev", "prod"]);
        let dev = registry_rule(
            "acme-dev-orders-schema-registry",
            vec!["acme-orders@OrderNotification"],
        );
        let prod = registry_rule(
            "acme-prod-orders-schema-registry",
            vec!["acme-orders@OrderNotification"],
        );
        let d_dev = describe_rule(
            "acme-dev-orders-schema-registry",
            "acme-orders@OrderNotification",
            5,
        );
        let d_prod = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@OrderNotification",
            3,
        );
        let (ddb, schemas) = clients(
            &[&binding, &entitlements, &domain],
            &[&dev, &prod, &d_dev, &d_prod],
        );

        let resp = handle(
            &ddb,
            &schemas,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout" }),
            ),
        )
        .await
        .unwrap();

        let items = body(&resp)["items"].clone();
        assert_eq!(items.as_array().unwrap().len(), 1);
        assert_eq!(items[0]["latestVersions"]["dev"], 5);
        assert_eq!(items[0]["latestVersions"]["prod"], 3);
    }

    #[tokio::test]
    async fn zero_entitlements_yield_an_empty_catalog() {
        set_env();
        let binding = subscriber_binding_rule("111122223333", "checkout");
        let entitlements = entitlements_rule("checkout", vec![]);
        let (ddb, schemas) = clients(&[&binding, &entitlements], &[]);

        let resp = handle(
            &ddb,
            &schemas,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 200);
        assert_eq!(body(&resp)["items"], json!([]));
    }

    #[tokio::test]
    async fn pages_are_stably_ordered_without_duplicates() {
        set_env();
        let binding = subscriber_binding_rule("111122223333", "checkout");
        let entitlements = entitlements_rule("checkout", vec!["*"]);
        let domain = domain_rule("orders", vec!["prod"]);
        let registry = registry_rule(
            "acme-prod-orders-schema-registry",
            vec!["acme-orders@Aardvark", "acme-orders@Zebra"],
        );
        let d1 = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@Aardvark",
            1,
        );
        let d2 = describe_rule("acme-prod-orders-schema-registry", "acme-orders@Zebra", 1);
        let (ddb, schemas) = clients(&[&binding, &entitlements, &domain], &[&registry, &d1, &d2]);

        let page1 = handle(
            &ddb,
            &schemas,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout", "maxResults": "1" }),
            ),
        )
        .await
        .unwrap();
        let page1_body = body(&page1);
        let token = page1_body["nextToken"]
            .as_str()
            .expect("page 2 must exist")
            .to_string();
        assert_eq!(page1_body["items"][0]["schemaName"], "acme-orders@Aardvark");

        let page2 = handle(
            &ddb,
            &schemas,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout", "maxResults": "1", "nextToken": token }),
            ),
        )
        .await
        .unwrap();
        let page2_body = body(&page2);
        assert_eq!(page2_body["items"][0]["schemaName"], "acme-orders@Zebra");
        assert!(page2_body["nextToken"].is_null(), "catalog exhausted");
    }

    #[tokio::test]
    async fn revoked_coverage_never_appears_in_a_later_page() {
        set_env();
        // Page 1: the grant covers both schemas; the first page emits
        // Aardvark and a continuation token.
        let binding1 = subscriber_binding_rule("111122223333", "checkout");
        let granted = entitlements_rule("checkout", vec!["*"]);
        let domain1 = domain_rule("orders", vec!["prod"]);
        let registry1 = registry_rule(
            "acme-prod-orders-schema-registry",
            vec!["acme-orders@Aardvark", "acme-orders@Zebra"],
        );
        let d1 = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@Aardvark",
            1,
        );
        let (ddb1, schemas1) = clients(&[&binding1, &granted, &domain1], &[&registry1, &d1]);

        let page1 = handle(
            &ddb1,
            &schemas1,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout", "maxResults": "1" }),
            ),
        )
        .await
        .unwrap();
        let token = body(&page1)["nextToken"]
            .as_str()
            .expect("a second page is pending")
            .to_string();

        // The grant is revoked, then page 2 re-reads entitlements: nothing is
        // covered anymore, so the rest of the sequence is empty — the cursor
        // resumes position, never authorization state.
        let binding2 = subscriber_binding_rule("111122223333", "checkout");
        let revoked = entitlements_rule("checkout", vec![]);
        let domain2 = domain_rule("orders", vec!["prod"]);
        let (ddb2, schemas2) = clients(&[&binding2, &revoked, &domain2], &[]);

        let page2 = handle(
            &ddb2,
            &schemas2,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout", "maxResults": "1", "nextToken": token }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(body(&page2)["items"], json!([]));
        assert!(body(&page2)["nextToken"].is_null());
    }

    #[tokio::test]
    async fn mid_sequence_grant_behind_the_watermark_does_not_appear() {
        set_env();
        // Page 1: Mammoth and Zebra are covered; page size 1 emits Mammoth.
        let binding1 = subscriber_binding_rule("111122223333", "checkout");
        let granted = entitlements_rule("checkout", vec!["Mammoth", "Zebra"]);
        let domain1 = domain_rule("orders", vec!["prod"]);
        let registry1 = registry_rule(
            "acme-prod-orders-schema-registry",
            vec![
                "acme-orders@Aardvark",
                "acme-orders@Mammoth",
                "acme-orders@Zebra",
            ],
        );
        let d1 = describe_rule("acme-prod-orders-schema-registry", "acme-orders@Mammoth", 1);
        let (ddb1, schemas1) = clients(&[&binding1, &granted, &domain1], &[&registry1, &d1]);

        let page1 = handle(
            &ddb1,
            &schemas1,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout", "maxResults": "1" }),
            ),
        )
        .await
        .unwrap();
        let token = body(&page1)["nextToken"].as_str().unwrap().to_string();
        assert_eq!(
            body(&page1)["items"][0]["schemaName"],
            "acme-orders@Mammoth"
        );

        // Before page 2, a grant covering Aardvark arrives. Aardvark sorts
        // before the watermark (Mammoth), so it must not appear in the
        // remaining sequence — only Zebra does.
        let binding2 = subscriber_binding_rule("111122223333", "checkout");
        let widened = entitlements_rule("checkout", vec!["*"]);
        let domain2 = domain_rule("orders", vec!["prod"]);
        let registry2 = registry_rule(
            "acme-prod-orders-schema-registry",
            vec![
                "acme-orders@Aardvark",
                "acme-orders@Mammoth",
                "acme-orders@Zebra",
            ],
        );
        let d2 = describe_rule("acme-prod-orders-schema-registry", "acme-orders@Zebra", 1);
        let d3 = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@Aardvark",
            1,
        );
        let d4 = describe_rule("acme-prod-orders-schema-registry", "acme-orders@Mammoth", 1);
        let (ddb2, schemas2) = clients(
            &[&binding2, &widened, &domain2],
            &[&registry2, &d2, &d3, &d4],
        );

        let page2 = handle(
            &ddb2,
            &schemas2,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout", "maxResults": "1", "nextToken": token }),
            ),
        )
        .await
        .unwrap();
        let page2_body = body(&page2);
        let names: Vec<&str> = page2_body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["schemaName"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["acme-orders@Zebra"]);

        // A fresh listing does see the newly covered schema.
        let fresh = handle(
            &ddb2,
            &schemas2,
            request(
                iam_context("111122223333"),
                json!({ "consumerDomain": "checkout" }),
            ),
        )
        .await
        .unwrap();
        assert_eq!(body(&fresh)["items"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn cursor_with_a_changed_filter_set_is_a_validation_error() {
        set_env();
        let binding = subscriber_binding_rule("111122223333", "checkout");
        let (ddb, schemas) = clients(&[&binding], &[]);

        let foreign_cursor = Cursor {
            producer: "orders".to_string(),
            schema: "acme-orders@OrderNotification".to_string(),
            filter_hash: crate::catalog::filter_hash(Some("checkout"), Some("orders"), None),
        };
        let resp = handle(
            &ddb,
            &schemas,
            request(
                iam_context("111122223333"),
                json!({
                    "consumerDomain": "checkout",
                    "nextToken": foreign_cursor.encode(),
                }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 400);
        assert_eq!(body(&resp)["code"], "ValidationException");
    }

    #[tokio::test]
    async fn platform_roles_get_the_unfiltered_catalog_without_consumer_domain() {
        set_env();
        let domain = domain_rule("orders", vec!["prod"]);
        let scan = mock!(aws_sdk_dynamodb::Client::scan).then_output(|| {
            let item = HashMap::from([
                (
                    "domainName".to_string(),
                    nexus_common::dynamodb::s("orders"),
                ),
                ("status".to_string(), nexus_common::dynamodb::s("ACTIVE")),
                (
                    "environments".to_string(),
                    AttributeValue::L(vec![nexus_common::dynamodb::s("prod")]),
                ),
            ]);
            aws_sdk_dynamodb::operation::scan::ScanOutput::builder()
                .set_items(Some(vec![item]))
                .build()
        });
        let registry = registry_rule(
            "acme-prod-orders-schema-registry",
            vec!["acme-orders@OrderNotification", "acme-orders@InternalAudit"],
        );
        let d1 = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@OrderNotification",
            3,
        );
        let d2 = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@InternalAudit",
            1,
        );
        let (ddb, schemas) = clients(&[&scan, &domain], &[&registry, &d1, &d2]);

        for role in ["platform-admin", "platform-auditor"] {
            let resp = handle(
                &ddb,
                &schemas,
                request(oauth_platform_context(role), json!({})),
            )
            .await
            .unwrap();
            assert_eq!(resp["statusCode"], 200, "{role} reads the platform catalog");
            assert_eq!(body(&resp)["items"].as_array().unwrap().len(), 2);
        }
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

    #[tokio::test]
    async fn publisher_binding_yields_own_domain_catalog_without_entitlements() {
        set_env();
        let binding = publisher_binding_rule("team-orders", "orders");
        let domain = domain_rule("orders", vec!["prod"]);
        let registry = registry_rule(
            "acme-prod-orders-schema-registry",
            vec!["acme-orders@OrderNotification", "acme-orders@InternalAudit"],
        );
        let d1 = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@OrderNotification",
            3,
        );
        let d2 = describe_rule(
            "acme-prod-orders-schema-registry",
            "acme-orders@InternalAudit",
            1,
        );
        // No entitlements rule: the producer scope must never consult the
        // entitlement table.
        let (ddb, schemas) = clients(&[&binding, &domain], &[&registry, &d1, &d2]);

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
        let items = body(&resp)["items"].clone();
        let names: Vec<&str> = items
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["schemaName"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["acme-orders@InternalAudit", "acme-orders@OrderNotification"],
            "the publisher sees its full catalog, not an entitlement view"
        );
    }

    #[tokio::test]
    async fn publisher_binding_on_another_domain_is_forbidden() {
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
        assert_eq!(body(&resp)["code"], "AccessDeniedException");
    }

    #[tokio::test]
    async fn non_platform_without_consumer_domain_is_forbidden() {
        set_env();
        let (ddb, schemas) = clients(&[], &[]);
        let resp = handle(
            &ddb,
            &schemas,
            request(iam_context("111122223333"), json!({})),
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
                json!({ "consumerDomain": "checkout" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 403);
        assert_eq!(body(&resp)["code"], "AccessDeniedException");
    }
}
