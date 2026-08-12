use anyhow::{Context, Result};
use aws_sdk_dynamodb::types::AttributeValue;
use konductor_common::api;
use konductor_common::domain_scope::{
    fetch_domains_by_name, is_full_visibility, owned_domain_names,
};
use konductor_common::dynamodb::{
    deserialize_pagination_token, get_s, get_string_set, s, serialize_pagination_token,
};
use konductor_common::error::ControlPlaneError;
use konductor_common::principal::TrustedPrincipal;
use konductor_common::principal_context::log_principal;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;

struct QueryResult {
    items: Vec<HashMap<String, AttributeValue>>,
    last_key: Option<HashMap<String, AttributeValue>>,
}

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let org = env::var("ORGANIZATION").context("ORGANIZATION not set")?;

    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "list domains: deny (no trusted principal)");
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ));
        }
    };

    if api::query_param(&event.payload, "view").as_deref() == Some("directory") {
        return directory_listing(ddb, &table, &org, &event.payload).await;
    }

    if is_full_visibility(&principal) {
        return full_listing(ddb, &table, &org, &event.payload).await;
    }

    let bindings_table = env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    owned_listing(
        ddb,
        &table,
        &bindings_table,
        &org,
        &event.payload,
        &principal,
    )
    .await
}

/// The management list: every domain, with full records (IAM automation and
/// platform roles).
async fn full_listing(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    org: &str,
    payload: &Value,
) -> Result<Value, Error> {
    let owner_filter = api::query_param(payload, "ownerAccountId");
    let status_filter = api::query_param(payload, "status");
    let next_token = api::query_param(payload, "nextToken");
    let max_results = parse_max_results(payload);

    let start_key = next_token.as_deref().and_then(deserialize_pagination_token);

    let result = if let Some(owner) = &owner_filter {
        query_by_index(
            ddb,
            table,
            "by-owner",
            "ownerAccountId",
            owner,
            max_results,
            start_key,
        )
        .await
    } else if let Some(status) = &status_filter {
        query_by_index(
            ddb,
            table,
            "by-status",
            "status",
            status,
            max_results,
            start_key,
        )
        .await
    } else {
        scan_all(ddb, table, max_results, start_key).await
    };

    match result {
        Ok(qr) => {
            let items: Vec<Value> = qr
                .items
                .iter()
                .map(|item| full_item_json(org, item))
                .collect();
            let pagination_token = qr.last_key.as_ref().map(serialize_pagination_token);
            Ok(api::success_response(
                200,
                &json!({ "domains": items, "nextToken": pagination_token }),
            ))
        }
        Err(e) => {
            tracing::error!(error = %e, "domains query failed");
            Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                e.to_string(),
            )))
        }
    }
}

/// The team list: only domains the caller's team groups own. Owner bindings
/// resolve the names; filters then apply within that authorized set, and
/// pagination is in-memory over the (small) owned set.
async fn owned_listing(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    bindings_table: &str,
    org: &str,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let owned = match owned_domain_names(ddb, bindings_table, principal).await {
        Ok(names) => names,
        Err(e) => {
            tracing::error!(error = %e, "owner bindings query failed");
            return Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                e.to_string(),
            )));
        }
    };

    let owner_filter = api::query_param(payload, "ownerAccountId");
    let status_filter = api::query_param(payload, "status");
    let max_results = parse_max_results(payload) as usize;
    let after = api::query_param(payload, "nextToken")
        .as_deref()
        .and_then(deserialize_pagination_token)
        .and_then(|key| get_s(&key, "domainName"));

    let mut items: Vec<Value> = Vec::new();
    let mut next_token_out: Option<String> = None;
    if !owned.is_empty() {
        let records = fetch_domains_by_name(ddb, table, &owned)
            .await
            .context("domain batch read failed")?;
        let mut filtered: Vec<&HashMap<String, AttributeValue>> = records
            .iter()
            .filter(|item| {
                status_filter
                    .as_ref()
                    .is_none_or(|f| get_s(item, "status").as_deref() == Some(f))
                    && owner_filter
                        .as_ref()
                        .is_none_or(|f| get_s(item, "ownerAccountId").as_deref() == Some(f))
            })
            .collect();
        filtered.sort_by_key(|item| get_s(item, "domainName").unwrap_or_default());

        let start = after
            .as_ref()
            .map(|name| {
                filtered
                    .iter()
                    .position(|item| get_s(item, "domainName").as_deref() > Some(name))
                    .unwrap_or(filtered.len())
            })
            .unwrap_or(0);

        let page: Vec<&HashMap<String, AttributeValue>> = filtered
            .iter()
            .skip(start)
            .take(max_results)
            .copied()
            .collect();
        let has_more = start + page.len() < filtered.len();
        for item in &page {
            items.push(full_item_json(org, item));
        }
        next_token_out = if has_more {
            items
                .last()
                .and_then(|v| v.get("domainName"))
                .and_then(|v| v.as_str())
                .map(|name| {
                    serialize_pagination_token(&HashMap::from([(
                        "domainName".to_string(),
                        s(name),
                    )]))
                })
        } else {
            None
        };
    }

    Ok(api::success_response(
        200,
        &json!({ "domains": items, "nextToken": next_token_out }),
    ))
}

