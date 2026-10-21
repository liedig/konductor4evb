//! Deployment-time registration of the public system-event contracts in the
//! platform system-schema registry, driven by a CloudFormation custom
//! resource.
//!
//! The checked-in manifest (`api/system-events/manifest.json`) declares the
//! intended contract version and canonical content hash per event type.
//! Reconciliation is conservative:
//!
//! - identical registered content at the intended version is a no-op, so
//!   CloudFormation retries converge;
//! - a candidate one version ahead must pass the existing additive-only
//!   compatibility checker before `UpdateSchema` appends it;
//! - any other registry history, or same-version content drift, fails the
//!   deployment instead of minting an unplanned contract version.
//!
//! `Delete` is a success no-op: bootstrap never removes registered
//! contracts, so stack teardown cannot destroy the public contract history.

use anyhow::{anyhow, bail, Context};
use aws_sdk_schemas::error::ProvideErrorMetadata;
use aws_sdk_schemas::operation::create_schema::CreateSchemaError;
use aws_sdk_schemas::operation::describe_schema::DescribeSchemaError;
use serde::Deserialize;
use serde_json::Value;

use konductor_common::promotion::{canonical_content_eq, canonical_content_hash};
use konductor_common::schema_compatibility::check_compatible;

const MANIFEST_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../api/system-events/manifest.json"
));
const STARTED_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../api/system-events/subscription-upgrade-started.schema.json"
));
const UPGRADED_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../api/system-events/subscription-upgraded.schema.json"
));

const SCHEMA_DESCRIPTION: &str = "Konductor system event contract";

pub struct Clients {
    pub schemas: aws_sdk_schemas::Client,
}

/// One intended contract: name, target version, embedded definition, and
/// the manifest's canonical content hash.
#[derive(Debug)]
pub struct SchemaContract {
    pub schema_name: String,
    pub contract_version: i64,
    pub content: &'static str,
    pub content_hash: String,
}

/// Deployment configuration: the registry to reconcile plus the embedded
/// manifest contents.
pub struct BootstrapConfig {
    pub registry_name: String,
    pub contracts: Vec<SchemaContract>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schemas: Vec<ManifestEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestEntry {
    schema_name: String,
    contract_version: i64,
    file: String,
    content_hash: String,
}

impl BootstrapConfig {
    pub fn from_env() -> Result<Self, String> {
        let registry_name = std::env::var("SYSTEM_SCHEMA_REGISTRY")
            .map_err(|_| "SYSTEM_SCHEMA_REGISTRY not set".to_string())?;
        let contracts = manifest_contracts(MANIFEST_JSON)
            .map_err(|error| format!("embedded manifest is unusable: {error:#}"))?;
        Ok(BootstrapConfig {
            registry_name,
            contracts,
        })
    }
}

/// Parses the checked-in manifest, resolves each entry to its embedded
/// definition, and verifies the declared canonical hash. A hash mismatch
/// means the definition changed without a manifest update; that drift fails
/// deployment before any registry call.
fn manifest_contracts(manifest_json: &str) -> anyhow::Result<Vec<SchemaContract>> {
    let manifest: Manifest =
        serde_json::from_str(manifest_json).context("manifest is not valid JSON")?;
    if manifest.schemas.is_empty() {
        bail!("manifest declares no schemas");
    }
    manifest
        .schemas
        .iter()
        .map(|entry| {
            let content = embedded_schema(&entry.file)
                .ok_or_else(|| anyhow!("manifest references unknown file '{}'", entry.file))?;
            let actual = canonical_content_hash(content)
                .ok_or_else(|| anyhow!("'{}' is not valid JSON", entry.file))?;
            if actual != entry.content_hash {
                bail!(
                    "content hash drift for '{}': manifest declares {}, definition hashes to {}",
                    entry.file,
                    entry.content_hash,
                    actual
                );
            }
            Ok(SchemaContract {
                schema_name: entry.schema_name.clone(),
                contract_version: entry.contract_version,
                content,
                content_hash: entry.content_hash.clone(),
            })
        })
        .collect()
}

fn embedded_schema(file: &str) -> Option<&'static str> {
    match file {
        "subscription-upgrade-started.schema.json" => Some(STARTED_SCHEMA),
        "subscription-upgraded.schema.json" => Some(UPGRADED_SCHEMA),
        _ => None,
    }
}

