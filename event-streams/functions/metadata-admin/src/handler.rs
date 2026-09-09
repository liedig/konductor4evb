//! Metadata administration reads: dispatch, platform-role authorization,
//! and the paginated revision history. Mutations live in `create`; the
//! route surface is OAuth-only by design (an intentional API-surface
//! exception, like binding administration).

use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde::Serialize;
use serde_json::Value;

use konductor_common::api;
use konductor_common::error::ControlPlaneError;
use konductor_common::metadata::revisions::{
    MetadataStore, MetadataStoreError, RevisionRecord, RevisionView,
};
use konductor_common::principal::{AuthenticationMechanism, GlobalRole, TrustedPrincipal};
use konductor_common::principal_context::log_principal;

use crate::create;
use crate::pagination;

pub const DEFAULT_PAGE_SIZE: i32 = 20;
pub const MAX_PAGE_SIZE: i32 = 50;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = std::env::var("METADATA_TABLE").context("METADATA_TABLE not set")?;
    let store = MetadataStore::new(ddb.clone(), table);
    dispatch(&store, &event.payload).await
}

pub async fn dispatch(store: &MetadataStore, payload: &Value) -> Result<Value, Error> {
    let request_context = payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(_) => {
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ))
        }
    };
    if principal.authentication_mechanism() != AuthenticationMechanism::Oauth {
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::AccessDenied,
        ));
    }

    let method = payload
        .get("httpMethod")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let resource = payload
        .get("resource")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match (method, resource) {
        ("GET", "/metadata/revisions") => {
            if !can_read(&principal) {
                return Ok(denied());
            }
            list_revisions(store, payload).await
        }
        ("GET", "/metadata/revisions/{revision}") => {
            if !can_read(&principal) {
                return Ok(denied());
            }
            get_revision(store, payload).await
        }
        ("POST", "/metadata/revisions") => {
            if !principal.is_platform_admin() {
                return Ok(denied());
            }
            create::create_revision(store, payload, &principal).await
        }
        _ => Ok(api::error_response(
            404,
            konductor_common::models::ApiError {
                code: "ResourceNotFoundException".to_string(),
                message: format!("unknown route {method} {resource}"),
                request_id: None,
                reason: None,
                field_list: None,
            },
        )),
    }
}

/// Reads are open to both controlled platform roles; mutations are
/// admin-only. Any other OAuth identity (team groups, domain roles) is
/// denied without touching configuration.
fn can_read(principal: &TrustedPrincipal) -> bool {
    principal.is_platform_admin()
        || principal
            .global_roles()
            .contains(&GlobalRole::PlatformAuditor)
}

