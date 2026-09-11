//! Revision creation: one conditional transaction per mutation, additive
//! validation against the expected head, and deterministic replay/conflict
//! resolution when the transaction loses a race.

use chrono::Utc;
use lambda_runtime::Error;
use serde::Deserialize;
use serde_json::Value;

use konductor_common::api;
use konductor_common::error::ControlPlaneError;
use konductor_common::metadata::additive::check_additive;
use konductor_common::metadata::definition::{MetadataDefinition, ValidatedMetadataDefinition};
use konductor_common::metadata::revisions::{
    HeadRecord, MetadataStore, MetadataStoreError, MutationAuditRecord, RevisionRecord,
    AUDIT_OPERATION_REVISION_CREATED,
};
use konductor_common::metadata::schema::compile_metadata_schema;
use konductor_common::metadata::selector::MetadataRevision;
use konductor_common::principal::TrustedPrincipal;

use crate::handler::{store_failure, view_of};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateRevisionRequest {
    expected_head: u32,
    operation_id: String,
    definition: MetadataDefinition,
}

const MAX_OPERATION_ID_LEN: usize = 128;

pub async fn create_revision(
    store: &MetadataStore,
    payload: &Value,
    principal: &TrustedPrincipal,
) -> Result<Value, Error> {
    let request: CreateRevisionRequest = match api::parse_body(payload) {
        Ok(r) => r,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    if !valid_operation_id(&request.operation_id) {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "operationId must be 1-128 characters of printable text",
        )));
    }
    if request.expected_head == 0 {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "expectedHead must reference an existing positive revision; 0 is the legacy selector",
        )));
    }

    // The predecessor is the revision the client believes is the head. The
    // transaction below enforces that belief against the real head; this
    // read exists so the additive check compares against exactly the
    // revision the head move would start from.
    let predecessor = match store.get_revision(request.expected_head).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            return Ok(api::from_control_plane_error(&api::validation_error(
                "expectedHead does not reference an existing revision",
            )))
        }
        Err(e) => return Ok(store_failure("read expected head revision", e)),
    };
    let predecessor_definition = match parse_stored_definition(&predecessor.definition) {
        Ok(d) => d,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    let candidate = match check_additive(request.definition, &predecessor_definition) {
        Ok(c) => c,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    let candidate_revision = request.expected_head + 1;
    let Ok(revision) = MetadataRevision::new(candidate_revision) else {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "the candidate revision must be positive",
        )));
    };
    let compiled = match compile_metadata_schema(revision, &candidate) {
        Ok(c) => c,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    let digest = match candidate.definition_digest() {
        Ok(d) => d,
        Err(e) => {
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::MetadataConfigurationUnavailable { reason: e },
            ))
        }
    };

    let now = Utc::now();
    let record = RevisionRecord {
        revision: candidate_revision,
        predecessor: request.expected_head,
        definition: String::from_utf8(
            candidate
                .canonical_bytes()
                .map_err(|e| ControlPlaneError::MetadataConfigurationUnavailable { reason: e })?,
        )
        .map_err(|e| ControlPlaneError::MetadataConfigurationUnavailable {
            reason: e.to_string(),
        })?,
        metadata_schema: compiled.document_string(),
        definition_digest: digest.clone(),
        compiler_format: compiled.compiler_format,
        created_by: principal.principal_id().to_string(),
        created_at: now.to_rfc3339(),
        operation_id: request.operation_id.clone(),
    };
    let audit = MutationAuditRecord {
        operation: AUDIT_OPERATION_REVISION_CREATED.to_string(),
        actor: principal.principal_id().to_string(),
        before_revision: request.expected_head,
        after_revision: candidate_revision,
        operation_id: request.operation_id.clone(),
        occurred_at: now.to_rfc3339(),
        occurred_at_millis: now.timestamp_millis(),
    };

    match store
        .create_revision(&record, &audit, request.expected_head)
        .await
    {
        Ok(()) => Ok(api::success_response(201, &view_of(&record)?)),
        Err(MetadataStoreError::ConditionFailed) => {
            resolve_conflict(store, &request.operation_id, candidate_revision, &digest).await
        }
        Err(e) => Ok(store_failure("create revision", e)),
    }
}