/// Handles one custom-resource invocation. `Delete` registers nothing and
/// removes nothing.
pub async fn handle(
    clients: &Clients,
    config: &BootstrapConfig,
    payload: &Value,
) -> anyhow::Result<()> {
    match payload.get("RequestType").and_then(Value::as_str) {
        Some("Delete") => Ok(()),
        _ => {
            for contract in &config.contracts {
                tracing::info!(
                    schema = %contract.schema_name,
                    version = contract.contract_version,
                    content_hash = %contract.content_hash,
                    "reconciling system contract"
                );
                reconcile(clients, &config.registry_name, contract).await?;
            }
            Ok(())
        }
    }
}

async fn reconcile(
    clients: &Clients,
    registry_name: &str,
    contract: &SchemaContract,
) -> anyhow::Result<()> {
    let described = clients
        .schemas
        .describe_schema()
        .registry_name(registry_name)
        .schema_name(&contract.schema_name)
        .send()
        .await;
    let output = match described {
        Ok(output) => output,
        Err(error) => {
            let service_error = error.into_service_error();
            if matches!(service_error, DescribeSchemaError::NotFoundException(_)) {
                return create_first_version(clients, registry_name, contract).await;
            }
            return Err(anyhow::Error::new(service_error))
                .context(format!("describing '{}' failed", contract.schema_name));
        }
    };
    let registered_version = output
        .schema_version()
        .and_then(|version| version.parse::<i64>().ok())
        .ok_or_else(|| {
            anyhow!(
                "registry returned no version for '{}'",
                contract.schema_name
            )
        })?;
    let registered_content = output.content().ok_or_else(|| {
        anyhow!(
            "registry returned no content for '{}'",
            contract.schema_name
        )
    })?;

    if registered_version == contract.contract_version {
        return match canonical_content_eq(registered_content, contract.content) {
            Some(true) => {
                tracing::info!(
                    schema = %contract.schema_name,
                    version = contract.contract_version,
                    "system contract already registered; no-op"
                );
                Ok(())
            }
            _ => bail!(
                "registry drift for '{}': version {} is registered with different content",
                contract.schema_name,
                registered_version
            ),
        };
    }
    if registered_version == contract.contract_version - 1 {
        return append_compatible_version(clients, registry_name, contract, registered_content)
            .await;
    }
    bail!(
        "unexpected registry history for '{}': registered v{}, manifest targets v{}",
        contract.schema_name,
        registered_version,
        contract.contract_version
    )
}

async fn create_first_version(
    clients: &Clients,
    registry_name: &str,
    contract: &SchemaContract,
) -> anyhow::Result<()> {
    if contract.contract_version != 1 {
        bail!(
            "'{}' is missing but the manifest targets v{}; first registration must be v1",
            contract.schema_name,
            contract.contract_version
        );
    }
    let result = clients
        .schemas
        .create_schema()
        .registry_name(registry_name)
        .schema_name(&contract.schema_name)
        .r#type(aws_sdk_schemas::types::Type::JsonSchemaDraft4)
        .description(SCHEMA_DESCRIPTION)
        .content(contract.content)
        .send()
        .await;
    match result {
        Ok(output) => {
            let minted = output
                .schema_version()
                .and_then(|version| version.parse::<i64>().ok());
            if minted != Some(1) {
                bail!(
                    "registry minted {:?} for '{}', expected v1",
                    minted,
                    contract.schema_name
                );
            }
            tracing::info!(schema = %contract.schema_name, "system contract registered at v1");
            Ok(())
        }
        Err(error) => {
            let service_error = error.into_service_error();
            let duplicate = matches!(&service_error,
                CreateSchemaError::BadRequestException(bad_request)
                    if bad_request.message().unwrap_or_default().contains("already exists"))
                || service_error.code() == Some("ConflictException");
            if duplicate {
                // A previous attempt landed between describe and create;
                // accept only if what landed is exactly this contract.
                verify_landed(clients, registry_name, contract, 1).await
            } else {
                Err(anyhow::Error::new(service_error))
                    .context(format!("creating '{}' failed", contract.schema_name))
            }
        }
    }
}

