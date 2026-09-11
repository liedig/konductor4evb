//! Environment activation: the pointer cutover. Readiness is re-checked
//! against server state (never trusting client preflight results), then
//! one conditional transaction moves the pointer and commits the audit
//! record. Reapplying the active revision is a no-op; backward moves are
//! rejected outright; nothing here can move another environment's pointer.

use chrono::Utc;
use lambda_runtime::Error;
use serde::Serialize;
use serde_json::Value;

use konductor_common::api;
use konductor_common::error::ControlPlaneError;
use konductor_common::metadata::pointers::{
    ActivationIntent, EnvironmentPointer, AUDIT_OPERATION_ENVIRONMENT_ACTIVATED,
};
use konductor_common::metadata::revisions::{
    MetadataStore, MetadataStoreError, MutationAuditRecord,
};
use konductor_common::principal::TrustedPrincipal;

use crate::context::AdminConfig;
use crate::create::valid_operation_id;
use crate::handler::store_failure;
use crate::readiness::run_checks;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActivationRequest {
    revision: u32,
    environment: String,
    expected_active_revision: u32,
    operation_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivationResponse {
    environment: String,
    active_revision: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation: Option<u64>,
    activated_by: String,
    activated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition_digest: Option<String>,
    already_active: bool,
}

pub async fn activate(
    store: &MetadataStore,
    config: &AdminConfig,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let request: ActivationRequest = match api::parse_body(payload) {
        Ok(r) => r,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    if !valid_operation_id(&request.operation_id) {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "operationId must be 1-128 characters of printable text",
        )));
    }
    if request.revision == 0 {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "revision 0 is the read-only legacy selector and cannot be activated",
        )));
    }
    if !config.is_configured_environment(&request.environment) {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "environment is not one of the configured deployment environments",
        )));
    }

    // Readiness is recomputed from server state on every activation.
    let report = match run_checks(store, config, request.revision, &request.environment).await {
        Ok(report) => report,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    // Reapplying the active revision is a no-op: confirm the pointer and
    // answer with current state, committing nothing.
    if request.revision == request.expected_active_revision {
        return same_revision_noop(store, &request).await;
    }
    // Activation only moves forward: reverting a pointer would remove
    // fields from new events and violate the additive contract.
    if request.revision < request.expected_active_revision {
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::MetadataConflict {
                reason: format!(
                    "cannot activate revision {} after expected active revision {}; \
                     activation moves forward only",
                    request.revision, request.expected_active_revision
                ),
            },
        ));
    }

    // The success response reports the bumped generation, so the pre-transaction
    // value is read here. The transaction condition pins activeRevision to the
    // expected value and activation moves forward only, so a commit means this
    // read is exactly the state the transaction incremented.
    let pointer = match store.get_environment_pointer(&request.environment).await {
        Ok(Some(pointer)) => pointer,
        Ok(None) => {
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::MetadataConfigurationUnavailable {
                    reason: format!(
                        "environment '{}' has no active pointer",
                        request.environment
                    ),
                },
            ))
        }
        Err(e) => return Ok(store_failure("read pointer before activation", e)),
    };

    let now = Utc::now();
    let audit = MutationAuditRecord {
        operation: AUDIT_OPERATION_ENVIRONMENT_ACTIVATED.to_string(),
        actor: principal.principal_id().to_string(),
        before_revision: request.expected_active_revision,
        after_revision: request.revision,
        operation_id: request.operation_id.clone(),
        occurred_at: now.to_rfc3339(),
        occurred_at_millis: now.timestamp_millis(),
    };
    let intent = ActivationIntent {
        environment: request.environment.clone(),
        candidate: request.revision,
        definition_digest: report.definition_digest.clone(),
        actor: principal.principal_id().to_string(),
        when: now.to_rfc3339(),
        expected_active: request.expected_active_revision,
    };
    match store.activate_revision(&intent, &audit).await {
        Ok(()) => Ok(api::success_response(
            200,
            &ActivationResponse {
                environment: request.environment,
                active_revision: request.revision,
                generation: Some(pointer.generation + 1),
                activated_by: principal.principal_id().to_string(),
                activated_at: now.to_rfc3339(),
                definition_digest: Some(report.definition_digest),
                already_active: false,
            },
        )),
        Err(MetadataStoreError::ConditionFailed) => {
            resolve_conflict(
                store,
                &request.environment,
                request.revision,
                request.expected_active_revision,
            )
            .await
        }
        Err(e) => Ok(store_failure("activate revision", e)),
    }
}

