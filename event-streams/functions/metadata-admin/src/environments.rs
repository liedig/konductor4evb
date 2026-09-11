//! Environment state and activation history reads. Both are read-only:
//! environment state is the authoritative pointer snapshot admission
//! consumes, and the activation history pages the durable audit records
//! committed with each activation transaction.

use std::collections::HashMap;

use lambda_runtime::Error;
use serde::Serialize;
use serde_json::Value;

use konductor_common::api;
use konductor_common::error::ControlPlaneError;
use konductor_common::metadata::pointers::AUDIT_OPERATION_ENVIRONMENT_ACTIVATED;
use konductor_common::metadata::revisions::{MetadataStore, MutationAuditRecord};

use crate::context::AdminConfig;
use crate::handler::store_failure;
use crate::pagination;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentState {
    environment: String,
    active_revision: u32,
    generation: u64,
    legacy: bool,
    activated_by: String,
    activated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentListResponse {
    environments: Vec<EnvironmentState>,
}

/// Reports every configured environment's pointer. A configured
/// environment without a pointer is configuration unavailability, not an
/// empty answer: admission would fail the same way.
pub async fn list_environments(
    store: &MetadataStore,
    config: &AdminConfig,
) -> Result<Value, Error> {
    let pointers = match store.list_environment_pointers().await {
        Ok(p) => p,
        Err(e) => return Ok(store_failure("list environment pointers", e)),
    };
    let by_name = pointers
        .iter()
        .map(|p| (p.environment.clone(), p))
        .collect::<HashMap<_, _>>();
    let mut states = Vec::with_capacity(config.environments.len());
    for environment in &config.environments {
        let pointer = match by_name.get(environment.as_str()) {
            Some(p) => *p,
            None => {
                return Ok(api::from_control_plane_error(
                    &ControlPlaneError::MetadataConfigurationUnavailable {
                        reason: format!("environment '{environment}' has no active pointer"),
                    },
                ))
            }
        };
        states.push(EnvironmentState {
            environment: pointer.environment.clone(),
            active_revision: pointer.active_revision,
            generation: pointer.generation,
            legacy: pointer.active_revision == 0,
            activated_by: pointer.activated_by.clone(),
            activated_at: pointer.activated_at.clone(),
        });
    }
    Ok(api::success_response(
        200,
        &EnvironmentListResponse {
            environments: states,
        },
    ))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivationListResponse {
    activations: Vec<MutationAuditRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_token: Option<String>,
}

/// Pages the durable activation audit history, newest first. Only the
/// committed activation records appear: an activation that lost its race
/// never committed and so never entered this history.
pub async fn list_activations(store: &MetadataStore, payload: &Value) -> Result<Value, Error> {
    let (limit, exclusive_start) =
        match pagination::page_request(payload, pagination::decode_audit_cursor) {
            Ok(page) => page,
            Err(response) => return Ok(response),
        };
    let (records, last_evaluated) = match store
        .list_audit_records(
            limit,
            exclusive_start,
            Some(AUDIT_OPERATION_ENVIRONMENT_ACTIVATED),
        )
        .await
    {
        Ok(page) => page,
        Err(e) => return Ok(store_failure("list activation history", e)),
    };
    Ok(api::success_response(
        200,
        &ActivationListResponse {
            activations: records,
            next_token: last_evaluated.and_then(|key| pagination::encode_cursor(&key)),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use konductor_common::dynamodb::{n, s};
    use serde_json::json;

    fn config() -> AdminConfig {
        AdminConfig {
            region: "us-east-1".to_string(),
            organization: "acme".to_string(),
            environments: vec!["dev".to_string(), "prod".to_string()],
        }
    }

    fn store_with(rules: &[Rule]) -> MetadataStore {
        MetadataStore::new(
            mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, rules),
            "test-table",
        )
    }

    fn status(resp: &Value) -> u16 {
        resp["statusCode"].as_u64().unwrap() as u16
    }

    fn body(resp: &Value) -> Value {
        serde_json::from_str(resp["body"].as_str().unwrap()).unwrap()
    }

    fn pointer_item(
        environment: &str,
        active_revision: u32,
        generation: u64,
    ) -> HashMap<String, AttributeValue> {
        let mut item = HashMap::new();
        item.insert("PK".to_string(), s("ENV"));
        item.insert("SK".to_string(), s(environment));
        item.insert("activeRevision".to_string(), n(active_revision as i64));
        item.insert("generation".to_string(), n(generation as i64));
        item.insert("activatedBy".to_string(), s("admin-sub"));
        item.insert("activatedAt".to_string(), s("2026-01-01T00:00:00.000Z"));
        item
    }

    fn pointers_rule(items: Vec<HashMap<String, AttributeValue>>) -> Rule {
        mock!(aws_sdk_dynamodb::Client::query).then_output(move || {
            aws_sdk_dynamodb::operation::query::QueryOutput::builder()
                .set_items(Some(items.clone()))
                .build()
        })
    }

    fn audit_item(
        millis: i64,
        operation: &str,
        before: u32,
        after: u32,
    ) -> HashMap<String, AttributeValue> {
        let mut item = HashMap::new();
        item.insert("PK".to_string(), s("AUDIT"));
        item.insert("SK".to_string(), s(&format!("{millis}#op-{millis}")));
        item.insert("operation".to_string(), s(operation));
        item.insert("actor".to_string(), s("admin-sub"));
        item.insert("beforeRevision".to_string(), n(before as i64));
        item.insert("afterRevision".to_string(), n(after as i64));
        item.insert("operationId".to_string(), s(&format!("op-{millis}")));
        item.insert("occurredAt".to_string(), s("2026-01-01T00:00:00.000Z"));
        item
    }

    fn audit_page_rule(
        items: Vec<HashMap<String, AttributeValue>>,
        last_evaluated: Option<HashMap<String, AttributeValue>>,
        continued: bool,
    ) -> Rule {
        // The rule matches only the activation-history filter and only the
        // page shape it serves, so an unfiltered query or a misrouted page
        // fails loudly.
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(
                move |req: &aws_sdk_dynamodb::operation::query::QueryInput| {
                    req.filter_expression() == Some("#op = :op")
                        && req.exclusive_start_key().is_some() == continued
                },
            )
            .then_output(move || {
                let mut builder = aws_sdk_dynamodb::operation::query::QueryOutput::builder();
                builder = builder.set_items(Some(items.clone()));
                if let Some(key) = last_evaluated.clone() {
                    builder = builder.set_last_evaluated_key(Some(key));
                }
                builder.build()
            })
    }

    #[tokio::test]
    async fn environment_state_reports_configured_environments() {
        // dev sits at legacy 0; prod is activated at revision 2. A pointer
        // for an environment no longer in deployment config is ignored.
        let rule = pointers_rule(vec![
            pointer_item("dev", 0, 0),
            pointer_item("prod", 2, 3),
            pointer_item("old-env", 2, 1),
        ]);
        let store = store_with(&[rule]);
        let resp = list_environments(&store, &config()).await.unwrap();
        assert_eq!(status(&resp), 200);
        let listed = body(&resp)["environments"].as_array().unwrap().clone();
        assert_eq!(listed.len(), 2, "only configured environments appear");
        assert_eq!(listed[0]["environment"], "dev");
        assert_eq!(listed[0]["activeRevision"], 0);
        assert_eq!(listed[0]["legacy"], true);
        assert_eq!(listed[1]["environment"], "prod");
        assert_eq!(listed[1]["activeRevision"], 2);
        assert_eq!(listed[1]["generation"], 3);
        assert_eq!(listed[1]["legacy"], false);
    }

    #[tokio::test]
    async fn configured_environment_without_pointer_is_503() {
        let rule = pointers_rule(vec![pointer_item("dev", 0, 0)]);
        let store = store_with(&[rule]);
        let resp = list_environments(&store, &config()).await.unwrap();
        assert_eq!(status(&resp), 503);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(message.contains("prod"), "{message}");
    }

    #[tokio::test]
    async fn activation_history_pages_committed_activations_newest_first() {
        let mut last_key = HashMap::new();
        last_key.insert("PK".to_string(), s("AUDIT"));
        last_key.insert("SK".to_string(), s("1767225600000#op-1767225600000"));
        let first = audit_page_rule(
            vec![
                audit_item(1_767_225_600_000, "environment-activated", 1, 2),
                audit_item(1_767_225_500_000, "environment-activated", 0, 1),
            ],
            Some(last_key),
            false,
        );
        let second = audit_page_rule(
            vec![audit_item(1_767_225_400_000, "environment-activated", 0, 1)],
            None,
            true,
        );
        let store = store_with(&[first, second]);

        let mut page_one = json!({"queryStringParameters": {"maxResults": "2"}});
        let resp = list_activations(&store, &page_one).await.unwrap();
        assert_eq!(status(&resp), 200);
        let listed = body(&resp);
        let activations = listed["activations"].as_array().unwrap().clone();
        assert_eq!(activations.len(), 2);
        assert_eq!(activations[0]["operation"], "environment-activated");
        assert_eq!(activations[0]["afterRevision"], 2);
        assert_eq!(activations[1]["beforeRevision"], 0);
        let token = listed["nextToken"].as_str().unwrap().to_string();

        page_one["queryStringParameters"] = json!({"maxResults": "2", "nextToken": token});
        let resp = list_activations(&store, &page_one).await.unwrap();
        assert_eq!(status(&resp), 200);
        let listed = body(&resp);
        assert_eq!(listed["activations"].as_array().unwrap().len(), 1);
        assert!(listed.get("nextToken").is_none());
    }

    #[tokio::test]
    async fn invalid_paging_parameters_are_rejected() {
        let store = store_with(&[]);
        let payload = json!({"queryStringParameters": {"maxResults": "0"}});
        let resp = list_activations(&store, &payload).await.unwrap();
        assert_eq!(status(&resp), 400);

        let payload = json!({"queryStringParameters": {"nextToken": "garbage!"}});
        let resp = list_activations(&store, &payload).await.unwrap();
        assert_eq!(status(&resp), 400);
    }
}