async fn append_compatible_version(
    clients: &Clients,
    registry_name: &str,
    contract: &SchemaContract,
    registered_content: &str,
) -> anyhow::Result<()> {
    let predecessor: Value =
        serde_json::from_str(registered_content).context("registered content is not valid JSON")?;
    let candidate: Value =
        serde_json::from_str(contract.content).context("manifest definition is not valid JSON")?;
    let check = check_compatible(&predecessor, &candidate);
    if !check.is_accepted() {
        bail!(
            "candidate v{} for '{}' is not provably compatible with v{}; refusing to register",
            contract.contract_version,
            contract.schema_name,
            contract.contract_version - 1
        );
    }
    let result = clients
        .schemas
        .update_schema()
        .registry_name(registry_name)
        .schema_name(&contract.schema_name)
        .r#type(aws_sdk_schemas::types::Type::JsonSchemaDraft4)
        .description(SCHEMA_DESCRIPTION)
        .content(contract.content)
        .send()
        .await;
    let output = match result {
        Ok(output) => output,
        Err(error) => {
            let service_error = error.into_service_error();
            if service_error.code() == Some("ConflictException") {
                return verify_landed(clients, registry_name, contract, contract.contract_version)
                    .await;
            }
            return Err(anyhow::Error::new(service_error))
                .context(format!("updating '{}' failed", contract.schema_name));
        }
    };
    let minted = output
        .schema_version()
        .and_then(|version| version.parse::<i64>().ok())
        .ok_or_else(|| {
            anyhow!(
                "registry returned no version for '{}'",
                contract.schema_name
            )
        })?;
    if minted != contract.contract_version {
        bail!(
            "registry minted v{} for '{}', expected v{}",
            minted,
            contract.schema_name,
            contract.contract_version
        );
    }
    tracing::info!(
        schema = %contract.schema_name,
        version = contract.contract_version,
        "system contract appended"
    );
    Ok(())
}