/// Confirms the pointer already holds the candidate revision and answers
/// with current state. A mismatched expectation or missing pointer is a
/// conflict or unavailability, never a write.
async fn same_revision_noop(
    store: &MetadataStore,
    request: &ActivationRequest,
) -> Result<Value, Error> {
    match store.get_environment_pointer(&request.environment).await {
        Ok(Some(pointer)) if pointer.active_revision == request.revision => {
            Ok(already_active_response(&pointer))
        }
        Ok(Some(_)) => Ok(api::from_control_plane_error(
            &ControlPlaneError::MetadataConflict {
                reason: format!(
                    "expected active revision {} does not match the pointer; refresh and retry",
                    request.expected_active_revision
                ),
            },
        )),
        Ok(None) => Ok(api::from_control_plane_error(
            &ControlPlaneError::MetadataConfigurationUnavailable {
                reason: format!(
                    "environment '{}' has no active pointer",
                    request.environment
                ),
            },
        )),
        Err(e) => Ok(store_failure("read pointer for no-op activation", e)),
    }
}

/// A transaction that lost its race re-reads the pointer: the candidate
/// being active is an idempotent no-op, a newer active revision means the
/// move is backward, and anything else is a stale expectation.
async fn resolve_conflict(
    store: &MetadataStore,
    environment: &str,
    candidate: u32,
    expected_active: u32,
) -> Result<Value, Error> {
    match store.get_environment_pointer(environment).await {
        Ok(Some(pointer)) if pointer.active_revision == candidate => Ok(already_active_response(&pointer)),
        Ok(Some(pointer)) if pointer.active_revision > candidate => {
            Ok(api::from_control_plane_error(&ControlPlaneError::MetadataConflict {
                reason: format!(
                    "environment '{environment}' is already on revision {}, which is newer than {}; \
                     activation moves forward only",
                    pointer.active_revision, candidate
                ),
            }))
        }
        Ok(Some(pointer)) => Ok(api::from_control_plane_error(
            &ControlPlaneError::MetadataConflict {
                reason: format!(
                    "expected active revision {} but environment '{environment}' is on revision {}; \
                     refresh and retry",
                    expected_active, pointer.active_revision
                ),
            },
        )),
        Ok(None) => Ok(api::from_control_plane_error(
            &ControlPlaneError::MetadataConfigurationUnavailable {
                reason: format!("environment '{environment}' has no active pointer"),
            },
        )),
        Err(e) => Ok(store_failure("re-read pointer after conflict", e)),
    }
}