/// The names-only directory: any authenticated caller, picker-grade fields
/// only — no owner identities, accounts, or resource identifiers.
async fn directory_listing(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    org: &str,
    payload: &Value,
) -> Result<Value, Error> {
    let max_results = parse_max_results(payload);
    let start_key = api::query_param(payload, "nextToken")
        .as_deref()
        .and_then(deserialize_pagination_token);

    let mut scan = ddb
        .scan()
        .table_name(table)
        .projection_expression("domainName, description, #s")
        .expression_attribute_names("#s", "status")
        .limit(max_results);
    if let Some(key) = start_key {
        for (k, v) in key {
            scan = scan.exclusive_start_key(k, v);
        }
    }

    match scan.send().await {
        Ok(output) => {
            let mut items: Vec<Value> = output
                .items()
                .iter()
                .map(|item| directory_item_json(org, item))
                .collect();
            items.sort_by_key(|v| {
                v.get("domainName")
                    .and_then(|d| d.as_str())
                    .map(str::to_string)
            });
            let pagination_token = output
                .last_evaluated_key()
                .filter(|k| !k.is_empty())
                .map(serialize_pagination_token);
            Ok(api::success_response(
                200,
                &json!({ "domains": items, "nextToken": pagination_token }),
            ))
        }
        Err(e) => {
            tracing::error!(error = %e, "directory scan failed");
            Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                e.to_string(),
            )))
        }
    }
}

fn full_item_json(org: &str, item: &HashMap<String, AttributeValue>) -> Value {
    let domain_name = get_s(item, "domainName");
    json!({
        "domainName": domain_name,
        "domainNamespace": domain_name.as_ref().map(|name| format!("{org}-{name}")),
        "description": get_s(item, "description"),
        "ownerAccountId": get_s(item, "ownerAccountId"),
        "ownerEmail": get_s(item, "ownerEmail"),
        "secondaryOwnerEmails": get_string_set(item, "secondaryOwnerEmails"),
        "status": get_s(item, "status"),
        "createdAt": get_s(item, "createdAt"),
    })
}

fn directory_item_json(org: &str, item: &HashMap<String, AttributeValue>) -> Value {
    let domain_name = get_s(item, "domainName");
    json!({
        "domainName": domain_name,
        "domainNamespace": domain_name.as_ref().map(|name| format!("{org}-{name}")),
        "description": get_s(item, "description"),
        "status": get_s(item, "status"),
    })
}

fn parse_max_results(event: &Value) -> i32 {
    api::query_param(event, "maxResults")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
        .clamp(1, 100)
}

async fn query_by_index(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    index: &str,
    key_name: &str,
    key_value: &str,
    limit: i32,
    start_key: Option<HashMap<String, AttributeValue>>,
) -> Result<QueryResult> {
    let is_reserved = key_name == "status";
    let key_expr = if is_reserved {
        "#k = :val".to_string()
    } else {
        format!("{key_name} = :val")
    };

    let mut query = ddb
        .query()
        .table_name(table)
        .index_name(index)
        .key_condition_expression(&key_expr)
        .expression_attribute_values(":val", s(key_value))
        .limit(limit);

    if is_reserved {
        query = query.expression_attribute_names("#k", key_name);
    }
    if let Some(key) = start_key {
        for (k, v) in key {
            query = query.exclusive_start_key(k, v);
        }
    }

    let output = query.send().await.context("failed to query by-index GSI")?;
    Ok(QueryResult {
        items: output.items.unwrap_or_default(),
        last_key: output.last_evaluated_key,
    })
}

