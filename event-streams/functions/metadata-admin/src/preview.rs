//! The non-persisting draft preview: validates a definition, derives the
//! exact metadata schema the draft would produce, and generates synthetic
//! example values from trusted context. Nothing is written, no revision is
//! created, and no publish path is reachable from here.

use lambda_runtime::Error;
use serde::Serialize;
use serde_json::Value;

use konductor_common::api;
use konductor_common::error::ControlPlaneError;
use konductor_common::metadata::definition::{
    GeneratedType, MetadataDefinition, MetadataNode, ValidatedMetadataDefinition,
    MAX_DEFINITION_BYTES, MAX_EXTENSION_NODES, MAX_OBJECT_LEVELS,
};
use konductor_common::metadata::generation::{generate_extensions, PlatformUuidSource};
use konductor_common::metadata::schema::compile_metadata_schema;
use konductor_common::metadata::selector::MetadataRevision;

use konductor_common::metadata::revisions::MetadataStore;

use crate::context::AdminConfig;
use crate::handler::store_failure;
use crate::readiness::synthetic_metadata_block;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewRequest {
    definition: MetadataDefinition,
    environment: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BoundsReport {
    extension_nodes_used: usize,
    extension_nodes_max: usize,
    object_levels_max: usize,
    definition_bytes_used: usize,
    definition_bytes_max: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeneratorView {
    path: String,
    generator: String,
    constraint: &'static str,
    source: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewResponse {
    candidate_revision: u32,
    metadata_schema: Value,
    bounds: BoundsReport,
    generators: Vec<GeneratorView>,
    example: Value,
}

pub async fn preview(
    store: &MetadataStore,
    config: &AdminConfig,
    payload: &Value,
) -> Result<Value, Error> {
    let request: PreviewRequest = match api::parse_body(payload) {
        Ok(r) => r,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    let validated = match ValidatedMetadataDefinition::validate(request.definition) {
        Ok(v) => v,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    // The example context uses a real configured environment so generated
    // `environment` values are representative; it defaults to the first
    // configured environment.
    let environment = match request.environment.as_deref() {
        None => match config.environments.first() {
            Some(env) => env.clone(),
            None => {
                return Ok(api::from_control_plane_error(
                    &ControlPlaneError::MetadataConfigurationUnavailable {
                        reason: "no environments are configured".to_string(),
                    },
                ))
            }
        },
        Some(env) => {
            if !config.is_configured_environment(env) {
                return Ok(api::from_control_plane_error(&api::validation_error(
                    "environment is not one of the configured deployment environments",
                )));
            }
            env.to_string()
        }
    };

    // The draft would become the next revision; the preview schema pins
    // that identity so administrators see the exact closed contract.
    let candidate_revision = match store.get_head().await {
        Ok(Some(head)) => head.revision + 1,
        Ok(None) => {
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::MetadataConfigurationUnavailable {
                    reason: "the metadata revision head is missing".to_string(),
                },
            ))
        }
        Err(e) => return Ok(store_failure("read head for preview", e)),
    };
    let revision = match MetadataRevision::new(candidate_revision) {
        Ok(r) => r,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    let compiled = match compile_metadata_schema(revision, &validated) {
        Ok(c) => c,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    let context = match config.generation_context(&environment) {
        Ok(c) => c,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };
    let extensions = match generate_extensions(&validated, &context, &PlatformUuidSource) {
        Ok(e) => e,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    let definition_bytes = validated
        .canonical_bytes()
        .map(|b| b.len())
        .unwrap_or(MAX_DEFINITION_BYTES);
    Ok(api::success_response(
        200,
        &PreviewResponse {
            candidate_revision,
            metadata_schema: compiled.document.clone(),
            bounds: BoundsReport {
                extension_nodes_used: validated.extension_node_count(),
                extension_nodes_max: MAX_EXTENSION_NODES,
                object_levels_max: MAX_OBJECT_LEVELS,
                definition_bytes_used: definition_bytes,
                definition_bytes_max: MAX_DEFINITION_BYTES,
            },
            generators: generator_views(validated.children(), String::new()),
            example: Value::Object(synthetic_metadata_block(candidate_revision, extensions)),
        },
    ))
}

/// Flattens the definition tree into documented generator entries with
/// dotted paths. Constraints and sources are derived from the generator,
/// never authored, so the editor can only show what the platform enforces.
fn generator_views(children: &[MetadataNode], prefix: String) -> Vec<GeneratorView> {
    let mut views = Vec::new();
    for node in children {
        let path = if prefix.is_empty() {
            node.name().to_string()
        } else {
            format!("{prefix}.{}", node.name())
        };
        match node {
            MetadataNode::Field { generator, .. } => views.push(GeneratorView {
                path,
                generator: generator.as_str().to_string(),
                constraint: constraint_of(generator.generated_type()),
                source: generator.source_description(),
            }),
            MetadataNode::Object { children, .. } => views.extend(generator_views(children, path)),
        }
    }
    views
}

fn constraint_of(generated_type: GeneratedType) -> &'static str {
    generated_type.constraint_description()
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

    async fn invoke(store: &MetadataStore, payload: Value) -> Value {
        preview(store, &config(), &payload).await.unwrap()
    }

    fn status(resp: &Value) -> u16 {
        resp["statusCode"].as_u64().unwrap() as u16
    }

    fn body(resp: &Value) -> Value {
        serde_json::from_str(resp["body"].as_str().unwrap()).unwrap()
    }

    fn head_rule(revision: u32) -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            let mut item = std::collections::HashMap::<String, AttributeValue>::new();
            item.insert("PK".to_string(), s("HEAD"));
            item.insert("SK".to_string(), s("HEAD"));
            item.insert("revision".to_string(), n(revision as i64));
            item.insert("definitionDigest".to_string(), s(&"d".repeat(64)));
            item.insert("updatedAt".to_string(), s("2026-01-01T00:00:00.000Z"));
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .set_item(Some(item))
                .build()
        })
    }

    fn preview_payload(definition: Value, environment: Option<&str>) -> Value {
        let mut body = json!({"definition": definition});
        if let Some(environment) = environment {
            body["environment"] = Value::from(environment);
        }
        json!({
            "httpMethod": "POST",
            "resource": "/metadata/preview",
            "requestContext": json!({"authorizer": {}}),
            "body": body.to_string(),
        })
    }

    #[tokio::test]
    async fn valid_draft_previews_schema_generators_bounds_and_example() {
        // Read rules only: the preview performs exactly one head read and
        // cannot write anything.
        let head = head_rule(1);
        let store = store_with(&[head]);
        let payload = preview_payload(
            json!({
                "children": [
                    {"kind": "field", "name": "traceId", "generator": "uuid-v4"},
                    {"kind": "object", "name": "origin", "children": [
                        {"kind": "field", "name": "region", "generator": "aws-region"}
                    ]}
                ]
            }),
            Some("prod"),
        );
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 200, "{:?}", body(&resp));
        let previewed = body(&resp);
        assert_eq!(previewed["candidateRevision"], 2);
        assert_eq!(previewed["bounds"]["extensionNodesUsed"], 3);
        assert_eq!(previewed["bounds"]["extensionNodesMax"], 64);
        assert_eq!(
            previewed["metadataSchema"]["properties"]["traceId"]["type"],
            "string"
        );

        let generators = previewed["generators"].as_array().unwrap().clone();
        assert_eq!(generators.len(), 2);
        assert_eq!(generators[0]["path"], "traceId");
        assert_eq!(generators[0]["generator"], "uuid-v4");
        assert_eq!(
            generators[0]["source"],
            "platform RNG, generated once per admitted event"
        );
        assert_eq!(generators[1]["path"], "origin.region");
        assert_eq!(generators[1]["generator"], "aws-region");

        let example = previewed["example"].clone();
        assert_eq!(example["metadataRevision"], 2);
        assert_eq!(example["origin"]["region"], "us-east-1");
        assert!(example["traceId"].as_str().unwrap().len() == 36);
    }

    #[tokio::test]
    async fn invalid_definition_is_400_with_field_violations() {
        let store = store_with(&[]);
        let payload = preview_payload(
            json!({"children": [
                {"kind": "field", "name": "__proto__", "generator": "uuid-v4"}
            ]}),
            None,
        );
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 400);
        let violations = body(&resp)["fieldList"].as_array().unwrap().clone();
        assert!(!violations.is_empty());
    }

    #[tokio::test]
    async fn unknown_environment_is_400_even_before_any_read() {
        let store = store_with(&[]);
        let payload = preview_payload(json!({"children": []}), Some("staging"));
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 400);
    }

    #[tokio::test]
    async fn unavailable_context_is_503() {
        let head = head_rule(1);
        let store = store_with(&[head]);
        let mut config = config();
        config.region = "  ".to_string();
        let payload = preview_payload(json!({"children": []}), None);
        let resp = preview(&store, &config, &payload).await.unwrap();
        assert_eq!(status(&resp), 503);
        assert_eq!(body(&resp)["code"], "ServiceUnavailableException");
    }

    #[tokio::test]
    async fn missing_head_is_503_and_unknown_fields_are_rejected() {
        let empty_head = mock!(aws_sdk_dynamodb::Client::get_item).then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build()
        });
        let store = store_with(&[empty_head]);
        let payload = preview_payload(json!({"children": []}), None);
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 503);

        let store = store_with(&[]);
        let mut payload = preview_payload(json!({"children": []}), None);
        let mut body: serde_json::Map<String, Value> =
            serde_json::from_str(payload["body"].as_str().unwrap()).unwrap();
        body.insert("candidateRevision".to_string(), Value::from(9));
        payload["body"] = Value::from(Value::Object(body).to_string());
        let resp = invoke(&store, payload).await;
        assert_eq!(status(&resp), 400, "injected fields must be rejected");
    }
}
