use anyhow::{Context, Result};
use lambda_runtime::Error;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use konductor_common::dynamodb::{get_s, s};
use konductor_common::envelope::rule_name;
use konductor_common::streams::{EventStream, StreamStatus};
use konductor_common::streams_db::query_streams_by_schema;

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub lambda: aws_sdk_lambda::Client,
    pub eb: aws_sdk_eventbridge::Client,
    pub sfn: aws_sdk_sfn::Client,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: aws_sdk_dynamodb::Client::new(config),
            lambda: aws_sdk_lambda::Client::new(config),
            eb: aws_sdk_eventbridge::Client::new(config),
            sfn: aws_sdk_sfn::Client::new(config),
        }
    }
}

pub(crate) struct Settings {
    pub(crate) streams_table: String,
    pub(crate) domains_table: String,
    pub(crate) materialize_arn: String,
    pub(crate) control_plane_bus: String,
    pub(crate) environments: Vec<String>,
    pub(crate) repin_state_machine: String,
}

impl Settings {
    fn from_env() -> Result<Self> {
        Ok(Self {
            streams_table: std::env::var("EVENT_STREAMS_TABLE")
                .context("EVENT_STREAMS_TABLE not set")?,
            domains_table: std::env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?,
            materialize_arn: std::env::var("MATERIALIZE_FUNCTION_ARN")
                .context("MATERIALIZE_FUNCTION_ARN not set")?,
            control_plane_bus: std::env::var("KONDUKTOR_EVENT_BUS")
                .context("KONDUKTOR_EVENT_BUS not set")?,
            environments: konductor_common::environments::configured_environments()?,
            repin_state_machine: std::env::var("REPIN_STATE_MACHINE_ARN")
                .context("REPIN_STATE_MACHINE_ARN not set")?,
        })
    }
}

/// The `SchemaVersionPublished` detail emitted by `schema-admin`
/// (follow-schema-promotion design §4).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionDetail {
    pub(crate) producer_domain: String,
    /// The registry's full schema name (`{namespace}@{eventName}`).
    pub(crate) schema_name: String,
    pub(crate) environment: String,
    pub(crate) version: Option<i64>,
}

impl PromotionDetail {
    /// The authoring environment is the only one whose appends advance pins
    /// directly; other environments advance through promotions.
    pub(crate) fn is_authoring(&self, settings: &Settings) -> bool {
        settings.environments.first() == Some(&self.environment)
    }

    pub(crate) fn event_name(&self) -> Result<&str> {
        self.schema_name
            .split_once('@')
            .map(|(_, name)| name)
            .filter(|name| !name.is_empty())
            .context("schema name is not in {namespace}@{eventName} form")
    }
}