/// A transaction that lost its race means the head moved or the revision
/// slot filled. The slot record decides: an identical recorded operation is
/// replayed (the recorded outcome is returned), the same operation id with
/// different content conflicts, and anyone else's revision is a stale head.
async fn resolve_conflict(
    store: &MetadataStore,
    operation_id: &str,
    candidate_revision: u32,
    candidate_digest: &str,
) -> Result<Value, Error> {
    match store.get_revision(candidate_revision).await {
        Ok(Some(existing)) => {
            if existing.operation_id != operation_id {
                let head = match store.get_head().await {
                    Ok(head) => head,
                    Err(e) => return Ok(store_failure("re-read head after conflict", e)),
                };
                return Ok(stale_head_response(head, candidate_revision));
            }
            if existing.definition_digest != candidate_digest {
                return Ok(api::from_control_plane_error(
                    &ControlPlaneError::MetadataConflict {
                        reason: format!(
                            "operation id '{operation_id}' was already used with different content"
                        ),
                    },
                ));
            }
            Ok(api::success_response(200, &view_of(&existing)?))
        }
        Ok(None) => match store.get_head().await {
            Ok(Some(head)) => Ok(stale_head_response(Some(head), candidate_revision)),
            Ok(None) => Ok(api::from_control_plane_error(
                &ControlPlaneError::MetadataConfigurationUnavailable {
                    reason: "the metadata revision head is missing".to_string(),
                },
            )),
            Err(e) => Ok(store_failure("re-read head after conflict", e)),
        },
        Err(e) => Ok(store_failure("re-read revision after conflict", e)),
    }
}

fn stale_head_response(head: Option<HeadRecord>, candidate: u32) -> Value {
    let reason = match head {
        Some(h) => format!(
            "revision {candidate} was created concurrently; the current head is {}",
            h.revision
        ),
        None => format!("revision {candidate} was created concurrently"),
    };
    api::from_control_plane_error(&ControlPlaneError::MetadataConflict { reason })
}

/// Operation ids are printable, bounded request identity carried by every
/// mutation; validation is shared by the revision and activation routes.
pub fn valid_operation_id(operation_id: &str) -> bool {
    !operation_id.is_empty()
        && operation_id.len() <= MAX_OPERATION_ID_LEN
        && operation_id.chars().all(|c| !c.is_control())
}

