//! Conditional baseline seeding for the metadata configuration table,
//! driven by a CloudFormation custom resource.
//!
//! Every write is conditioned on the item not existing, so CloudFormation
//! retries and stack redeployments never overwrite committed configuration.
//! `Delete` is a success no-op: bootstrap never removes configuration, so
//! stack teardown cannot destroy revisions or active pointers.

use aws_sdk_dynamodb::operation::put_item::builders::PutItemFluentBuilder;
use aws_sdk_dynamodb::operation::put_item::PutItemError;
use serde_json::Value;

use konductor_common::dynamodb::{n, s};
use konductor_common::error::ControlPlaneError;
use konductor_common::metadata::definition::{MetadataDefinition, ValidatedMetadataDefinition};
use konductor_common::metadata::schema::{
    compile_metadata_schema, CompiledMetadataSchema, METADATA_SCHEMA_COMPILER_FORMAT,
};
use konductor_common::metadata::selector::MetadataRevision;
use konductor_common::metadata::store::{
    revision_sort_key, ENVIRONMENT_PARTITION, HEAD_PARTITION, HEAD_SORT_KEY, REVISION_PARTITION,
};

const BASELINE_REVISION: u32 = 1;
const BOOTSTRAP_ACTOR: &str = "system-bootstrap";

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
}

/// The deployment-time configuration the main entrypoint reads from the
/// function environment and hands to the handler. The bootstrap is
/// organization-scoped: environments come from the stack's configured list,
/// never user text.
pub struct BootstrapConfig {
    pub table: String,
    pub environments: Vec<String>,
}

impl BootstrapConfig {
    pub fn from_env() -> Result<Self, String> {
        let table = std::env::var("METADATA_TABLE").map_err(|_| "METADATA_TABLE not set")?;
        let raw = std::env::var("ENVIRONMENTS").map_err(|_| "ENVIRONMENTS not set")?;
        let environments = raw
            .split(',')
            .map(str::trim)
            .filter(|env| !env.is_empty())
            .map(String::from)
            .collect::<Vec<_>>();
        Ok(BootstrapConfig {
            table,
            environments,
        })
    }
}

/// Handles one custom-resource invocation. `Delete` seeds nothing.
pub async fn handle(
    clients: &Clients,
    config: &BootstrapConfig,
    payload: &Value,
) -> Result<(), ControlPlaneError> {
    match payload.get("RequestType").and_then(Value::as_str) {
        Some("Delete") => Ok(()),
        _ => seed(clients, config).await,
    }
}

/// Seeds revision 1 (protected defaults), the revision head, and every
/// configured environment at legacy revision 0. Returns how many items were
/// created; items that already exist are left untouched and reported as
/// preserved.
async fn seed(clients: &Clients, config: &BootstrapConfig) -> Result<(), ControlPlaneError> {
    let definition = ValidatedMetadataDefinition::validate(MetadataDefinition::default())
        .map_err(|e| ControlPlaneError::DynamoDb(format!("baseline definition unusable: {e}")))?;
    let compiled = compile_metadata_schema(
        MetadataRevision::new(BASELINE_REVISION)
            .map_err(|e| ControlPlaneError::DynamoDb(e.to_string()))?,
        &definition,
    )
    .map_err(|e| ControlPlaneError::DynamoDb(format!("baseline schema unusable: {e}")))?;
    let digest = definition
        .definition_digest()
        .map_err(ControlPlaneError::DynamoDb)?;
    let now = chrono::Utc::now().to_rfc3339();

    let mut created = 0usize;
    let mut preserved = 0usize;
    for item in baseline_items(&definition, &compiled, &digest, &now, &config.environments) {
        if put_if_absent(
            clients
                .ddb
                .put_item()
                .table_name(&config.table)
                .set_item(Some(item)),
        )
        .await?
        {
            created += 1;
        } else {
            preserved += 1;
        }
    }
    tracing::info!(
        metric = "MetadataBootstrapSeeded",
        created,
        preserved,
        "metadata baseline bootstrap completed"
    );
    Ok(())
}