#[derive(Debug, Deserialize)]
pub struct PromotionEvent {
    detail: PromotionDetail,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaterializeOutcome {
    status: String,
    enabled: bool,
    pinned_version: Option<i64>,
}

pub async fn handle(clients: &Clients, event: Value) -> Result<Value, Error> {
    let event: PromotionEvent =
        serde_json::from_value(event).context("invalid SchemaVersionPublished event")?;
    let settings = Settings::from_env()?;
    let summary = extend(clients, &settings, &event.detail).await?;
    crate::repin::maybe_start_repin(clients, &settings, &event.detail).await?;
    Ok(summary)
}

/// Promotion-following extension: every ACTIVE subscription on the promoted
/// schema whose record lacks a materialized entry for the environment gets
/// that environment materialized through the shared per-env function. The
/// function merges the fresh entry itself, so replaying the event is a safe
/// no-op once an extension has landed.
async fn extend(clients: &Clients, settings: &Settings, detail: &PromotionDetail) -> Result<Value> {
    let streams = query_streams_by_schema(
        &clients.ddb,
        &settings.streams_table,
        &detail.producer_domain,
        &detail.schema_name,
    )
    .await?;

    let mut extended = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;
    let mut accounts: HashMap<String, Option<String>> = HashMap::new();

    for stream in &streams {
        if !eligible(stream, &detail.environment) {
            skipped += 1;
            continue;
        }
        let account =
            consumer_account(clients, settings, &stream.consumer_domain, &mut accounts).await?;
        let Some(account) = account else {
            tracing::error!(
                consumer = %stream.consumer_domain,
                "consumer domain record missing — cannot build the delivery target"
            );
            failed += 1;
            continue;
        };

        let entry = stream.environments.get(&detail.environment);
        let enabled = entry.map(|e| e.enabled).unwrap_or(true);
        let name = match entry {
            Some(e) => e.rule_name.clone(),
            None => rule_name(&stream.consumer_domain, &stream.detail_type)
                .context("recorded subscription identity cannot form a rule name")?,
        };

        let outcome =
            invoke_materialize(clients, settings, stream, detail, &account, &name, enabled).await?;
        match outcome.status.as_str() {
            "SUCCEEDED" => {
                emit_extended(clients, settings, stream, detail, &outcome).await;
                extended += 1;
            }
            "AWAITING" => {
                // The promotion event raced the registry read; the entry stays
                // awaiting and the next publish to this environment re-extends.
                tracing::warn!(
                    schema = %detail.schema_name,
                    environment = %detail.environment,
                    "schema describe missed immediately after publish — still awaiting"
                );
                skipped += 1;
            }
            _ => failed += 1,
        }
    }

    Ok(json!({
        "schemaName": detail.schema_name,
        "environment": detail.environment,
        "extended": extended,
        "failed": failed,
        "skipped": skipped,
    }))
}

/// Only ACTIVE streams extend, and only environments still absent or
/// awaiting promotion: PENDING streams belong to the running activation,
/// FAILED/REMOVED are terminal, and a materialized entry (ACTIVE, or FAILED
/// pending its heal path) is a deliberate no-op.
fn eligible(stream: &EventStream, environment: &str) -> bool {
    if stream.status != StreamStatus::Active {
        return false;
    }
    match stream.environments.get(environment) {
        None => true,
        Some(entry) => entry.status == StreamStatus::AwaitingPromotion,
    }
}

async fn consumer_account(
    clients: &Clients,
    settings: &Settings,
    consumer_domain: &str,
    cache: &mut HashMap<String, Option<String>>,
) -> Result<Option<String>> {
    if let Some(cached) = cache.get(consumer_domain) {
        return Ok(cached.clone());
    }
    let output = clients
        .ddb
        .get_item()
        .table_name(&settings.domains_table)
        .key("domainName", s(consumer_domain))
        .send()
        .await
        .context("failed to read consumer domain record")?;
    let account = output
        .item
        .as_ref()
        .and_then(|item| get_s(item, "ownerAccountId"));
    cache.insert(consumer_domain.to_string(), account.clone());
    Ok(account)
}

async fn invoke_materialize(
    clients: &Clients,
    settings: &Settings,
    stream: &EventStream,
    detail: &PromotionDetail,
    consumer_account_id: &str,
    rule: &str,
    enabled: bool,
) -> Result<MaterializeOutcome> {
    let payload = json!({
        "producerDomain": stream.producer_domain,
        "consumerDomain": stream.consumer_domain,
        "consumerAccountId": consumer_account_id,
        "source": stream.source,
        "detailType": stream.detail_type,
        "schemaName": stream.schema_name,
        "ruleName": rule,
        "environment": detail.environment,
        "enabled": enabled,
    });
    let response = clients
        .lambda
        .invoke()
        .function_name(&settings.materialize_arn)
        .payload(aws_sdk_lambda::primitives::Blob::new(serde_json::to_vec(
            &payload,
        )?))
        .send()
        .await
        .context("failed to invoke materialize-subscription-env")?;
    if let Some(error) = response.function_error() {
        // Invocation-level failure: nothing was recorded, so the event must
        // fail and retry (materialization is idempotent) rather than strand
        // the environment as awaiting.
        return Err(anyhow::anyhow!(
            "materialize-subscription-env errored: {error}"
        ));
    }
    let bytes = response.payload().map(|b| b.as_ref()).unwrap_or(b"{}");
    serde_json::from_slice(bytes).context("materialize-subscription-env returned no outcome")
}

/// The lifecycle event is observational; a lost emission is logged, never
/// retried — the committed record entry is the durable outcome, and a retry
/// of the promotion event would find the environment already materialized
/// and skip without re-emitting.
async fn emit_extended(
    clients: &Clients,
    settings: &Settings,
    stream: &EventStream,
    detail: &PromotionDetail,
    outcome: &MaterializeOutcome,
) {
    let detail_json = json!({
        "subscriptionId": stream.subscription_id().to_string(),
        "producerDomain": stream.producer_domain,
        "consumerDomain": stream.consumer_domain,
        "schemaName": stream.schema_name,
        "environment": detail.environment,
        "pinnedVersion": outcome.pinned_version,
        "enabled": outcome.enabled,
        "extendedAt": chrono::Utc::now().to_rfc3339(),
    });
    let entry = aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
        .event_bus_name(&settings.control_plane_bus)
        .source("konductor.event-streams")
        .detail_type("SubscriptionEnvironmentExtended")
        .detail(detail_json.to_string())
        .build();
    if let Err(error) = clients.eb.put_events().entries(entry).send().await {
        tracing::error!(error = %error, schema = %stream.schema_name, "SubscriptionEnvironmentExtended emission failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::operation::query::QueryOutput;
    use aws_sdk_eventbridge::operation::put_events::PutEventsOutput;
    use aws_sdk_lambda::operation::invoke::{InvokeError, InvokeOutput};
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use konductor_common::streams::EnvStream;
    use konductor_common::streams_db::stream_to_item;

    const ACCOUNT: &str = "444455556666";

    fn set_env() {
        std::env::set_var("EVENT_STREAMS_TABLE", "streams");
        std::env::set_var("DOMAINS_TABLE", "domains");
        std::env::set_var(
            "MATERIALIZE_FUNCTION_ARN",
            "arn:aws:lambda:us-east-1:111122223333:function:konductor-materialize-subscription-env",
        );
        std::env::set_var("KONDUKTOR_EVENT_BUS", "konductor-bus");
        std::env::set_var("ENVIRONMENTS", "dev,staging,prod");
        std::env::set_var(
            "REPIN_STATE_MACHINE_ARN",
            "arn:aws:states:us-east-1:111122223333:stateMachine:konductor-repin-schema-env",
        );
    }

    fn clients(
        ddb_rules: &[&Rule],
        lambda_rules: &[&Rule],
        eb_rules: &[&Rule],
        sfn_rules: &[&Rule],
    ) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
            lambda: mock_client!(aws_sdk_lambda, RuleMode::MatchAny, lambda_rules),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, eb_rules),
            sfn: mock_client!(aws_sdk_sfn, RuleMode::MatchAny, sfn_rules),
        }
    }