fn denied() -> Value {
    api::from_control_plane_error(&ControlPlaneError::AccessDenied)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RevisionListResponse {
    revisions: Vec<RevisionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_token: Option<String>,
}

async fn list_revisions(store: &MetadataStore, payload: &Value) -> Result<Value, Error> {
    let limit = match api::query_param(payload, "limit") {
        None => DEFAULT_PAGE_SIZE,
        Some(raw) => match raw.parse::<i32>() {
            Ok(v) if (1..=MAX_PAGE_SIZE).contains(&v) => v,
            _ => {
                return Ok(api::from_control_plane_error(&api::validation_error(
                    format!("query parameter 'limit' must be between 1 and {MAX_PAGE_SIZE}"),
                )))
            }
        },
    };
    let exclusive_start = match api::query_param(payload, "nextToken") {
        None => None,
        Some(raw) => match pagination::decode_revision_cursor(&raw) {
            Ok(key) => Some(key),
            Err(e) => return Ok(api::from_control_plane_error(&e)),
        },
    };

    let (records, last_evaluated) = match store.list_revisions(limit, exclusive_start).await {
        Ok(page) => page,
        Err(e) => return Ok(store_failure("list revisions", e)),
    };
    let views = records.iter().map(view_of).collect::<Result<Vec<_>, _>>()?;
    Ok(api::success_response(
        200,
        &RevisionListResponse {
            revisions: views,
            next_token: last_evaluated.and_then(|key| pagination::encode_cursor(&key)),
        },
    ))
}

async fn get_revision(store: &MetadataStore, payload: &Value) -> Result<Value, Error> {
    let raw = api::path_param(payload, "revision").unwrap_or_default();
    let revision: u32 = match raw.parse() {
        Ok(v) => v,
        Err(_) => {
            return Ok(api::from_control_plane_error(&api::validation_error(
                "path parameter 'revision' must be a non-negative number",
            )))
        }
    };
    match store.get_revision(revision).await {
        Ok(None) => Ok(api::from_control_plane_error(
            &ControlPlaneError::MetadataRevisionNotFound { revision },
        )),
        Ok(Some(record)) => match view_of(&record) {
            Ok(view) => Ok(api::success_response(200, &view)),
            Err(e) => Ok(api::from_control_plane_error(&e)),
        },
        Err(e) => Ok(store_failure("read revision", e)),
    }
}

/// Parses the stored canonical documents back into JSON for the response.
/// A document that no longer parses is a corrupt record, not a 404.
pub fn view_of(record: &RevisionRecord) -> Result<RevisionView, ControlPlaneError> {
    let parse = |text: &str, field: &str| -> Result<Value, ControlPlaneError> {
        serde_json::from_str(text).map_err(|_| {
            ControlPlaneError::MetadataConfigurationUnavailable {
                reason: format!("stored {field} is not parseable"),
            }
        })
    };
    Ok(RevisionView {
        revision: record.revision,
        predecessor: record.predecessor,
        definition_digest: record.definition_digest.clone(),
        compiler_format: record.compiler_format,
        created_by: record.created_by.clone(),
        created_at: record.created_at.clone(),
        definition: parse(&record.definition, "definition")?,
        metadata_schema: parse(&record.metadata_schema, "metadata schema")?,
    })
}

/// Configuration reads/writes failing at the service level are
/// unavailability, not internal corruption of the contract.
pub fn store_failure(operation: &str, e: MetadataStoreError) -> Value {
    match e {
        MetadataStoreError::CorruptItem(reason) => {
            api::from_control_plane_error(&ControlPlaneError::MetadataConfigurationUnavailable {
                reason: format!("stored configuration is corrupt: {reason}"),
            })
        }
        other => api::from_control_plane_error(&ControlPlaneError::DynamoDb(format!(
            "{operation}: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use konductor_common::dynamodb::{n, s};
    use konductor_common::metadata::store::revision_sort_key;
    use serde_json::json;

    const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

    fn context(global_roles: &str, team_groups: &str) -> Value {
        json!({
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": "a1b2c3d4-1111-2222-3333-444455556666",
                "issuer": ISSUER,
                "clientId": "client123",
                "globalRoles": global_roles,
                "teamGroups": team_groups
            }
        })
    }

    fn admin_context() -> Value {
        context("[\"platform-admin\"]", "[]")
    }

    fn event(method: &str, resource: &str, ctx: Value) -> Value {
        json!({
            "httpMethod": method,
            "resource": resource,
            "requestContext": ctx,
        })
    }

    fn store_with(rules: &[Rule]) -> MetadataStore {
        MetadataStore::new(
            mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, rules),
            "test-table",
        )
    }

    async fn invoke(store: &MetadataStore, payload: Value) -> Value {
        dispatch(store, &payload).await.unwrap()
    }

    fn status(resp: &Value) -> u16 {
        resp["statusCode"].as_u64().unwrap() as u16
    }

    fn body(resp: &Value) -> Value {
        serde_json::from_str(resp["body"].as_str().unwrap()).unwrap()
    }

    fn revision_item(revision: u32) -> std::collections::HashMap<String, AttributeValue> {
        let mut item = std::collections::HashMap::new();
        item.insert("PK".to_string(), s("REVISION"));
        item.insert("SK".to_string(), s(&revision_sort_key(revision)));
        item.insert("revision".to_string(), n(revision as i64));
        item.insert("predecessor".to_string(), n((revision - 1) as i64));
        item.insert(
            "definition".to_string(),
            s(r#"{"children":[{"kind":"field","name":"traceId","generator":"uuid-v4"}]}"#),
        );
        item.insert("metadataSchema".to_string(), s(r#"{"type":"object"}"#));
        item.insert("definitionDigest".to_string(), s(&"d".repeat(64)));
        item.insert("compilerFormat".to_string(), n(1));
        item.insert("createdBy".to_string(), s("system-bootstrap"));
        item.insert("createdAt".to_string(), s("2026-01-01T00:00:00.000Z"));
        item.insert("operationId".to_string(), s("op-1"));
        item
    }

    fn get_rule_returning(item: std::collections::HashMap<String, AttributeValue>) -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(
                |req: &aws_sdk_dynamodb::operation::get_item::GetItemInput| {
                    req.consistent_read() == Some(true)
                },
            )
            .then_output(move || {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .set_item(Some(item.clone()))
                    .build()
            })
    }

    #[tokio::test]
    async fn unknown_revision_returns_404() {
        let empty = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build()
        });
        let store = store_with(&[empty]);
        let mut payload = event("GET", "/metadata/revisions/{revision}", admin_context());
        payload["pathParameters"] = json!({"revision": "9"});
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 404);
        assert_eq!(body(&resp)["code"], "ResourceNotFoundException");
    }

    #[tokio::test]
    async fn non_numeric_revision_returns_400() {
        let store = store_with(&[]);
        let mut payload = event("GET", "/metadata/revisions/{revision}", admin_context());
        payload["pathParameters"] = json!({"revision": "abc"});
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn auditor_reads_revision_detail_but_cannot_mutate() {
        let rule = get_rule_returning(revision_item(1));
        let store = store_with(&[rule]);

        let auditor = context("[\"platform-auditor\"]", "[]");
        let mut payload = event("GET", "/metadata/revisions/{revision}", auditor.clone());
        payload["pathParameters"] = json!({"revision": "1"});
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 200);
        assert_eq!(body(&resp)["revision"], 1);
        assert_eq!(body(&resp)["definition"]["children"][0]["name"], "traceId");

        let mut mutation = event("POST", "/metadata/revisions", auditor);
        mutation["body"] =
            json!(json!({"expectedHead": 1, "operationId": "op-2", "definition": {}}).to_string());
        let resp = invoke(&store, mutation).await;
        assert_eq!(status(&resp), 403);
    }

    #[tokio::test]
    async fn team_group_and_iam_callers_are_denied_before_any_read() {
        let store = store_with(&[]);

        let team_only = context("[]", "[\"team-orders\"]");
        let mut payload = event("GET", "/metadata/revisions/{revision}", team_only);
        payload["pathParameters"] = json!({"revision": "1"});
        assert_eq!(status(&invoke(&store, payload).await), 403);

        let iam = json!({
            "authorizer": {
                "authenticationMechanism": "iam",
                "principalId": "111122223333",
                "accountId": "111122223333"
            }
        });
        let mut payload = event("GET", "/metadata/revisions/{revision}", iam);
        payload["pathParameters"] = json!({"revision": "1"});
        assert_eq!(status(&invoke(&store, payload).await), 403);
    }

    #[tokio::test]
    async fn admin_lists_revisions_newest_first_with_continuation() {
        let older = revision_item(1);
        let page = vec![revision_item(3), revision_item(2)];
        let mut last_key = std::collections::HashMap::new();
        last_key.insert("PK".to_string(), s("REVISION"));
        last_key.insert("SK".to_string(), s(&revision_sort_key(2)));
        let first_page = mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req: &aws_sdk_dynamodb::operation::query::QueryInput| {
                req.scan_index_forward() == Some(false)
                    && req.limit() == Some(2)
                    && req.exclusive_start_key().is_none()
            })
            .then_output(move || {
                aws_sdk_dynamodb::operation::query::QueryOutput::builder()
                    .set_items(Some(page.clone()))
                    .set_last_evaluated_key(Some(last_key.clone()))
                    .build()
            });
        let second_page = mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req: &aws_sdk_dynamodb::operation::query::QueryInput| {
                req.exclusive_start_key().is_some()
            })
            .then_output(move || {
                aws_sdk_dynamodb::operation::query::QueryOutput::builder()
                    .set_items(Some(vec![older.clone()]))
                    .build()
            });

        let store = store_with(&[first_page, second_page]);
        let mut payload = event("GET", "/metadata/revisions", admin_context());
        payload["queryStringParameters"] = json!({"limit": "2"});
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 200);
        let listed = body(&resp);
        assert_eq!(
            listed["revisions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["revision"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [3, 2]
        );
        let token = listed["nextToken"].as_str().unwrap().to_string();

        let mut next = event("GET", "/metadata/revisions", admin_context());
        next["queryStringParameters"] = json!({"limit": "2", "nextToken": token});
        let resp = invoke(&store, next).await;
        assert_eq!(status(&resp), 200);
        let listed = body(&resp);
        assert_eq!(listed["revisions"][0]["revision"], 1);
        assert!(listed.get("nextToken").is_none());
    }

    #[tokio::test]
    async fn invalid_limit_and_token_are_rejected() {
        let store = store_with(&[]);
        let mut payload = event("GET", "/metadata/revisions", admin_context());
        payload["queryStringParameters"] = json!({"limit": "0"});
        assert_eq!(status(&invoke(&store, payload).await), 400);

        let mut payload = event("GET", "/metadata/revisions", admin_context());
        payload["queryStringParameters"] = json!({"nextToken": "garbage!"});
        assert_eq!(status(&invoke(&store, payload).await), 400);
    }

    #[tokio::test]
    async fn unknown_routes_are_404() {
        let store = store_with(&[]);
        let payload = event("PATCH", "/metadata/revisions", admin_context());
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 404);
        assert_eq!(body(&resp)["code"], "ResourceNotFoundException");
    }
}