fn parse_stored_definition(
    canonical: &str,
) -> Result<ValidatedMetadataDefinition, ControlPlaneError> {
    let definition: MetadataDefinition = serde_json::from_str(canonical).map_err(|_| {
        ControlPlaneError::MetadataConfigurationUnavailable {
            reason: "the stored head definition is not parseable".to_string(),
        }
    })?;
    ValidatedMetadataDefinition::validate(definition).map_err(|_| {
        ControlPlaneError::MetadataConfigurationUnavailable {
            reason: "the stored head definition is no longer valid".to_string(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_sdk_dynamodb::types::CancellationReason;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use konductor_common::dynamodb::{n, s};
    use konductor_common::metadata::store::revision_sort_key;
    use serde_json::json;

    const ADMIN_SUB: &str = "a1b2c3d4-1111-2222-3333-444455556666";

    fn admin() -> TrustedPrincipal {
        TrustedPrincipal::oauth(
            ADMIN_SUB,
            "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh",
            "client123",
            vec![konductor_common::principal::GlobalRole::PlatformAdmin],
            vec![],
        )
    }

    fn event_with(body: Value) -> (Value, TrustedPrincipal) {
        (
            json!({
                "httpMethod": "POST",
                "resource": "/metadata/revisions",
                "requestContext": json!({"authorizer": {}}),
                "body": body.to_string(),
            }),
            admin(),
        )
    }

    fn store_with(rules: &[Rule]) -> MetadataStore {
        MetadataStore::new(
            mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, rules),
            "test-table",
        )
    }

    async fn invoke(store: &MetadataStore, payload: Value, principal: &TrustedPrincipal) -> Value {
        create_revision(store, &payload, principal).await.unwrap()
    }

    fn status(resp: &Value) -> u16 {
        resp["statusCode"].as_u64().unwrap() as u16
    }

    fn body(resp: &Value) -> Value {
        serde_json::from_str(resp["body"].as_str().unwrap()).unwrap()
    }

    fn baseline_definition() -> MetadataDefinition {
        serde_json::from_value(json!({
            "children": [
                {"kind": "field", "name": "traceId", "generator": "uuid-v4"}
            ]
        }))
        .unwrap()
    }

    fn baseline_digest() -> String {
        ValidatedMetadataDefinition::validate(baseline_definition())
            .unwrap()
            .definition_digest()
            .unwrap()
    }

    fn revision_item(
        revision: u32,
        operation_id: &str,
        digest: &str,
    ) -> std::collections::HashMap<String, AttributeValue> {
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
        item.insert("definitionDigest".to_string(), s(digest));
        item.insert("compilerFormat".to_string(), n(1));
        item.insert("createdBy".to_string(), s("system-bootstrap"));
        item.insert("createdAt".to_string(), s("2026-01-01T00:00:00.000Z"));
        item.insert("operationId".to_string(), s(operation_id));
        item
    }

    fn predecessor_rule() -> Rule {
        get_rule_for(
            "r0000000001",
            Some(revision_item(1, "op-1", &"d".repeat(64))),
        )
    }

    fn transact_ok() -> Rule {
        mock!(aws_sdk_dynamodb::Client::transact_write_items)
            .match_requests(|req: &aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsInput| {
                let items = req.transact_items();
                items.len() == 3
                    && items.iter().any(|i| i.put().is_some_and(|p|
                        p.condition_expression() == Some("attribute_not_exists(PK)")
                            && p.item().get("SK").and_then(|v| v.as_s().ok())
                                .map(String::as_str)
                                == Some("r0000000002")
                            && p.item().get("operationId").and_then(|v| v.as_s().ok())
                                .map(String::as_str)
                                == Some("op-2")))
                    && items.iter().any(|i| i.update().is_some_and(|u|
                        u.condition_expression() == Some("attribute_exists(PK) AND #rev = :expected")
                            && u.update_expression().contains("definitionDigest")))
                    && items.iter().filter(|i| i.put().is_some()).count() == 2
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

    fn head_item(revision: u32) -> std::collections::HashMap<String, AttributeValue> {
        let mut item = std::collections::HashMap::new();
        item.insert("PK".to_string(), s("HEAD"));
        item.insert("SK".to_string(), s("HEAD"));
        item.insert("revision".to_string(), n(revision as i64));
        item.insert("definitionDigest".to_string(), s(&"d".repeat(64)));
        item.insert("updatedAt".to_string(), s("2026-01-01T00:00:00.000Z"));
        item
    }

    fn get_rule_returning(item: Option<std::collections::HashMap<String, AttributeValue>>) -> Rule {
        get_rule_for("r0000000001", item)
    }

    /// Matches one strongly consistent get by its sort key, so multi-get
    /// flows (predecessor, slot, head) resolve to the right record.
    fn get_rule_for(
        sk: &str,
        item: Option<std::collections::HashMap<String, AttributeValue>>,
    ) -> Rule {
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
                let mut builder = aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder();
                if let Some(item) = item.clone() {
                    builder = builder.set_item(Some(item));
                }
                builder.build()
            })
    }

    fn add_field_body() -> Value {
        json!({
            "expectedHead": 1,
            "operationId": "op-2",
            "definition": {
                "children": [
                    {"kind": "field", "name": "traceId", "generator": "uuid-v4"},
                    {"kind": "field", "name": "admittedAt", "generator": "utc-timestamp"}
                ]
            }
        })
    }

    #[tokio::test]
    async fn admin_creates_additive_revision_atomically() {
        let store = store_with(&[predecessor_rule(), transact_ok()]);
        let (payload, principal) = event_with(add_field_body());
        let resp = invoke(&store, payload, &principal).await;
        assert_eq!(status(&resp), 201, "{:?}", body(&resp));
        let created = body(&resp);
        assert_eq!(created["revision"], 2);
        assert_eq!(created["predecessor"], 1);
        assert_eq!(created["definition"]["children"][1]["name"], "admittedAt");
        assert_eq!(created["definitionDigest"].as_str().unwrap().len(), 64);
    }

    #[tokio::test]
    async fn non_additive_definition_is_400_with_field_path() {
        let store = store_with(&[predecessor_rule()]);
        let (payload, principal) = event_with(json!({
            "expectedHead": 1,
            "operationId": "op-2",
            "definition": {"children": []}
        }));
        let resp = invoke(&store, payload, &principal).await;
        assert_eq!(status(&resp), 400);
        let violations = body(&resp)["fieldList"].as_array().unwrap().clone();
        assert!(violations.iter().any(|v| v["name"] == "traceId"));
    }

    #[tokio::test]
    async fn unknown_expected_head_and_legacy_zero_are_400() {
        let empty = store_with(&[get_rule_for("r0000000007", None)]);
        let (payload, principal) = event_with(json!({
            "expectedHead": 7,
            "operationId": "op-2",
            "definition": {"children": []}
        }));
        assert_eq!(status(&invoke(&empty, payload, &principal).await), 400);

        let store = store_with(&[]);
        let (payload, principal) = event_with(json!({
            "expectedHead": 0,
            "operationId": "op-2",
            "definition": {"children": []}
        }));
        assert_eq!(status(&invoke(&store, payload, &principal).await), 400);
    }

    #[tokio::test]
    async fn invalid_operation_ids_are_400() {
        let store = store_with(&[]);
        for operation_id in ["", &"x".repeat(129), "op\u{7}bad"] {
            let (payload, principal) = event_with(json!({
                "expectedHead": 1,
                "operationId": operation_id,
                "definition": {"children": []}
            }));
            let resp = invoke(&store, payload, &principal).await;
            assert_eq!(status(&resp), 400, "operation id {operation_id:?}");
        }
    }

    #[tokio::test]
    async fn operation_id_replay_returns_the_recorded_outcome() {
        // The committed record for op-2 carries exactly the content this
        // retry computes, so the lost transaction is a replay, not a conflict.
        let digest = baseline_digest();
        let existing = revision_item(2, "op-2", &digest);
        let store = store_with(&[
            predecessor_rule(),
            transact_conditional_failure(),
            get_rule_for("r0000000002", Some(existing)),
        ]);
        let (payload, principal) = event_with(json!({
            "expectedHead": 1,
            "operationId": "op-2",
            "definition": {
                "children": [
                    {"kind": "field", "name": "traceId", "generator": "uuid-v4"}
                ]
            }
        }));
        let resp = invoke(&store, payload, &principal).await;
        assert_eq!(status(&resp), 200, "{:?}", body(&resp));
        assert_eq!(body(&resp)["revision"], 2);
    }

    #[tokio::test]
    async fn operation_id_with_different_content_conflicts() {
        let store = store_with(&[
            predecessor_rule(),
            transact_conditional_failure(),
            get_rule_for(
                "r0000000002",
                Some(revision_item(2, "op-2", &"e".repeat(64))),
            ),
        ]);
        let (payload, principal) = event_with(add_field_body());
        let resp = invoke(&store, payload, &principal).await;
        assert_eq!(status(&resp), 409);
        assert_eq!(body(&resp)["code"], "ConflictException");
        assert!(body(&resp)["message"]
            .as_str()
            .unwrap()
            .contains("different content"));
    }

    #[tokio::test]
    async fn concurrent_save_by_another_admin_is_a_stale_head_conflict() {
        // The slot is taken by a different operation; the head reports the
        // authoritative current revision for the error message.
        let store = store_with(&[
            predecessor_rule(),
            transact_conditional_failure(),
            get_rule_for(
                "r0000000002",
                Some(revision_item(2, "someone-else", &"e".repeat(64))),
            ),
            get_rule_for("HEAD", Some(head_item(2))),
        ]);
        let (payload, principal) = event_with(add_field_body());
        let resp = invoke(&store, payload, &principal).await;
        assert_eq!(status(&resp), 409);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(message.contains("concurrently"), "{message}");
    }

    #[tokio::test]
    async fn missing_head_during_conflict_resolution_is_503() {
        // No slot record and no head: configuration itself is unavailable.
        let store = store_with(&[
            predecessor_rule(),
            transact_conditional_failure(),
            get_rule_for("r0000000002", None),
            get_rule_for("HEAD", None),
        ]);
        let (payload, principal) = event_with(add_field_body());
        let resp = invoke(&store, payload, &principal).await;
        assert_eq!(status(&resp), 503);
        assert_eq!(body(&resp)["code"], "ServiceUnavailableException");
    }

    #[tokio::test]
    async fn corrupt_stored_head_definition_is_503_not_a_silent_accept() {
        let mut corrupt = revision_item(1, "op-1", &"d".repeat(64));
        corrupt.insert("definition".to_string(), s("{not json"));
        let store = store_with(&[get_rule_returning(Some(corrupt))]);
        let (payload, principal) = event_with(add_field_body());
        let resp = invoke(&store, payload, &principal).await;
        assert_eq!(status(&resp), 503);
    }
}
