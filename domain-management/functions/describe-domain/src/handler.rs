use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use konductor_common::api;
use konductor_common::domain_scope::is_full_visibility;
use konductor_common::dynamodb::{
    attr_to_resource_arns_map, attr_to_string_map, get_s, get_string_list, get_string_set, s,
};
use konductor_common::error::ControlPlaneError;
use konductor_common::grant::ResourceRole;
use konductor_common::grant_engine;
use konductor_common::grant_store::BindingStore;
use konductor_common::namespace::NamespaceResolver;
use konductor_common::principal_context::log_principal;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let bindings_table = env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    let org = env::var("ORGANIZATION").context("ORGANIZATION not set")?;

    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "describe domain: deny (no trusted principal)");
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ));
        }
    };

    Ok(fetch_domain(
        ddb,
        &table,
        &bindings_table,
        &event.payload,
        &principal,
        &org,
    )
    .await)
}

/// Team callers see their own domains only; a domain outside their
/// ownership scope answers exactly like one that does not exist.
async fn fetch_domain(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    bindings_table: &str,
    event: &Value,
    principal: &konductor_common::principal::TrustedPrincipal,
    org: &str,
) -> Value {
    let domain_name = match api::path_param(event, "domainName") {
        Some(name) => name,
        None => {
            return api::from_control_plane_error(&api::validation_error(
                "Missing domainName path parameter",
            ));
        }
    };

    let item = match ddb
        .get_item()
        .table_name(table)
        .key("domainName", s(&domain_name))
        .send()
        .await
    {
        Ok(output) => match output.item {
            Some(item) => item,
            None => {
                return api::from_control_plane_error(&ControlPlaneError::DomainNotFound {
                    domain_name,
                });
            }
        },
        Err(e) => {
            tracing::error!(error = %e, "domain lookup failed");
            return api::from_control_plane_error(&ControlPlaneError::DynamoDb(e.to_string()));
        }
    };

    if !is_full_visibility(principal) {
        let store = BindingStore::new(ddb.clone(), bindings_table);
        let owns =
            grant_engine::has_role_binding(&store, principal, ResourceRole::Owner, &domain_name)
                .await;
        if !owns {
            tracing::info!(domain = %domain_name, "describe denied: no owner binding in caller teams");
            return api::from_control_plane_error(&ControlPlaneError::DomainNotFound {
                domain_name,
            });
        }
    }

    render_domain(&item, org)
}

