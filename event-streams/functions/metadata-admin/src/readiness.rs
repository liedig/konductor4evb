//! Read-only readiness validation of a saved revision in a selected
//! environment: the revision must recompile exactly under the current
//! compiler, trusted context must be available, and the platform's own
//! generated values must satisfy the revision's closed contract within the
//! event size budget. Nothing is persisted or activated; activation
//! re-runs authoritative checks rather than trusting these results.

use lambda_runtime::Error;
use serde::Serialize;
use serde_json::{Map, Value};

use konductor_common::api;
use konductor_common::envelope::PUT_EVENTS_ENTRY_MAX_BYTES;
use konductor_common::error::ControlPlaneError;
use konductor_common::metadata::definition::{MetadataDefinition, ValidatedMetadataDefinition};
use konductor_common::metadata::generation::{generate_extensions, PlatformUuidSource};
use konductor_common::metadata::revisions::MetadataStore;
use konductor_common::metadata::schema::{
    compile_metadata_schema, METADATA_SCHEMA_COMPILER_FORMAT,
};
use konductor_common::metadata::selector::MetadataRevision;

use crate::context::AdminConfig;
use crate::handler::store_failure;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadinessRequest {
    revision: u32,
    environment: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadinessResponse {
    revision: u32,
    environment: String,
    definition_digest: String,
    compiler_format: u32,
    checks: ChecksReport,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChecksReport {
    definition_recompiles_exactly: bool,
    generated_values_satisfy_contract: bool,
    example_metadata_bytes: usize,
    size_budget_bytes: usize,
}

pub async fn readiness(
    store: &MetadataStore,
    config: &AdminConfig,
    payload: &Value,
) -> Result<Value, Error> {
    let request: ReadinessRequest = match api::parse_body(payload) {
        Ok(r) => r,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    if !config.is_configured_environment(&request.environment) {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "environment is not one of the configured deployment environments",
        )));
    }

    let record = match store.get_revision(request.revision).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::MetadataRevisionNotFound {
                    revision: request.revision,
                },
            ))
        }
        Err(e) => return Ok(store_failure("read revision for readiness", e)),
    };
    if record.compiler_format != METADATA_SCHEMA_COMPILER_FORMAT {
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::MetadataConfigurationUnavailable {
                reason: format!(
                    "revision {} was compiled by compiler format {} but this deployment uses format {}",
                    record.revision, record.compiler_format, METADATA_SCHEMA_COMPILER_FORMAT
                ),
            },
        ));
    }

    let definition = match parse_stored_definition(&record.definition) {
        Ok(d) => d,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    let revision = match MetadataRevision::new(record.revision) {
        Ok(r) => r,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    let compiled = match compile_metadata_schema(revision, &definition) {
        Ok(c) => c,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    // An immutable revision must recompile to exactly the document it was
    // stored with; drift means a compiler change would silently reinterpret
    // saved configuration.
    if compiled.document_string() != record.metadata_schema {
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::MetadataConfigurationUnavailable {
                reason: format!(
                    "revision {} no longer recompiles to its stored schema",
                    record.revision
                ),
            },
        ));
    }

    let context = match config.generation_context(&request.environment) {
        Ok(c) => c,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    let extensions = match generate_extensions(&definition, &context, &PlatformUuidSource) {
        Ok(e) => e,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    let example = synthetic_metadata_block(record.revision, extensions);
    if let Err(reason) = compiled.validate_block(&Value::Object(example.clone())) {
        return Ok(api::from_control_plane_error(&api::validation_error(
            format!("the saved revision does not accept the platform's generated values: {reason}"),
        )));
    }
    let example_bytes = serde_json::to_vec(&example)
        .map_err(|e| ControlPlaneError::MetadataConfigurationUnavailable {
            reason: e.to_string(),
        })?
        .len();
    if example_bytes >= PUT_EVENTS_ENTRY_MAX_BYTES {
        return Ok(api::from_control_plane_error(&api::validation_error(
            "the generated metadata block alone exceeds the event size budget",
        )));
    }

    Ok(api::success_response(
        200,
        &ReadinessResponse {
            revision: record.revision,
            environment: request.environment,
            definition_digest: record.definition_digest,
            compiler_format: record.compiler_format,
            checks: ChecksReport {
                definition_recompiles_exactly: true,
                generated_values_satisfy_contract: true,
                example_metadata_bytes: example_bytes,
                size_budget_bytes: PUT_EVENTS_ENTRY_MAX_BYTES,
            },
        },
    ))
}

/// The full metadata block the platform would stamp: the four protected
/// built-ins plus the revision's configured extension values. Shared by
/// preview (as the synthetic example) and readiness (as the size-budget
/// and contract probe).
pub fn synthetic_metadata_block(
    revision: u32,
    extensions: Map<String, Value>,
) -> Map<String, Value> {
    let mut block = Map::new();
    block.insert("schemaVersion".to_string(), Value::from(1));
    block.insert(
        "idempotencyKey".to_string(),
        Value::from("00000000-0000-4000-8000-000000000000"),
    );
    block.insert(
        "publishedAt".to_string(),
        Value::from("2026-01-01T00:00:00.000Z"),
    );
    block.insert("metadataRevision".to_string(), Value::from(revision));
    for (name, value) in extensions {
        block.insert(name, value);
    }
    block
}

fn parse_stored_definition(
    canonical: &str,
) -> Result<ValidatedMetadataDefinition, ControlPlaneError> {
    let definition: MetadataDefinition = serde_json::from_str(canonical).map_err(|_| {
        ControlPlaneError::MetadataConfigurationUnavailable {
            reason: "the stored revision definition is not parseable".to_string(),
        }
    })?;
    ValidatedMetadataDefinition::validate(definition).map_err(|_| {
        ControlPlaneError::MetadataConfigurationUnavailable {
            reason: "the stored revision definition is no longer valid".to_string(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use konductor_common::dynamodb::{n, s};
    use konductor_common::metadata::definition::MetadataDefinition;
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

    async fn invoke(store: &MetadataStore, payload: Value) -> Value {
        readiness(store, &config(), &payload).await.unwrap()
    }

    fn status(resp: &Value) -> u16 {
        resp["statusCode"].as_u64().unwrap() as u16
    }

    fn body(resp: &Value) -> Value {
        serde_json::from_str(resp["body"].as_str().unwrap()).unwrap()
    }

    fn payload(revision: u32, environment: &str) -> Value {
        json!({
            "httpMethod": "POST",
            "resource": "/metadata/readiness",
            "requestContext": json!({"authorizer": {}}),
            "body": json!({"revision": revision, "environment": environment}).to_string(),
        })
    }

    /// A stored revision-2 record with a caller-chosen compiler format and
    /// stored schema, so drift and future-format cases are one-parameter
    /// variations of the healthy record.
    fn revision_rule(compiler_format: i64, stored_schema: &str) -> Rule {
        let validated = ValidatedMetadataDefinition::validate(
            serde_json::from_str::<MetadataDefinition>(DEFINITION).unwrap(),
        )
        .unwrap();
        let digest = validated.definition_digest().unwrap();
        let stored_schema = stored_schema.to_string();
        mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            let mut item = std::collections::HashMap::<String, AttributeValue>::new();
            item.insert("PK".to_string(), s("REVISION"));
            item.insert("SK".to_string(), s(&revision_sort_key(2)));
            item.insert("revision".to_string(), n(2));
            item.insert("predecessor".to_string(), n(1));
            item.insert("definition".to_string(), s(DEFINITION));
            item.insert("metadataSchema".to_string(), s(&stored_schema));
            item.insert("definitionDigest".to_string(), s(&digest));
            item.insert("compilerFormat".to_string(), n(compiler_format));
            item.insert("createdBy".to_string(), s("system-bootstrap"));
            item.insert("createdAt".to_string(), s("2026-01-01T00:00:00.000Z"));
            item.insert("operationId".to_string(), s("op-1"));
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .set_item(Some(item))
                .build()
        })
    }

    fn current_schema_document() -> String {
        let validated = ValidatedMetadataDefinition::validate(
            serde_json::from_str::<MetadataDefinition>(DEFINITION).unwrap(),
        )
        .unwrap();
        compile_metadata_schema(MetadataRevision::new(2).unwrap(), &validated)
            .unwrap()
            .document_string()
    }

    #[tokio::test]
    async fn ready_revision_reports_checks_without_any_write() {
        let revision = revision_rule(1, &current_schema_document());
        let store = store_with(&[revision]);
        let resp = invoke(&store, payload(2, "dev")).await;
        assert_eq!(status(&resp), 200, "{:?}", body(&resp));
        let report = body(&resp);
        assert_eq!(report["revision"], 2);
        assert_eq!(report["environment"], "dev");
        assert_eq!(report["definitionDigest"].as_str().unwrap().len(), 64);
        assert_eq!(report["compilerFormat"], 1);
        assert_eq!(report["checks"]["definitionRecompilesExactly"], true);
        assert_eq!(report["checks"]["generatedValuesSatisfyContract"], true);
        assert!(report["checks"]["exampleMetadataBytes"].as_u64().unwrap() > 0);
        assert_eq!(report["checks"]["sizeBudgetBytes"], 262_144);
    }

    #[tokio::test]
    async fn unknown_environment_is_400_before_any_read() {
        let store = store_with(&[]);
        let resp = invoke(&store, payload(2, "staging")).await;
        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn unknown_revision_is_404() {
        let missing = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build()
        });
        let store = store_with(&[missing]);
        let resp = invoke(&store, payload(9, "dev")).await;
        assert_eq!(status(&resp), 404);
        assert_eq!(body(&resp)["code"], "ResourceNotFoundException");
    }

    #[tokio::test]
    async fn future_compiler_format_is_actionable_503() {
        let revision = revision_rule(2, &current_schema_document());
        let store = store_with(&[revision]);
        let resp = invoke(&store, payload(2, "dev")).await;
        assert_eq!(status(&resp), 503);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(message.contains("compiler format 2"), "{message}");
    }

    #[tokio::test]
    async fn stored_schema_drift_is_503() {
        let revision = revision_rule(1, r#"{"type":"object"}"#);
        let store = store_with(&[revision]);
        let resp = invoke(&store, payload(2, "dev")).await;
        assert_eq!(status(&resp), 503);
        let message = body(&resp)["message"].as_str().unwrap().to_string();
        assert!(message.contains("recompile"), "{message}");
    }

    #[tokio::test]
    async fn unavailable_context_is_503() {
        let revision = revision_rule(1, &current_schema_document());
        let store = store_with(&[revision]);
        let mut config = config();
        config.organization = String::new();
        let resp = readiness(&store, &config, &payload(2, "dev"))
            .await
            .unwrap();
        assert_eq!(status(&resp), 503);
        assert_eq!(body(&resp)["code"], "ServiceUnavailableException");
    }
}