    fn repin_start_rule() -> Rule {
        mock!(aws_sdk_sfn::Client::start_execution)
            .match_requests(|req| {
                if req.name() != Some("repin-orders-OrderNotification-dev-v7") {
                    return false;
                }
                let input: Value =
                    serde_json::from_str(req.input().unwrap()).unwrap();
                input["domain"] == "orders"
                    && input["schemaName"] == "OrderNotification"
                    && input["environment"] == "dev"
                    && input["version"] == 7
            })
            .then_output(|| {
                aws_sdk_sfn::operation::start_execution::StartExecutionOutput::builder()
                    .execution_arn("arn:aws:states:us-east-1:111122223333:execution:konductor-repin-schema-env:repin-orders-OrderNotification-dev-v7")
                    .start_date(aws_sdk_sfn::primitives::DateTime::from_secs(0))
                    .build()
                    .unwrap()
            })
    }

    fn env_entry(status: StreamStatus, enabled: bool) -> EnvStream {
        EnvStream {
            pinned_version: (status == StreamStatus::Active).then_some(3),
            rule_name: "checkout-OrderNotification".to_string(),
            rule_arn: None,
            enabled,
            status,
            error_detail: None,
        }
    }

    fn stream(status: StreamStatus, envs: Vec<(&str, StreamStatus, bool)>) -> EventStream {
        EventStream {
            producer_domain: "orders".to_string(),
            consumer_domain: "checkout".to_string(),
            source: "acme.orders".to_string(),
            detail_type: "OrderNotification".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            status,
            error_detail: None,
            environments: envs
                .into_iter()
                .map(|(name, status, enabled)| (name.to_string(), env_entry(status, enabled)))
                .collect(),
            upgrade_events: None,
            request_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            removed_at: None,
            expires_at: None,
        }
    }

    fn promotion_event(environment: &str) -> Value {
        json!({
            "version": "0",
            "id": "evt-1",
            "source": "konductor.schema-management",
            "detail-type": "SchemaVersionPublished",
            "detail": {
                "producerDomain": "orders",
                "schemaName": "acme-orders@OrderNotification",
                "environment": environment,
                "version": 7,
                "publishedAt": "2026-09-08T10:00:00Z"
            }
        })
    }