fn already_active_response(pointer: &EnvironmentPointer) -> Value {
    api::success_response(
        200,
        &ActivationResponse {
            environment: pointer.environment.clone(),
            active_revision: pointer.active_revision,
            generation: None,
            activated_by: pointer.activated_by.clone(),
            activated_at: pointer.activated_at.clone(),
            definition_digest: None,
            already_active: true,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_sdk_dynamodb::types::CancellationReason;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use konductor_common::dynamodb::{n, s};
    use konductor_common::metadata::definition::{MetadataDefinition, ValidatedMetadataDefinition};
    use konductor_common::metadata::schema::compile_metadata_schema;
    use konductor_common::metadata::selector::MetadataRevision;
    use konductor_common::metadata::store::revision_sort_key;
    use serde_json::json;

    const DEFINITION: &str =
        r#"{"children":[{"kind":"field","name":"traceId","generator":"uuid-v4"}]}"#;

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

    fn admin() -> TrustedPrincipal {
        TrustedPrincipal::oauth(
            "admin-sub",
            "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh",
            "client123",
            vec![konductor_common::principal::GlobalRole::PlatformAdmin],
            vec![],
        )
    }

    async fn invoke(store: &MetadataStore, payload: Value) -> Value {
        activate(store, &config(), &payload, &admin())
            .await
            .unwrap()
    }

    fn status(resp: &Value) -> u16 {
        resp["statusCode"].as_u64().unwrap() as u16
    }

    fn body(resp: &Value) -> Value {
        serde_json::from_str(resp["body"].as_str().unwrap()).unwrap()
    }

    fn payload(revision: u32, environment: &str, expected: u32) -> Value {
        json!({
            "httpMethod": "POST",
            "resource": "/metadata/activations",
            "requestContext": json!({"authorizer": {}}),
            "body": json!({
                "revision": revision,
                "environment": environment,
                "expectedActiveRevision": expected,
                "operationId": "op-act-1"
            })
            .to_string(),
        })
    }

    /// A healthy revision record for the readiness recheck.
    fn revision_rule(revision: u32) -> Rule {
        let validated = ValidatedMetadataDefinition::validate(
            serde_json::from_str::<MetadataDefinition>(DEFINITION).unwrap(),
        )
        .unwrap();
        let compiled =
            compile_metadata_schema(MetadataRevision::new(revision).unwrap(), &validated).unwrap();
        let digest = validated.definition_digest().unwrap();
        get_rule_for(&revision_sort_key(revision), {
            let mut item = std::collections::HashMap::<String, AttributeValue>::new();
            item.insert("PK".to_string(), s("REVISION"));
            item.insert("SK".to_string(), s(&revision_sort_key(revision)));
            item.insert("revision".to_string(), n(revision as i64));
            item.insert("predecessor".to_string(), n((revision - 1) as i64));
            item.insert("definition".to_string(), s(DEFINITION));
            item.insert("metadataSchema".to_string(), s(&compiled.document_string()));
            item.insert("definitionDigest".to_string(), s(&digest));
            item.insert("compilerFormat".to_string(), n(1));
            item.insert("createdBy".to_string(), s("system-bootstrap"));
            item.insert("createdAt".to_string(), s("2026-01-01T00:00:00.000Z"));
            item.insert("operationId".to_string(), s("op-1"));
            item
        })
    }

    fn pointer_rule(environment: &str, active_revision: u32, generation: u64) -> Rule {
        get_rule_for(environment, {
            let mut item = std::collections::HashMap::<String, AttributeValue>::new();
            item.insert("PK".to_string(), s("ENV"));
            item.insert("SK".to_string(), s(environment));
            item.insert("activeRevision".to_string(), n(active_revision as i64));
            item.insert("generation".to_string(), n(generation as i64));
            item.insert("activatedBy".to_string(), s("admin-sub"));
            item.insert("activatedAt".to_string(), s("2026-01-01T00:00:00.000Z"));
            item
        })
    }

    fn get_rule_for(sk: &str, item: std::collections::HashMap<String, AttributeValue>) -> Rule {
        let sk = sk.to_string();
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(
                move |req: &aws_sdk_dynamodb::operation::get_item::GetItemInput| {
                    req.key()
                        .and_then(|keys| keys.get("SK"))
                        .and_then(|v| v.as_s().ok())
                        .is_some_and(|key| key == sk.as_str())
                },
            )
            .then_output(move || {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .set_item(Some(item.clone()))
                    .build()
            })
    }

    fn empty_get_rule(sk: &str) -> Rule {
        let sk = sk.to_string();
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(
                move |req: &aws_sdk_dynamodb::operation::get_item::GetItemInput| {
                    req.key()
                        .and_then(|keys| keys.get("SK"))
                        .and_then(|v| v.as_s().ok())
                        .is_some_and(|key| key == sk.as_str())
                },
            )
            .then_output(|| aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build())
    }

    fn transact_ok(expected: u32, candidate: u32, environment: &str) -> Rule {
        let environment = environment.to_string();
        let expected = expected.to_string();
        let candidate = candidate.to_string();
        mock!(aws_sdk_dynamodb::Client::transact_write_items)
            .match_requests(move |req: &aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsInput| {
                let items = req.transact_items();
                if items.len() != 2 {
                    return false;
                }
                let Some(update) = items.iter().find_map(|i| i.update()) else {
                    return false;
                };
                let Some(audit) = items.iter().find_map(|i| i.put()) else {
                    return false;
                };
                let value_of = |vals: Option<
                    &std::collections::HashMap<String, AttributeValue>,
                >,
                                name: &str| {
                    vals.and_then(|vals| vals.get(name))
                        .and_then(|v| v.as_n().ok())
                        .cloned()
                };
                update.condition_expression()
                    == Some("attribute_exists(PK) AND activeRevision = :expected")
                    && update
                        .key()
                        .get("SK")
                        .and_then(|v| v.as_s().ok())
                        .map(String::as_str)
                        == Some(environment.as_str())
                    && value_of(update.expression_attribute_values(), ":expected")
                        == Some(expected.clone())
                    && value_of(update.expression_attribute_values(), ":candidate")
                        == Some(candidate.clone())
                    && update
                        .update_expression()
                        .contains("generation = if_not_exists")
                    && audit.condition_expression() == Some("attribute_not_exists(PK)")
                    && audit
                        .item()
                        .get("operation")
                        .and_then(|v| v.as_s().ok())
                        .map(String::as_str)
                        == Some("environment-activated")
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsOutput::builder().build()
            })
    }

    fn transact_conditional_failure() -> Rule {
        mock!(aws_sdk_dynamodb::Client::transact_write_items).then_error(|| {
            let reason = CancellationReason::builder()
                .code("ConditionalCheckFailed")
                .build();
            aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError::TransactionCanceledException(
                aws_sdk_dynamodb::types::error::TransactionCanceledException::builder()
                    .cancellation_reasons(reason)
                    .build(),
            )
        })
    }

    #[tokio::test]
    async fn activation_moves_the_pointer_and_commits_audit_together() {
        let transact = transact_ok(1, 2, "dev");
        let store = store_with(&[revision_rule(2), pointer_rule("dev", 1, 4), transact]);
        let resp = invoke(&store, payload(2, "dev", 1)).await;
        assert_eq!(status(&resp), 200, "{:?}", body(&resp));
        let activated = body(&resp);
        assert_eq!(activated["activeRevision"], 2);
        assert_eq!(activated["alreadyActive"], false);
        assert_eq!(activated["generation"], 5);
        assert_eq!(activated["definitionDigest"].as_str().unwrap().len(), 64);
    }

    #[tokio::test]
    async fn same_revision_activation_is_a_noop_without_a_transaction() {
        // No transact rule exists: any write attempt fails the mock.
        let store = store_with(&[revision_rule(2), pointer_rule("dev", 2, 4)]);
        let resp = invoke(&store, payload(2, "dev", 2)).await;
        assert_eq!(status(&resp), 200, "{:?}", body(&resp));
        let no_op = body(&resp);
        assert_eq!(no_op["alreadyActive"], true);
        assert_eq!(no_op["activeRevision"], 2);
        assert!(no_op.get("generation").is_none());
    }

    #[tokio::test]
    async fn backward_activation_is_rejected() {
        let store = store_with(&[revision_rule(1)]);
        let resp = invoke(&store, payload(1, "dev", 3)).await;
        assert_eq!(status(&resp), 409);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(message.contains("forward only"), "{message}");
    }

    #[tokio::test]
    async fn legacy_revision_zero_cannot_be_activated() {
        let store = store_with(&[]);
        let resp = invoke(&store, payload(0, "dev", 1)).await;
        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn stale_expectation_conflict_reports_current_pointer_state() {
        // The pointer sits at revision 1 while the client expected 2: the
        // conditional transaction fails and the resolution reports the
        // authoritative current state.
        let store = store_with(&[
            revision_rule(3),
            pointer_rule("dev", 1, 4),
            transact_conditional_failure(),
            pointer_rule("dev", 1, 4),
        ]);
        let resp = invoke(&store, payload(3, "dev", 2)).await;
        assert_eq!(status(&resp), 409);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(message.contains("is on revision 1"), "{message}");
        assert!(message.contains("expected active revision 2"), "{message}");
    }

    #[tokio::test]
    async fn race_lost_to_a_newer_activation_is_backward_not_stale() {
        // Another admin already moved the environment past our candidate:
        // forward-only means our move cannot land.
        let store = store_with(&[
            revision_rule(2),
            pointer_rule("dev", 5, 2),
            transact_conditional_failure(),
            pointer_rule("dev", 5, 2),
        ]);
        let resp = invoke(&store, payload(2, "dev", 1)).await;
        assert_eq!(status(&resp), 409);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(message.contains("newer than 2"), "{message}");
    }

    #[tokio::test]
    async fn race_won_by_the_same_revision_is_an_idempotent_noop() {
        let store = store_with(&[
            revision_rule(2),
            pointer_rule("dev", 2, 5),
            transact_conditional_failure(),
            pointer_rule("dev", 2, 5),
        ]);
        let resp = invoke(&store, payload(2, "dev", 1)).await;
        assert_eq!(status(&resp), 200);
        let no_op = body(&resp);
        assert_eq!(no_op["alreadyActive"], true);
        assert!(no_op.get("generation").is_none());
    }

    #[tokio::test]
    async fn missing_pointer_is_503_not_a_silent_activation() {
        let store = store_with(&[
            revision_rule(2),
            transact_conditional_failure(),
            empty_get_rule("dev"),
        ]);
        let resp = invoke(&store, payload(2, "dev", 1)).await;
        assert_eq!(status(&resp), 503);
    }

    #[tokio::test]
    async fn unknown_revision_is_404_before_any_pointer_write() {
        // Readiness is recomputed from server state: a revision that does
        // not exist can never be activated, and no write is attempted.
        let store = store_with(&[empty_get_rule("r0000000009")]);
        let resp = invoke(&store, payload(9, "dev", 1)).await;
        assert_eq!(status(&resp), 404);
    }

    #[tokio::test]
    async fn readiness_drift_blocks_activation() {
        // The stored schema no longer recompiles exactly: activation must
        // refuse rather than trust the client's preflight claim.
        let validated = ValidatedMetadataDefinition::validate(
            serde_json::from_str::<MetadataDefinition>(DEFINITION).unwrap(),
        )
        .unwrap();
        let digest = validated.definition_digest().unwrap();
        let drifted = get_rule_for(&revision_sort_key(2), {
            let mut item = std::collections::HashMap::<String, AttributeValue>::new();
            item.insert("PK".to_string(), s("REVISION"));
            item.insert("SK".to_string(), s(&revision_sort_key(2)));
            item.insert("revision".to_string(), n(2));
            item.insert("predecessor".to_string(), n(1));
            item.insert("definition".to_string(), s(DEFINITION));
            item.insert("metadataSchema".to_string(), s(r#"{"type":"object"}"#));
            item.insert("definitionDigest".to_string(), s(&digest));
            item.insert("compilerFormat".to_string(), n(1));
            item.insert("createdBy".to_string(), s("system-bootstrap"));
            item.insert("createdAt".to_string(), s("2026-01-01T00:00:00.000Z"));
            item.insert("operationId".to_string(), s("op-1"));
            item
        });
        let store = store_with(&[drifted]);
        let resp = invoke(&store, payload(2, "dev", 1)).await;
        assert_eq!(status(&resp), 503);
    }

    #[tokio::test]
    async fn unknown_environment_is_400_before_any_read() {
        let store = store_with(&[]);
        let resp = invoke(&store, payload(2, "staging", 1)).await;
        assert_eq!(status(&resp), 400);
    }
}