fn baseline_items(
    definition: &ValidatedMetadataDefinition,
    compiled: &CompiledMetadataSchema,
    digest: &str,
    now: &str,
    environments: &[String],
) -> Vec<std::collections::HashMap<String, aws_sdk_dynamodb::types::AttributeValue>> {
    let mut items = Vec::with_capacity(2 + environments.len());
    let definition_json = String::from_utf8(
        definition
            .canonical_bytes()
            .expect("validated definitions serialize"),
    )
    .expect("canonical JSON is UTF-8");

    let mut revision = std::collections::HashMap::new();
    revision.insert("PK".to_string(), s(REVISION_PARTITION));
    revision.insert("SK".to_string(), s(&revision_sort_key(BASELINE_REVISION)));
    revision.insert("revision".to_string(), n(BASELINE_REVISION as i64));
    revision.insert("predecessor".to_string(), n(0));
    revision.insert("definition".to_string(), s(&definition_json));
    revision.insert("metadataSchema".to_string(), s(&compiled.document_string()));
    revision.insert("definitionDigest".to_string(), s(digest));
    revision.insert(
        "compilerFormat".to_string(),
        n(METADATA_SCHEMA_COMPILER_FORMAT as i64),
    );
    revision.insert("createdBy".to_string(), s(BOOTSTRAP_ACTOR));
    revision.insert("createdAt".to_string(), s(now));
    items.push(revision);

    let mut head = std::collections::HashMap::new();
    head.insert("PK".to_string(), s(HEAD_PARTITION));
    head.insert("SK".to_string(), s(HEAD_SORT_KEY));
    head.insert("revision".to_string(), n(BASELINE_REVISION as i64));
    head.insert("definitionDigest".to_string(), s(digest));
    head.insert("updatedAt".to_string(), s(now));
    items.push(head);

    for environment in environments {
        let mut pointer = std::collections::HashMap::new();
        pointer.insert("PK".to_string(), s(ENVIRONMENT_PARTITION));
        pointer.insert("SK".to_string(), s(environment));
        pointer.insert("activeRevision".to_string(), n(0));
        pointer.insert("generation".to_string(), n(0));
        pointer.insert("activatedBy".to_string(), s(BOOTSTRAP_ACTOR));
        pointer.insert("activatedAt".to_string(), s(now));
        items.push(pointer);
    }
    items
}