    fn streams_query_rule(streams: Vec<EventStream>) -> Rule {
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req| req.table_name() == Some("streams"))
            .then_output(move || {
                QueryOutput::builder()
                    .set_items(Some(streams.iter().map(stream_to_item).collect()))
                    .build()
            })
    }

    fn domain_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.table_name() == Some("domains"))
            .then_output(|| {
                GetItemOutput::builder()
                    .item("domainName", s("checkout"))
                    .item("ownerAccountId", s(ACCOUNT))
                    .build()
            })
    }

    fn invoke_rule(
        expected_environment: &'static str,
        expected_enabled: bool,
        outcome: Value,
    ) -> Rule {
        mock!(aws_sdk_lambda::Client::invoke)
            .match_requests(move |req| {
                let payload: Value =
                    serde_json::from_slice(req.payload().unwrap().as_ref()).unwrap();
                payload["environment"] == expected_environment
                    && payload["enabled"] == expected_enabled
            })
            .then_output(move || {
                InvokeOutput::builder()
                    .payload(aws_sdk_lambda::primitives::Blob::new(
                        serde_json::to_vec(&outcome).unwrap(),
                    ))
                    .build()
            })
    }

    fn succeeded_outcome(enabled: bool) -> Value {
        json!({
            "status": "SUCCEEDED",
            "environment": "staging",
            "enabled": enabled,
            "ruleName": "checkout-OrderNotification",
            "pinnedVersion": 7,
            "ruleArn": "arn:aws:events:us-east-1:111122223333:rule/acme-staging-orders-event-bus/checkout-OrderNotification"
        })
    }

    fn extended_event_rule(environment: &'static str, version: i64, enabled: bool) -> Rule {
        mock!(aws_sdk_eventbridge::Client::put_events)
            .match_requests(move |req| {
                let entry = req.entries().first().unwrap();
                if entry.detail_type() != Some("SubscriptionEnvironmentExtended") {
                    return false;
                }
                let detail: Value = serde_json::from_str(entry.detail().unwrap()).unwrap();
                detail["subscriptionId"] == "checkout.orders.OrderNotification"
                    && detail["producerDomain"] == "orders"
                    && detail["environment"] == environment
                    && detail["pinnedVersion"] == version
                    && detail["enabled"] == enabled
            })
            .then_output(|| PutEventsOutput::builder().failed_entry_count(0).build())
    }

    #[tokio::test]
    async fn promotion_extends_awaiting_env_and_emits_event() {
        set_env();
        let active = stream(
            StreamStatus::Active,
            vec![
                ("dev", StreamStatus::Active, true),
                ("staging", StreamStatus::AwaitingPromotion, true),
            ],
        );
        let query = streams_query_rule(vec![active]);
        let domain = domain_rule();
        let invoke = invoke_rule("staging", true, succeeded_outcome(true));
        let published = extended_event_rule("staging", 7, true);
        let clients = clients(&[&query, &domain], &[&invoke], &[&published], &[]);

        let summary = handle(&clients, promotion_event("staging")).await.unwrap();

        assert_eq!(summary["extended"], 1);
        assert_eq!(invoke.num_calls(), 1);
        assert_eq!(published.num_calls(), 1);
    }

    #[tokio::test]
    async fn absent_env_entry_extends_with_default_enabled() {
        set_env();
        let active = stream(
            StreamStatus::Active,
            vec![("dev", StreamStatus::Active, true)],
        );
        let query = streams_query_rule(vec![active]);
        let domain = domain_rule();
        let invoke = mock!(aws_sdk_lambda::Client::invoke)
            .match_requests(|req| {
                let payload: Value =
                    serde_json::from_slice(req.payload().unwrap().as_ref()).unwrap();
                payload["environment"] == "staging"
                    && payload["enabled"] == true
                    && payload["ruleName"] == "checkout-OrderNotification"
            })
            .then_output(move || {
                InvokeOutput::builder()
                    .payload(aws_sdk_lambda::primitives::Blob::new(
                        serde_json::to_vec(&succeeded_outcome(true)).unwrap(),
                    ))
                    .build()
            });
        let published = extended_event_rule("staging", 7, true);
        let clients = clients(&[&query, &domain], &[&invoke], &[&published], &[]);

        let summary = handle(&clients, promotion_event("staging")).await.unwrap();

        assert_eq!(summary["extended"], 1);
        assert_eq!(invoke.num_calls(), 1);
    }

    #[tokio::test]
    async fn skips_non_active_streams_and_materialized_envs() {
        set_env();
        let pending = stream(
            StreamStatus::Pending,
            vec![("staging", StreamStatus::AwaitingPromotion, true)],
        );
        let failed = stream(
            StreamStatus::Failed,
            vec![("staging", StreamStatus::AwaitingPromotion, true)],
        );
        let mut removed = stream(
            StreamStatus::Removed,
            vec![("staging", StreamStatus::AwaitingPromotion, true)],
        );
        removed.removed_at = Some(chrono::Utc::now());
        let materialized = stream(
            StreamStatus::Active,
            vec![("staging", StreamStatus::Active, true)],
        );
        let query = streams_query_rule(vec![pending, failed, removed, materialized]);
        let clients = clients(&[&query], &[], &[], &[]);

        let summary = handle(&clients, promotion_event("staging")).await.unwrap();

        assert_eq!(summary["extended"], 0);
        assert_eq!(summary["skipped"], 4);
    }

    #[tokio::test]
    async fn recorded_desired_disabled_is_honored_at_promotion() {
        set_env();
        let active = stream(
            StreamStatus::Active,
            vec![
                ("dev", StreamStatus::Active, true),
                ("staging", StreamStatus::AwaitingPromotion, false),
            ],
        );
        let query = streams_query_rule(vec![active]);
        let domain = domain_rule();
        let invoke = invoke_rule("staging", false, succeeded_outcome(false));
        let published = extended_event_rule("staging", 7, false);
        let clients = clients(&[&query, &domain], &[&invoke], &[&published], &[]);

        let summary = handle(&clients, promotion_event("staging")).await.unwrap();

        assert_eq!(summary["extended"], 1);
        assert_eq!(invoke.num_calls(), 1);
        assert_eq!(published.num_calls(), 1);
    }

    #[tokio::test]
    async fn materialize_step_failure_is_recorded_without_an_event() {
        set_env();
        let active = stream(
            StreamStatus::Active,
            vec![("staging", StreamStatus::AwaitingPromotion, true)],
        );
        let query = streams_query_rule(vec![active]);
        let domain = domain_rule();
        let outcome = json!({
            "status": "FAILED",
            "environment": "staging",
            "enabled": true,
            "ruleName": "checkout-OrderNotification",
            "error": { "failedStep": "CreateSubscriptionRule", "cause": "throttled" }
        });
        let invoke = invoke_rule("staging", true, outcome);
        let clients = clients(&[&query, &domain], &[&invoke], &[], &[]);

        let summary = handle(&clients, promotion_event("staging")).await.unwrap();

        assert_eq!(summary["extended"], 0);
        assert_eq!(summary["failed"], 1);
    }

    #[tokio::test]
    async fn invocation_failure_fails_the_event_for_retry() {
        set_env();
        let active = stream(
            StreamStatus::Active,
            vec![("staging", StreamStatus::AwaitingPromotion, true)],
        );
        let query = streams_query_rule(vec![active]);
        let domain = domain_rule();
        let crashed = mock!(aws_sdk_lambda::Client::invoke).then_error(|| {
            InvokeError::ServiceException(
                aws_sdk_lambda::types::error::ServiceException::builder()
                    .message("lambda unavailable")
                    .build(),
            )
        });
        let clients = clients(&[&query, &domain], &[&crashed], &[], &[]);

        let result = handle(&clients, promotion_event("staging")).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn authoring_append_starts_the_repin_workflow() {
        set_env();
        let query = streams_query_rule(vec![]);
        let repin = repin_start_rule();
        let clients = clients(&[&query], &[], &[], &[&repin]);

        let summary = handle(&clients, promotion_event("dev")).await.unwrap();

        assert_eq!(summary["extended"], 0);
        assert_eq!(repin.num_calls(), 1, "environments[0] appends advance pins");
    }

    #[tokio::test]
    async fn non_authoring_environment_does_not_start_a_repin() {
        set_env();
        let query = streams_query_rule(vec![]);
        let repin = repin_start_rule();
        let clients = clients(&[&query], &[], &[], &[&repin]);

        let summary = handle(&clients, promotion_event("staging")).await.unwrap();

        assert_eq!(summary["extended"], 0);
        assert_eq!(
            repin.num_calls(),
            0,
            "later environments advance through promotions"
        );
    }

    #[tokio::test]
    async fn an_already_running_repin_is_success() {
        set_env();
        let query = streams_query_rule(vec![]);
        let already = mock!(aws_sdk_sfn::Client::start_execution).then_error(|| {
            aws_sdk_sfn::operation::start_execution::StartExecutionError::ExecutionAlreadyExists(
                aws_sdk_sfn::types::error::ExecutionAlreadyExists::builder()
                    .message("exists")
                    .build(),
            )
        });
        let clients = clients(&[&query], &[], &[], &[&already]);

        let result = handle(&clients, promotion_event("dev")).await;

        assert!(result.is_ok(), "a replayed publish event must not fail");
    }
}