async fn scan_all(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    limit: i32,
    start_key: Option<HashMap<String, AttributeValue>>,
) -> Result<QueryResult> {
    let mut scan = ddb.scan().table_name(table).limit(limit);
    if let Some(key) = start_key {
        for (k, v) in key {
            scan = scan.exclusive_start_key(k, v);
        }
    }
    let output = scan.send().await.context("failed to scan domains table")?;
    Ok(QueryResult {
        items: output.items.unwrap_or_default(),
        last_key: output.last_evaluated_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::batch_get_item::BatchGetItemOutput;
    use aws_sdk_dynamodb::operation::query::QueryOutput;
    use aws_sdk_dynamodb::operation::scan::ScanOutput;
    use aws_smithy_mocks::{mock, mock_client, Rule};
    use lambda_runtime::Context as LambdaContext;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

    fn set_env() {
        env::set_var("DOMAINS_TABLE", "domains");
        env::set_var("ORGANIZATION", "acme");
        env::set_var("BINDINGS_TABLE", "bindings");
    }

    fn event_with(ctx: Value, query: Value) -> LambdaEvent<Value> {
        let mut payload = json!({ "requestContext": ctx });
        if !query.is_null() {
            payload["queryStringParameters"] = query;
        }
        LambdaEvent::new(payload, LambdaContext::default())
    }

    fn iam_ctx() -> Value {
        json!({
            "identity": {
                "sourceIp": "10.0.0.1",
                "accountId": "123456789012",
                "userArn": "arn:aws:iam::123456789012:role/deploy"
            }
        })
    }

    /// Mirrors the OAuth mapping template's authorizer context: roles are
    /// JSON-encoded lists, split into platform (global) and team groups.
    fn oauth_ctx(global_roles: &[&str], team_groups: &[&str]) -> Value {
        json!({
            "identity": { "sourceIp": "10.0.0.1" },
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": "user-subject-1",
                "issuer": ISSUER,
                "clientId": "client-123",
                "globalRoles": serde_json::to_string(global_roles).unwrap(),
                "teamGroups": serde_json::to_string(team_groups).unwrap()
            }
        })
    }

    fn domain_item(
        name: &str,
        status: &str,
        owner_account: &str,
    ) -> HashMap<String, AttributeValue> {
        HashMap::from([
            ("domainName".to_string(), s(name)),
            ("description".to_string(), s(&format!("{name} domain"))),
            ("ownerAccountId".to_string(), s(owner_account)),
            ("ownerEmail".to_string(), s(&format!("{name}@example.com"))),
            ("status".to_string(), s(status)),
            ("createdAt".to_string(), s("2026-08-01T00:00:00Z")),
            ("taskToken".to_string(), s("secret-token-must-never-leak")),
        ])
    }

    fn owner_binding_item(domain: &str, group: &str) -> HashMap<String, AttributeValue> {
        HashMap::from([
            (
                "PK".to_string(),
                s(&format!("PRINCIPAL#cognito-group#{ISSUER}#{group}")),
            ),
            ("SK".to_string(), s(&format!("ROLE#owner#DOMAIN#{domain}"))),
        ])
    }

    fn bindings_query_rule(items: Vec<HashMap<String, AttributeValue>>) -> Rule {
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req| {
                req.table_name() == Some("bindings")
                    && req
                        .key_condition_expression()
                        .is_some_and(|e| e.contains("begins_with(SK, :prefix)"))
            })
            .then_output(move || {
                QueryOutput::builder()
                    .set_items(Some(items.clone()))
                    .build()
            })
    }

    fn scan_rule(items: Vec<HashMap<String, AttributeValue>>) -> Rule {
        mock!(aws_sdk_dynamodb::Client::scan)
            .then_output(move || ScanOutput::builder().set_items(Some(items.clone())).build())
    }

    fn batch_get_rule(items: Vec<HashMap<String, AttributeValue>>) -> Rule {
        mock!(aws_sdk_dynamodb::Client::batch_get_item).then_output(move || {
            BatchGetItemOutput::builder()
                .responses("domains", items.clone())
                .build()
        })
    }

    fn body_domains(resp: &Value) -> Vec<Value> {
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        body["domains"].as_array().unwrap().clone()
    }

    #[tokio::test]
    async fn team_caller_lists_only_owned_domains() {
        set_env();
        let bindings = bindings_query_rule(vec![
            owner_binding_item("orders", "team-payments"),
            owner_binding_item("payments", "team-payments"),
        ]);
        let batch = batch_get_rule(vec![
            domain_item("orders", "ACTIVE", "111122223333"),
            domain_item("payments", "ACTIVE", "444455556666"),
        ]);
        let ddb = mock_client!(aws_sdk_dynamodb, [&bindings, &batch]);

        let resp = handle(
            &ddb,
            event_with(oauth_ctx(&[], &["team-payments"]), Value::Null),
        )
        .await
        .unwrap();
        assert_eq!(resp["statusCode"], 200);

        let domains = body_domains(&resp);
        let names: Vec<&str> = domains
            .iter()
            .map(|d| d["domainName"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["orders", "payments"]);
        for item in &domains {
            assert!(
                item.get("ownerEmail").is_some(),
                "management view keeps full records"
            );
            assert!(
                item.get("taskToken").is_none(),
                "workflow task token must never appear in responses"
            );
        }
    }

    #[tokio::test]
    async fn team_caller_status_filter_applies_within_owned_scope() {
        set_env();
        let bindings = bindings_query_rule(vec![
            owner_binding_item("orders", "team-payments"),
            owner_binding_item("payments", "team-payments"),
        ]);
        let batch = batch_get_rule(vec![
            domain_item("orders", "ACTIVE", "111122223333"),
            domain_item("payments", "PENDING_APPROVAL", "444455556666"),
        ]);
        let ddb = mock_client!(aws_sdk_dynamodb, [&bindings, &batch]);

        let resp = handle(
            &ddb,
            event_with(
                oauth_ctx(&[], &["team-payments"]),
                json!({ "status": "ACTIVE" }),
            ),
        )
        .await
        .unwrap();
        let domains = body_domains(&resp);
        assert_eq!(domains.len(), 1);
        assert_eq!(domains[0]["domainName"], "orders");
    }

    #[tokio::test]
    async fn platform_caller_lists_all_domains() {
        set_env();
        let scan = scan_rule(vec![
            domain_item("orders", "ACTIVE", "111122223333"),
            domain_item("billing", "ACTIVE", "777788889999"),
        ]);
        let ddb = mock_client!(aws_sdk_dynamodb, [&scan]);

        let resp = handle(
            &ddb,
            event_with(oauth_ctx(&["platform-admin"], &[]), Value::Null),
        )
        .await
        .unwrap();
        assert_eq!(resp["statusCode"], 200);
        let domains = body_domains(&resp);
        let names: Vec<&str> = domains
            .iter()
            .map(|d| d["domainName"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["orders", "billing"]);
    }

    #[tokio::test]
    async fn iam_caller_lists_all_domains() {
        set_env();
        let scan = scan_rule(vec![domain_item("orders", "ACTIVE", "111122223333")]);
        let ddb = mock_client!(aws_sdk_dynamodb, [&scan]);

        let resp = handle(&ddb, event_with(iam_ctx(), Value::Null))
            .await
            .unwrap();
        assert_eq!(resp["statusCode"], 200);
        assert_eq!(body_domains(&resp).len(), 1);
    }

    #[tokio::test]
    async fn directory_view_returns_names_only_projection() {
        set_env();
        let scan = scan_rule(vec![
            HashMap::from([
                ("domainName".to_string(), s("orders")),
                ("description".to_string(), s("orders domain")),
                ("status".to_string(), s("ACTIVE")),
            ]),
            HashMap::from([
                ("domainName".to_string(), s("payments")),
                ("description".to_string(), s("payments domain")),
                ("status".to_string(), s("ACTIVE")),
            ]),
        ]);
        let ddb = mock_client!(aws_sdk_dynamodb, [&scan]);

        let resp = handle(
            &ddb,
            event_with(
                oauth_ctx(&[], &["team-payments"]),
                json!({ "view": "directory" }),
            ),
        )
        .await
        .unwrap();
        assert_eq!(resp["statusCode"], 200);
        let domains = body_domains(&resp);
        assert_eq!(domains.len(), 2);
        for item in &domains {
            let keys: Vec<&str> = item
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys,
                vec!["domainName", "domainNamespace", "description", "status"],
                "directory projection carries picker-grade fields only"
            );
        }
    }

    #[tokio::test]
    async fn request_without_principal_is_denied() {
        set_env();
        let scan = scan_rule(vec![]);
        let ddb = mock_client!(aws_sdk_dynamodb, [&scan]);

        let resp = handle(&ddb, event_with(json!({}), Value::Null))
            .await
            .unwrap();
        assert_eq!(resp["statusCode"], 403);
    }

    #[test]
    fn max_results_valid_value_is_used() {
        let event = json!({"queryStringParameters": {"maxResults": "50"}});
        assert_eq!(parse_max_results(&event), 50);
    }

    #[test]
    fn max_results_invalid_falls_back_to_default() {
        let event = json!({"queryStringParameters": {"maxResults": "abc"}});
        assert_eq!(parse_max_results(&event), 100);
    }

    #[test]
    fn max_results_negative_is_clamped_to_one() {
        let event = json!({"queryStringParameters": {"maxResults": "-5"}});
        assert_eq!(parse_max_results(&event), 1);
    }

    #[test]
    fn max_results_over_100_is_clamped_to_100() {
        let event = json!({"queryStringParameters": {"maxResults": "500"}});
        assert_eq!(parse_max_results(&event), 100);
    }

    #[test]
    fn pagination_token_round_trips() {
        let mut key = HashMap::new();
        key.insert("domainName".to_string(), s("billing"));
        key.insert("status".to_string(), s("ACTIVE"));

        let token = serialize_pagination_token(&key);
        let restored = deserialize_pagination_token(&token).unwrap();

        assert_eq!(
            restored.get("domainName").unwrap().as_s().unwrap(),
            "billing"
        );
        assert_eq!(restored.get("status").unwrap().as_s().unwrap(), "ACTIVE");
    }
}