/// Puts one item only when it does not exist. A conditional-check failure
/// means committed configuration is already present — preservation, not an
/// error.
async fn put_if_absent(request: PutItemFluentBuilder) -> Result<bool, ControlPlaneError> {
    match request
        .condition_expression("attribute_not_exists(PK)")
        .send()
        .await
    {
        Ok(_) => Ok(true),
        Err(error) => {
            let service_error = error.into_service_error();
            if matches!(
                service_error,
                PutItemError::ConditionalCheckFailedException(_)
            ) {
                Ok(false)
            } else {
                Err(ControlPlaneError::DynamoDb(service_error.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use serde_json::json;

    fn config() -> BootstrapConfig {
        BootstrapConfig {
            table: "metadata-test".to_string(),
            environments: vec!["dev".to_string(), "staging".to_string(), "prod".to_string()],
        }
    }

    fn clients(rules: &[&Rule]) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, rules),
        }
    }

    fn item<'a>(
        req: &'a aws_sdk_dynamodb::operation::put_item::PutItemInput,
        key: &str,
    ) -> &'a AttributeValue {
        req.item()
            .as_ref()
            .and_then(|item| item.get(key))
            .unwrap_or_else(|| panic!("item is missing '{key}'"))
    }

    fn item_str<'a>(
        req: &'a aws_sdk_dynamodb::operation::put_item::PutItemInput,
        key: &str,
    ) -> &'a str {
        item(req, key).as_s().ok().unwrap()
    }

    fn item_num(req: &aws_sdk_dynamodb::operation::put_item::PutItemInput, key: &str) -> String {
        item(req, key).as_n().ok().unwrap().clone()
    }

    #[tokio::test]
    async fn delete_requests_touch_nothing() {
        let clients = clients(&[]);
        let result = handle(&clients, &config(), &json!({"RequestType": "Delete"})).await;
        assert!(result.is_ok(), "delete must be a success no-op");
    }

    #[tokio::test]
    async fn create_seeds_revision_head_and_environment_pointers_conditionally() {
        let revision = mock!(aws_sdk_dynamodb::Client::put_item)
            .match_requests(|req| {
                req.condition_expression() == Some("attribute_not_exists(PK)")
                    && item_str(req, "PK") == "REVISION"
                    && item_str(req, "SK") == "r0000000001"
                    && item_num(req, "revision") == "1"
                    && item_num(req, "predecessor") == "0"
                    && item_str(req, "definition").contains("children")
                    && item_str(req, "definitionDigest").len() == 64
                    && item_num(req, "compilerFormat") == "1"
                    && item_str(req, "createdBy") == "system-bootstrap"
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
            });
        let head = mock!(aws_sdk_dynamodb::Client::put_item)
            .match_requests(|req| {
                item_str(req, "PK") == "HEAD"
                    && item_str(req, "SK") == "HEAD"
                    && item_num(req, "revision") == "1"
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
            });
        let dev = mock!(aws_sdk_dynamodb::Client::put_item)
            .match_requests(|req| {
                item_str(req, "PK") == "ENV"
                    && item_str(req, "SK") == "dev"
                    && item_num(req, "activeRevision") == "0"
                    && item_num(req, "generation") == "0"
            })
            .then_output(|| {
                aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
            });
        let staging = mock!(aws_sdk_dynamodb::Client::put_item)
            .match_requests(|req| item_str(req, "PK") == "ENV" && item_str(req, "SK") == "staging")
            .then_output(|| {
                aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
            });
        let prod = mock!(aws_sdk_dynamodb::Client::put_item)
            .match_requests(|req| item_str(req, "PK") == "ENV" && item_str(req, "SK") == "prod")
            .then_output(|| {
                aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
            });

        let clients = clients(&[&revision, &head, &dev, &staging, &prod]);
        handle(&clients, &config(), &json!({"RequestType": "Create"}))
            .await
            .unwrap();

        for rule in [&revision, &head, &dev, &staging, &prod] {
            assert_eq!(rule.num_calls(), 1);
        }
    }

    #[tokio::test]
    async fn existing_configuration_is_preserved_not_overwritten() {
        // Every write collides with committed state: retry-safety means
        // the bootstrap succeeds while leaving every existing item intact.
        let already_exists = mock!(aws_sdk_dynamodb::Client::put_item).then_error(|| {
            PutItemError::ConditionalCheckFailedException(
                aws_sdk_dynamodb::types::error::ConditionalCheckFailedException::builder()
                    .message("item already exists")
                    .build(),
            )
        });
        let clients = clients(&[&already_exists]);
        handle(&clients, &config(), &json!({"RequestType": "Update"}))
            .await
            .unwrap();
        assert!(already_exists.num_calls() >= 1);
    }

    #[tokio::test]
    async fn other_dynamodb_errors_are_not_swallowed() {
        let missing_table = mock!(aws_sdk_dynamodb::Client::put_item).then_error(|| {
            PutItemError::ResourceNotFoundException(
                aws_sdk_dynamodb::types::error::ResourceNotFoundException::builder()
                    .message("table does not exist")
                    .build(),
            )
        });
        let clients = clients(&[&missing_table]);
        let result = handle(&clients, &config(), &json!({"RequestType": "Create"})).await;
        assert!(
            result.is_err(),
            "only conditional-check failures are preservation"
        );
    }

    // The only test touching process env, so it cannot race the others.
    #[test]
    fn config_parses_the_comma_delimited_environment_list() {
        std::env::set_var("METADATA_TABLE", "metadata-test");
        std::env::remove_var("ENVIRONMENTS");
        assert!(
            BootstrapConfig::from_env().is_err(),
            "missing ENVIRONMENTS is a deployment error"
        );

        std::env::set_var("ENVIRONMENTS", " dev , staging , prod , ");
        let parsed = BootstrapConfig::from_env().unwrap();
        assert_eq!(parsed.table, "metadata-test");
        assert_eq!(
            parsed.environments,
            vec!["dev".to_string(), "staging".to_string(), "prod".to_string()]
        );

        std::env::remove_var("METADATA_TABLE");
        assert!(
            BootstrapConfig::from_env().is_err(),
            "missing METADATA_TABLE is a deployment error"
        );
        std::env::set_var("METADATA_TABLE", "metadata-test");
    }
}