fn render_domain(item: &HashMap<String, AttributeValue>, org: &str) -> Value {
    let resource_arns = attr_to_resource_arns_map(item, "resourceArns").map(|map| json!(map));
    let environments = get_string_list(item, "environments").unwrap_or_default();
    let secondary_owner_emails = get_string_set(item, "secondaryOwnerEmails");
    let domain_name = get_s(item, "domainName").unwrap_or_default();

    let resolver = NamespaceResolver::default_pattern();
    let namespaces: serde_json::Map<String, Value> = environments
        .iter()
        .map(|env_name| {
            (
                env_name.clone(),
                json!(resolver.resolve(env_name, org, &domain_name)),
            )
        })
        .collect();

    let mut body = json!({
        "domainName": domain_name,
        "description": get_s(item, "description"),
        "ownerAccountId": get_s(item, "ownerAccountId"),
        "ownerEmail": get_s(item, "ownerEmail"),
        "secondaryOwnerEmails": secondary_owner_emails,
        "status": get_s(item, "status"),
        "environments": environments,
        "namespaces": namespaces,
        "tags": attr_to_string_map(item, "tags"),
        "resourceArns": resource_arns,
        "namespaceParameter": get_s(item, "namespaceParameter"),
        "createdAt": get_s(item, "createdAt"),
        "updatedAt": get_s(item, "updatedAt"),
    });

    if let Some(v) = get_s(item, "decidedBy") {
        body["decidedBy"] = json!(v);
    }
    if let Some(v) = get_s(item, "justification") {
        body["justification"] = json!(v);
    }
    if let Some(v) = get_s(item, "approvedAt") {
        body["approvedAt"] = json!(v);
    }
    if let Some(v) = get_s(item, "rejectedAt") {
        body["rejectedAt"] = json!(v);
    }
    if let Some(v) = get_s(item, "errorDetail") {
        body["errorDetail"] = json!(v);
    }

    api::success_response(200, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client};
    use konductor_common::principal::TrustedPrincipal;
    use std::collections::HashMap;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

    fn iam_principal() -> TrustedPrincipal {
        TrustedPrincipal::iam(
            "123456789012",
            Some("arn:aws:iam::123456789012:role/deploy".to_string()),
        )
    }

    fn platform_principal() -> TrustedPrincipal {
        TrustedPrincipal::oauth(
            "admin-subject",
            ISSUER,
            "client-123",
            vec![konductor_common::principal::GlobalRole::PlatformAdmin],
            vec![],
        )
    }

    fn team_principal(teams: &[&str]) -> TrustedPrincipal {
        TrustedPrincipal::oauth(
            "member-subject",
            ISSUER,
            "client-123",
            vec![],
            teams.iter().map(|t| t.to_string()).collect(),
        )
    }

    /// The grant engine resolves ownership through strongly consistent
    /// GetItem point reads on the bindings table.
    fn bindings_rule(item: Option<HashMap<String, AttributeValue>>) -> aws_smithy_mocks::Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.table_name() == Some("bindings"))
            .then_output(move || GetItemOutput::builder().set_item(item.clone()).build())
    }

    fn owner_binding(domain: &str, group: &str) -> HashMap<String, AttributeValue> {
        HashMap::from([
            (
                "PK".to_string(),
                s(&format!("PRINCIPAL#cognito-group#{ISSUER}#{group}")),
            ),
            ("SK".to_string(), s(&format!("ROLE#owner#DOMAIN#{domain}"))),
            ("bindingId".to_string(), s("rb-111")),
            ("principalType".to_string(), s("cognito-group")),
            ("issuer".to_string(), s(ISSUER)),
            ("group".to_string(), s(group)),
            ("role".to_string(), s("owner")),
            ("domain".to_string(), s(domain)),
            ("createdAt".to_string(), s("2026-08-11T00:00:00Z")),
            ("createdBy".to_string(), s("admin-subject")),
        ])
    }

    fn domain_item() -> HashMap<String, AttributeValue> {
        HashMap::from([
            ("domainName".to_string(), s("billing")),
            ("description".to_string(), s("Billing domain")),
            ("ownerAccountId".to_string(), s("111122223333")),
            ("ownerEmail".to_string(), s("owner@example.com")),
            ("status".to_string(), s("ACTIVE")),
            ("createdAt".to_string(), s("2024-01-01T00:00:00Z")),
            ("updatedAt".to_string(), s("2024-01-01T00:00:00Z")),
        ])
    }

    #[tokio::test]
    async fn path_param_present_returns_domain() {
        let item = domain_item();
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let event = json!({"pathParameters": {"domainName": "billing"}});
        let resp = fetch_domain(
            &ddb,
            "domains",
            "bindings",
            &event,
            &iam_principal(),
            "acme",
        )
        .await;

        assert_eq!(resp["statusCode"], 200);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["domainName"], "billing");
        assert_eq!(body["status"], "ACTIVE");
        assert_eq!(body["ownerEmail"], "owner@example.com");
        assert_eq!(get_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn namespaces_resolved_for_each_environment() {
        let mut item = domain_item();
        item.insert(
            "environments".to_string(),
            AttributeValue::L(vec![s("dev"), s("staging"), s("prod")]),
        );
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let event = json!({"pathParameters": {"domainName": "billing"}});
        let resp = fetch_domain(
            &ddb,
            "domains",
            "bindings",
            &event,
            &iam_principal(),
            "acme",
        )
        .await;

        assert_eq!(resp["statusCode"], 200);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(
            body["namespaces"],
            json!({
                "dev": "acme-dev-billing",
                "staging": "acme-staging-billing",
                "prod": "acme-prod-billing",
            })
        );
    }

    #[tokio::test]
    async fn path_param_absent_returns_validation_error() {
        let ddb = mock_client!(aws_sdk_dynamodb, &[]);
        let event = json!({});

        let resp = fetch_domain(
            &ddb,
            "domains",
            "bindings",
            &event,
            &iam_principal(),
            "acme",
        )
        .await;

        assert_eq!(resp["statusCode"], 400);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "ValidationException");
        assert!(body["message"].as_str().unwrap().contains("domainName"));
    }

    #[tokio::test]
    async fn domain_not_found_returns_404_via_from_control_plane_error() {
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| GetItemOutput::builder().build());
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let event = json!({"pathParameters": {"domainName": "missing"}});
        let resp = fetch_domain(
            &ddb,
            "domains",
            "bindings",
            &event,
            &iam_principal(),
            "acme",
        )
        .await;

        assert_eq!(resp["statusCode"], 404);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "ResourceNotFoundException");
        assert_eq!(body["message"], "domain not found: missing");
        assert_eq!(get_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn dynamodb_error_maps_to_control_plane_error_dynamo_db() {
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_error(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemError::InternalServerError(
                aws_sdk_dynamodb::types::error::InternalServerError::builder().build(),
            )
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let event = json!({"pathParameters": {"domainName": "billing"}});
        let resp = fetch_domain(
            &ddb,
            "domains",
            "bindings",
            &event,
            &iam_principal(),
            "acme",
        )
        .await;

        assert_eq!(resp["statusCode"], 500);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "InternalServerException");
        assert_eq!(body["message"], "An internal error occurred");
        assert_eq!(get_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn optional_fields_included_when_present() {
        let mut item = domain_item();
        item.insert("decidedBy".to_string(), s("admin@example.com"));
        item.insert("justification".to_string(), s("approved by team"));

        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let event = json!({"pathParameters": {"domainName": "billing"}});
        let resp = fetch_domain(
            &ddb,
            "domains",
            "bindings",
            &event,
            &iam_principal(),
            "acme",
        )
        .await;

        assert_eq!(resp["statusCode"], 200);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["decidedBy"], "admin@example.com");
        assert_eq!(body["justification"], "approved by team");
    }

    #[tokio::test]
    async fn team_caller_with_owner_binding_gets_full_record() {
        let item = domain_item();
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        });
        let bindings = bindings_rule(Some(owner_binding("billing", "team-payments")));
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule, &bindings]);

        let event = json!({"pathParameters": {"domainName": "billing"}});
        let resp = fetch_domain(
            &ddb,
            "domains",
            "bindings",
            &event,
            &team_principal(&["team-payments"]),
            "acme",
        )
        .await;

        assert_eq!(resp["statusCode"], 200);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["domainName"], "billing");
        assert!(
            body.get("taskToken").is_none(),
            "workflow task token must never appear in responses"
        );
    }

    #[tokio::test]
    async fn team_caller_without_owner_binding_gets_indistinguishable_404() {
        let item = domain_item();
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        });
        let bindings = bindings_rule(None);
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule, &bindings]);

        let event = json!({"pathParameters": {"domainName": "billing"}});
        let denied = fetch_domain(
            &ddb,
            "domains",
            "bindings",
            &event,
            &team_principal(&["team-billing"]),
            "acme",
        )
        .await;

        let missing_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| GetItemOutput::builder().build());
        let ddb2 = mock_client!(aws_sdk_dynamodb, [&missing_rule]);
        let missing_event = json!({"pathParameters": {"domainName": "missing"}});
        let missing = fetch_domain(
            &ddb2,
            "domains",
            "bindings",
            &missing_event,
            &team_principal(&["team-billing"]),
            "acme",
        )
        .await;

        let denied_body: Value = serde_json::from_str(denied["body"].as_str().unwrap()).unwrap();
        let missing_body: Value = serde_json::from_str(missing["body"].as_str().unwrap()).unwrap();
        assert_eq!(denied["statusCode"], 404);
        assert_eq!(missing["statusCode"], 404);
        assert_eq!(
            denied_body["code"], missing_body["code"],
            "denied and missing must carry the same error shape"
        );
        assert_eq!(denied_body["code"], "ResourceNotFoundException");
    }

    #[tokio::test]
    async fn platform_caller_reads_any_domain_without_binding_check() {
        let item = domain_item();
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let event = json!({"pathParameters": {"domainName": "billing"}});
        let resp = fetch_domain(
            &ddb,
            "domains",
            "bindings",
            &event,
            &platform_principal(),
            "acme",
        )
        .await;

        assert_eq!(resp["statusCode"], 200);
        assert_eq!(get_rule.num_calls(), 1);
    }
}