/// After a duplicate/no-change refusal: accept only when the registered
/// version is exactly the intended one with canonically equal content.
async fn verify_landed(
    clients: &Clients,
    registry_name: &str,
    contract: &SchemaContract,
    version: i64,
) -> anyhow::Result<()> {
    let output = clients
        .schemas
        .describe_schema()
        .registry_name(registry_name)
        .schema_name(&contract.schema_name)
        .send()
        .await
        .map_err(|error| anyhow::Error::new(error.into_service_error()))
        .context(format!("re-describing '{}' failed", contract.schema_name))?;
    let registered_version = output
        .schema_version()
        .and_then(|version| version.parse::<i64>().ok());
    let registered_content = output.content().unwrap_or_default();
    if registered_version == Some(version)
        && canonical_content_eq(registered_content, contract.content) == Some(true)
    {
        tracing::info!(
            schema = %contract.schema_name,
            version,
            "system contract already landed; accepting retry"
        );
        return Ok(());
    }
    bail!(
        "duplicate registration of '{}' did not land as v{} with equal content",
        contract.schema_name,
        version
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_schemas::operation::create_schema::CreateSchemaOutput;
    use aws_sdk_schemas::operation::describe_schema::DescribeSchemaOutput;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use serde_json::json;

    const REGISTRY: &str = "acme-konductor-system-schemas";
    const V1: &str = r#"{"type":"object","additionalProperties":false,"properties":{"a":{"type":"string"}},"required":["a"]}"#;
    const V2_COMPATIBLE: &str = r#"{"type":"object","additionalProperties":false,"properties":{"a":{"type":"string"},"b":{"type":"integer"}},"required":["a"]}"#;
    const V2_INCOMPATIBLE: &str = r#"{"type":"object","additionalProperties":false,"properties":{"a":{"type":"string"},"b":{"type":"integer"}},"required":["a","b"]}"#;

    fn contract(schema_name: &str, content: &'static str, version: i64) -> SchemaContract {
        SchemaContract {
            schema_name: schema_name.to_string(),
            contract_version: version,
            content,
            content_hash: canonical_content_hash(content).unwrap(),
        }
    }

    fn config(contracts: Vec<SchemaContract>) -> BootstrapConfig {
        BootstrapConfig {
            registry_name: REGISTRY.to_string(),
            contracts,
        }
    }

    fn clients(rules: &[&Rule]) -> Clients {
        Clients {
            schemas: mock_client!(aws_sdk_schemas, rules),
        }
    }

    fn sequential_clients(rules: &[&Rule]) -> Clients {
        Clients {
            schemas: mock_client!(aws_sdk_schemas, RuleMode::Sequential, rules),
        }
    }

    fn create_payload() -> Value {
        json!({"RequestType": "Create"})
    }

    fn not_found(schema_name: &'static str) -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema)
            .match_requests(move |req| {
                req.registry_name() == Some(REGISTRY) && req.schema_name() == Some(schema_name)
            })
            .then_error(|| {
                DescribeSchemaError::NotFoundException(
                    aws_sdk_schemas::types::error::NotFoundException::builder()
                        .message("not found")
                        .build(),
                )
            })
    }

    fn registered(schema_name: &'static str, version: &'static str, content: &'static str) -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema)
            .match_requests(move |req| {
                req.registry_name() == Some(REGISTRY) && req.schema_name() == Some(schema_name)
            })
            .then_output(move || {
                DescribeSchemaOutput::builder()
                    .schema_version(version)
                    .content(content)
                    .build()
            })
    }

    fn creates(schema_name: &'static str, content: &'static str, minted: &'static str) -> Rule {
        mock!(aws_sdk_schemas::Client::create_schema)
            .match_requests(move |req| {
                req.registry_name() == Some(REGISTRY)
                    && req.schema_name() == Some(schema_name)
                    && req.content() == Some(content)
                    && req.r#type() == Some(&aws_sdk_schemas::types::Type::JsonSchemaDraft4)
            })
            .then_output(move || CreateSchemaOutput::builder().schema_version(minted).build())
    }

    fn create_refused_as_duplicate() -> Rule {
        mock!(aws_sdk_schemas::Client::create_schema).then_error(|| {
            CreateSchemaError::BadRequestException(
                aws_sdk_schemas::types::error::BadRequestException::builder()
                    .message("Schema already exists")
                    .build(),
            )
        })
    }

    #[tokio::test]
    async fn delete_is_a_noop() {
        let contract = contract("TestEvent", V1, 1);
        let clients = clients(&[]);
        handle(
            &clients,
            &config(vec![contract]),
            &json!({"RequestType": "Delete"}),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn missing_schema_registers_first_version() {
        let contract = contract("TestEvent", V1, 1);
        let missing = not_found("TestEvent");
        let create = creates("TestEvent", V1, "1");
        let clients = clients(&[&missing, &create]);

        handle(&clients, &config(vec![contract]), &create_payload())
            .await
            .unwrap();
        assert_eq!(create.num_calls(), 1);
    }

    #[tokio::test]
    async fn missing_schema_rejects_manifest_above_v1() {
        let contract = contract("TestEvent", V1, 2);
        let missing = not_found("TestEvent");
        let clients = clients(&[&missing]);

        let error = handle(&clients, &config(vec![contract]), &create_payload())
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("first registration must be v1"),
            "unexpected error: {error:#}"
        );
    }

    #[tokio::test]
    async fn identical_retry_is_a_noop() {
        let contract = contract("TestEvent", V1, 1);
        // Reordered keys prove the comparison is canonical, not textual.
        let same_content: &'static str =
            Box::leak(r#"{"required":["a"],"properties":{"a":{"type":"string"}},"type":"object","additionalProperties":false}"#.to_string().into_boxed_str());
        let current = registered("TestEvent", "1", same_content);
        let clients = clients(&[&current]);

        handle(&clients, &config(vec![contract]), &create_payload())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn same_version_content_drift_fails_deployment() {
        let drifted: &'static str = Box::leak(
            r#"{"type":"object","properties":{"a":{"type":"integer"}},"required":["a"]}"#
                .to_string()
                .into_boxed_str(),
        );
        let contract = contract("TestEvent", V1, 1);
        let current = registered("TestEvent", "1", drifted);
        let clients = clients(&[&current]);

        let error = handle(&clients, &config(vec![contract]), &create_payload())
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("registry drift"),
            "unexpected error: {error:#}"
        );
    }

    #[tokio::test]
    async fn compatible_update_appends_next_version() {
        let contract = contract("TestEvent", V2_COMPATIBLE, 2);
        let current = registered("TestEvent", "1", V1);
        let update = mock!(aws_sdk_schemas::Client::update_schema)
            .match_requests(move |req| {
                req.registry_name() == Some(REGISTRY)
                    && req.schema_name() == Some("TestEvent")
                    && req.content() == Some(V2_COMPATIBLE)
            })
            .then_output(|| {
                aws_sdk_schemas::operation::update_schema::UpdateSchemaOutput::builder()
                    .schema_version("2")
                    .build()
            });
        let clients = clients(&[&current, &update]);

        handle(&clients, &config(vec![contract]), &create_payload())
            .await
            .unwrap();
        assert_eq!(update.num_calls(), 1);
    }

    #[tokio::test]
    async fn incompatible_update_fails_deployment() {
        let contract = contract("TestEvent", V2_INCOMPATIBLE, 2);
        let current = registered("TestEvent", "1", V1);
        let clients = clients(&[&current]);

        let error = handle(&clients, &config(vec![contract]), &create_payload())
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("not provably compatible"),
            "unexpected error: {error:#}"
        );
    }

    #[tokio::test]
    async fn unexpected_registry_history_fails_deployment() {
        let contract = contract("TestEvent", V2_COMPATIBLE, 2);
        let current = registered("TestEvent", "3", V2_COMPATIBLE);
        let clients = clients(&[&current]);

        let error = handle(&clients, &config(vec![contract]), &create_payload())
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("unexpected registry history"),
            "unexpected error: {error:#}"
        );
    }

    #[tokio::test]
    async fn duplicate_create_accepts_a_verified_landing() {
        let contract = contract("TestEvent", V1, 1);
        let missing = not_found("TestEvent");
        let create_refused = create_refused_as_duplicate();
        let landed = registered("TestEvent", "1", V1);
        let clients = sequential_clients(&[&missing, &create_refused, &landed]);

        handle(&clients, &config(vec![contract]), &create_payload())
            .await
            .unwrap();
        assert_eq!(create_refused.num_calls(), 1);
    }

    #[tokio::test]
    async fn duplicate_create_without_a_verified_landing_fails() {
        let contract = contract("TestEvent", V1, 1);
        let missing = not_found("TestEvent");
        let create_refused = create_refused_as_duplicate();
        let drifted = registered("TestEvent", "1", r#"{"type":"object"}"#);
        let clients = sequential_clients(&[&missing, &create_refused, &drifted]);

        let error = handle(&clients, &config(vec![contract]), &create_payload())
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("did not land"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn embedded_manifest_matches_checked_in_definitions() {
        let contracts = manifest_contracts(MANIFEST_JSON).unwrap();
        assert_eq!(contracts.len(), 2);
        assert!(contracts.iter().all(|c| c.contract_version == 1));
        let names: Vec<&str> = contracts.iter().map(|c| c.schema_name.as_str()).collect();
        assert!(names.contains(&"SubscriptionUpgradeStarted"));
        assert!(names.contains(&"SubscriptionUpgraded"));
    }

    #[test]
    fn manifest_hash_drift_fails_before_any_registry_call() {
        let drifted = MANIFEST_JSON.replacen(
            "a534233cd3ef8a3d78b7559c3f572d7b46766949f493fc391a4d5ce84c591e0f",
            "0000000000000000000000000000000000000000000000000000000000000000",
            1,
        );
        let error = manifest_contracts(&drifted).unwrap_err();
        assert!(
            format!("{error:#}").contains("content hash drift"),
            "unexpected error: {error:#}"
        );
    }
}
