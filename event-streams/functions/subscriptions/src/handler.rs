use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};
use std::env;

use crate::subscribe;
use crate::unsubscribe;
use crate::update_state;
use konductor_common::entitlement_store::EntitlementStore;
use konductor_common::error::ControlPlaneError;
use konductor_common::grant_engine::DynamoGrantEngine;
use konductor_common::grant_store::BindingStore;
use konductor_common::principal::TrustedPrincipal;
use konductor_common::principal_context::sqs_attribute_map;

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub eb: aws_sdk_eventbridge::Client,
    pub sfn: aws_sdk_sfn::Client,
    pub lambda: aws_sdk_lambda::Client,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: aws_sdk_dynamodb::Client::new(config),
            eb: aws_sdk_eventbridge::Client::new(config),
            sfn: aws_sdk_sfn::Client::new(config),
            lambda: aws_sdk_lambda::Client::new(config),
        }
    }
}

pub async fn handle(clients: &Clients, event: LambdaEvent<Value>) -> Result<Value, Error> {
    let records = event
        .payload
        .get("Records")
        .and_then(|r| r.as_array())
        .context("missing Records array")?;

    let mut batch_item_failures = Vec::new();

    for record in records {
        let Some(message_id) = record.get("messageId").and_then(|v| v.as_str()) else {
            tracing::error!("skipping SQS record without messageId");
            continue;
        };

        if let Err(e) = process_record(clients, record, message_id).await {
            tracing::error!(error = %e, message_id, "record processing failed; reporting for retry");
            batch_item_failures.push(json!({ "itemIdentifier": message_id }));
        }
    }

    Ok(json!({ "batchItemFailures": batch_item_failures }))
}

async fn process_record(clients: &Clients, record: &Value, message_id: &str) -> Result<()> {
    let http_method = record
        .get("messageAttributes")
        .and_then(|a| a.get("HttpMethod"))
        .and_then(|m| m.get("stringValue"))
        .and_then(|v| v.as_str())
        .context("missing messageAttributes.HttpMethod.stringValue")?;

    let route = record
        .get("messageAttributes")
        .and_then(|a| a.get("Resource"))
        .and_then(|m| m.get("stringValue"))
        .and_then(|v| v.as_str())
        .context("missing messageAttributes.Resource.stringValue")?;

    let body_str = record
        .get("body")
        .and_then(|b| b.as_str())
        .context("missing SQS record body")?;

    let attributes = sqs_attribute_map(record);
    let principal = TrustedPrincipal::from_sqs_attributes(&attributes)
        .context("invalid trusted-principal envelope")?;

    let bindings_table = env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;
    let entitlements_table =
        env::var("ENTITLEMENTS_TABLE").context("ENTITLEMENTS_TABLE not set")?;
    let authorizer = DynamoGrantEngine::new(
        BindingStore::new(clients.ddb.clone(), bindings_table),
        EntitlementStore::new(clients.ddb.clone(), entitlements_table),
    );

    match (http_method, route) {
        ("POST", "/subscriptions") => {
            let message: subscribe::SubscribeMessage =
                serde_json::from_str(body_str).context("invalid SubscribeMessage")?;
            subscribe::handle(clients, &authorizer, &message, &principal, message_id).await
        }
        ("POST", "/subscriptions/state") => {
            let message: update_state::UpdateStateMessage =
                serde_json::from_str(body_str).context("invalid UpdateStateMessage")?;
            update_state::handle(clients, &authorizer, &message, &principal).await
        }
        ("DELETE", "/subscriptions/{subscriptionId}") => {
            let message: unsubscribe::UnsubscribeMessage =
                serde_json::from_str(body_str).context("invalid UnsubscribeMessage")?;
            unsubscribe::handle(clients, &authorizer, &message, &principal).await
        }
        _ => Err(ControlPlaneError::ValidationException {
            message: format!("unrecognised subscription command: {http_method} {route}"),
            reason: konductor_common::error::ValidationExceptionReason::UnknownOperation,
            field_list: None,
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::operation::put_item::PutItemOutput;
    use aws_sdk_dynamodb::operation::query::QueryOutput;
    use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_sdk_eventbridge::operation::disable_rule::DisableRuleOutput;
    use aws_sdk_eventbridge::operation::enable_rule::EnableRuleOutput;
    use aws_sdk_eventbridge::operation::put_events::PutEventsOutput;
    use aws_sdk_lambda::operation::invoke::InvokeOutput;
    use aws_sdk_lambda::primitives::Blob;
    use aws_sdk_sfn::operation::start_execution::StartExecutionOutput;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use lambda_runtime::Context as LambdaContext;

    use konductor_common::dynamodb::s;
    use konductor_common::entitlement::{Entitlement, ScopePattern};
    use konductor_common::entitlement_store::entitlement_to_item;
    use konductor_common::grant::{BindingPrincipal, ResourceRole, RoleBinding};
    use konductor_common::grant_store::binding_to_item;
    use konductor_common::streams::{EnvStream, EventStream, StreamStatus};
    use konductor_common::streams_db::stream_to_item;

    const ACCOUNT: &str = "111122223333";

    fn set_env() {
        std::env::set_var("DOMAINS_TABLE", "domains");
        std::env::set_var("EVENT_STREAMS_TABLE", "streams");
        std::env::set_var("KONDUKTOR_EVENT_BUS", "konductor-bus");
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var("BINDINGS_TABLE", "bindings");
        std::env::set_var("ENTITLEMENTS_TABLE", "entitlements");
        std::env::set_var(
            "ACTIVATE_SUBSCRIPTION_STATE_MACHINE_ARN",
            "arn:aws:states:us-east-1:123:stateMachine:activate",
        );
        std::env::set_var(
            "REMOVE_SUBSCRIPTION_STATE_MACHINE_ARN",
            "arn:aws:states:us-east-1:123:stateMachine:remove",
        );
        std::env::set_var(
            "MATERIALIZE_FUNCTION_ARN",
            "arn:aws:lambda:us-east-1:123:function:materialize",
        );
    }

    fn iam_envelope(method: &str, route: &str) -> Value {
        json!({
            "HttpMethod": { "stringValue": method },
            "Resource": { "stringValue": route },
            "PrincipalVersion": { "stringValue": "1" },
            "AuthMechanism": { "stringValue": "iam" },
            "AccountId": { "stringValue": ACCOUNT }
        })
    }

    fn oauth_admin_envelope(method: &str, route: &str) -> Value {
        json!({
            "HttpMethod": { "stringValue": method },
            "Resource": { "stringValue": route },
            "PrincipalVersion": { "stringValue": "1" },
            "AuthMechanism": { "stringValue": "oauth" },
            "PrincipalId": { "stringValue": "a1b2c3d4-1111" },
            "Issuer": { "stringValue": "https://cognito-idp.example/pool" },
            "ClientId": { "stringValue": "client123" },
            "GlobalRoles": { "stringValue": "[\"platform-admin\"]" },
            "TeamGroups": { "stringValue": "[]" }
        })
    }

    fn sqs_event(records: Vec<Value>) -> LambdaEvent<Value> {
        LambdaEvent::new(json!({ "Records": records }), LambdaContext::default())
    }

    fn record(id: &str, attrs: Value, body: Value) -> Value {
        json!({
            "messageId": id,
            "body": body.to_string(),
            "messageAttributes": attrs
        })
    }

    fn subscriber_binding_item(domain: &str) -> HashMap<String, AttributeValue> {
        let binding = RoleBinding::new(
            "rb-test-1",
            BindingPrincipal::aws_account(ACCOUNT).unwrap(),
            ResourceRole::Subscriber,
            domain,
            Utc::now(),
            "test",
        )
        .unwrap();
        binding_to_item(&binding)
    }

    fn binding_rule(domain: &str) -> Rule {
        let item = subscriber_binding_item(domain);
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.key().map(|k| k.contains_key("PK")).unwrap_or(false))
            .then_output(move || {
                GetItemOutput::builder()
                    .set_item(Some(item.clone()))
                    .build()
            })
    }

    fn no_binding_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| req.key().map(|k| k.contains_key("PK")).unwrap_or(false))
            .then_output(|| GetItemOutput::builder().build())
    }

    fn domain_item(name: &str, status: &str) -> GetItemOutput {
        GetItemOutput::builder()
            .item("domainName", s(name))
            .item("ownerAccountId", s(ACCOUNT))
            .item("status", s(status))
            .item("environments", AttributeValue::L(vec![s("dev"), s("prod")]))
            .build()
    }

    fn domain_rule(name: &'static str) -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(move |req| {
                req.key()
                    .and_then(|k| k.get("domainName"))
                    .and_then(|v| v.as_s().ok())
                    .map(|v| v == name)
                    .unwrap_or(false)
            })
            .then_output(move || domain_item(name, "ACTIVE"))
    }

    fn empty_stream_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.key()
                    .map(|k| k.contains_key("producerDomain"))
                    .unwrap_or(false)
            })
            .then_output(|| GetItemOutput::builder().build())
    }

    fn stream_item_with_env(status: StreamStatus, enabled: bool) -> HashMap<String, AttributeValue> {
        let mut environments = HashMap::new();
        environments.insert(
            "prod".to_string(),
            EnvStream {
                pinned_version: None,
                rule_name: "checkout-OrderNotification".to_string(),
                rule_arn: None,
                enabled,
                status,
                error_detail: None,
            },
        );
        let stream = EventStream {
            producer_domain: "orders".to_string(),
            consumer_domain: "checkout".to_string(),
            source: "acme-orders".to_string(),
            detail_type: "OrderNotification".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            status: StreamStatus::Active,
            error_detail: None,
            environments,
            request_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
            expires_at: None,
        };
        stream_to_item(&stream)
    }

    fn active_stream_item() -> HashMap<String, AttributeValue> {
        stream_item_with_env(StreamStatus::Active, false)
    }

    fn stream_rule(item: HashMap<String, AttributeValue>) -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(|req| {
                req.key()
                    .map(|k| k.contains_key("producerDomain"))
                    .unwrap_or(false)
            })
            .then_output(move || {
                GetItemOutput::builder()
                    .set_item(Some(item.clone()))
                    .build()
            })
    }

    fn active_stream_rule() -> Rule {
        stream_rule(active_stream_item())
    }

    fn set_env_enabled_rule(env: &'static str, enabled: bool) -> Rule {
        mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(move |req| {
                req.update_expression()
                    .is_some_and(|e| e.contains("environments.#env.enabled"))
                    && req
                        .expression_attribute_names()
                        .and_then(|names| names.get("#env"))
                        .map(String::as_str)
                        == Some(env)
                    && req
                        .expression_attribute_values()
                        .and_then(|v| v.get(":enabled"))
                        .and_then(|v| v.as_bool().ok())
                        .copied()
                        == Some(enabled)
            })
            .then_output(|| UpdateItemOutput::builder().build())
    }

    fn entitlements_rule(covering: bool) -> Rule {
        entitlements_rule_with_pattern(if covering { "*" } else { "" })
    }

    fn entitlements_rule_with_pattern(pattern: &str) -> Rule {
        let pattern = pattern.to_string();
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req| {
                req.expression_attribute_values()
                    .and_then(|v| v.get(":pk"))
                    .and_then(|v| v.as_s().ok())
                    .map(|v| v.starts_with("CONSUMER#"))
                    .unwrap_or(false)
            })
            .then_output(move || {
                if pattern.is_empty() {
                    QueryOutput::builder().build()
                } else {
                    let entitlement = Entitlement::new(
                        "checkout",
                        "orders",
                        ScopePattern::new(pattern.clone()).unwrap(),
                        Utc::now(),
                        "test",
                    )
                    .unwrap();
                    QueryOutput::builder()
                        .set_items(Some(vec![entitlement_to_item(&entitlement)]))
                        .build()
                }
            })
    }

    fn put_stream_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::put_item).then_output(|| PutItemOutput::builder().build())
    }

    fn update_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::update_item)
            .then_output(|| UpdateItemOutput::builder().build())
    }

    fn put_events_rule() -> Rule {
        mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| PutEventsOutput::builder().build())
    }

    fn start_execution_rule() -> Rule {
        mock!(aws_sdk_sfn::Client::start_execution).then_output(|| {
            StartExecutionOutput::builder()
                .execution_arn("arn:aws:states:us-east-1:123:execution:activate:x")
                .start_date(aws_sdk_sfn::primitives::DateTime::from_secs(1_700_000_000))
                .build()
                .unwrap()
        })
    }

    fn clients(ddb_rules: &[&Rule], eb_rules: &[&Rule], sfn_rules: &[&Rule]) -> Clients {
        clients_with_lambda(ddb_rules, eb_rules, sfn_rules, &[])
    }

    fn clients_with_lambda(
        ddb_rules: &[&Rule],
        eb_rules: &[&Rule],
        sfn_rules: &[&Rule],
        lambda_rules: &[&Rule],
    ) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
            eb: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, eb_rules),
            sfn: mock_client!(aws_sdk_sfn, RuleMode::MatchAny, sfn_rules),
            lambda: mock_client!(aws_sdk_lambda, RuleMode::MatchAny, lambda_rules),
        }
    }

    fn subscribe_body() -> Value {
        json!({
            "body": {
                "consumerDomain": "checkout",
                "producerDomain": "orders",
                "schemaName": "acme-orders@OrderNotification"
            }
        })
    }

    #[tokio::test]
    async fn unknown_command_is_reported_as_item_failure() {
        set_env();
        let c = clients(&[], &[&put_events_rule()], &[]);
        let rec = record(
            "msg-1",
            iam_envelope("OPTIONS", "/subscriptions"),
            json!({}),
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(
            result["batchItemFailures"],
            json!([{ "itemIdentifier": "msg-1" }])
        );
    }

    #[tokio::test]
    async fn unknown_route_is_reported_as_item_failure() {
        set_env();
        let c = clients(&[], &[&put_events_rule()], &[]);
        let rec = record(
            "msg-2",
            iam_envelope("POST", "/subscriptions/nope"),
            json!({}),
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(
            result["batchItemFailures"],
            json!([{ "itemIdentifier": "msg-2" }])
        );
    }

    #[tokio::test]
    async fn malformed_body_is_reported_as_item_failure() {
        set_env();
        let c = clients(&[], &[&put_events_rule()], &[]);
        let rec = json!({
            "messageId": "msg-bad",
            "body": "not json {",
            "messageAttributes": iam_envelope("POST", "/subscriptions")
        });

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(
            result["batchItemFailures"],
            json!([{ "itemIdentifier": "msg-bad" }])
        );
    }

    #[tokio::test]
    async fn subscribe_without_binding_is_denied_without_retry() {
        set_env();
        let c = clients(&[&no_binding_rule()], &[&put_events_rule()], &[]);
        let rec = record(
            "msg-deny",
            iam_envelope("POST", "/subscriptions"),
            subscribe_body(),
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
    }

    #[tokio::test]
    async fn subscribe_with_binding_and_entitlement_starts_workflow() {
        set_env();
        let binding = binding_rule("checkout");
        let orders = domain_rule("orders");
        let checkout = domain_rule("checkout");
        let no_stream = empty_stream_rule();
        let entitlements = entitlements_rule(true);
        let put = put_stream_rule();
        let put_events = put_events_rule();
        let start = start_execution_rule();
        let c = clients(
            &[
                &binding,
                &orders,
                &checkout,
                &no_stream,
                &entitlements,
                &put,
            ],
            &[&put_events],
            &[&start],
        );
        let rec = record(
            "msg-sub",
            iam_envelope("POST", "/subscriptions"),
            subscribe_body(),
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(start.num_calls(), 1, "activation workflow must start");
    }

    #[tokio::test]
    async fn subscribe_without_entitlement_is_denied_before_workflow() {
        set_env();
        let binding = binding_rule("checkout");
        let no_stream = empty_stream_rule();
        let entitlements = entitlements_rule(false);
        let put_events = put_events_rule();
        let start = start_execution_rule();
        let c = clients(
            &[&binding, &no_stream, &entitlements],
            &[&put_events],
            &[&start],
        );
        let rec = record(
            "msg-noent",
            iam_envelope("POST", "/subscriptions"),
            subscribe_body(),
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(start.num_calls(), 0, "uncovered subscribe must not start");
    }

    #[tokio::test]
    async fn platform_admin_subscribes_without_binding_when_entitlement_covers() {
        set_env();
        let no_binding = no_binding_rule();
        let orders = domain_rule("orders");
        let checkout = domain_rule("checkout");
        let no_stream = empty_stream_rule();
        let entitlements = entitlements_rule(true);
        let put = put_stream_rule();
        let put_events = put_events_rule();
        let start = start_execution_rule();
        let c = clients(
            &[
                &no_binding,
                &orders,
                &checkout,
                &no_stream,
                &entitlements,
                &put,
            ],
            &[&put_events],
            &[&start],
        );
        let rec = record(
            "msg-admin",
            oauth_admin_envelope("POST", "/subscriptions"),
            subscribe_body(),
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(start.num_calls(), 1);
    }

    #[tokio::test]
    async fn enable_without_entitlement_fails_deterministically() {
        set_env();
        let binding = binding_rule("checkout");
        let checkout = domain_rule("checkout");
        let stream = active_stream_rule();
        let entitlements = entitlements_rule(false);
        let put_events = put_events_rule();
        let enable = mock!(aws_sdk_eventbridge::Client::enable_rule)
            .then_output(|| EnableRuleOutput::builder().build());
        let c = clients(
            &[&binding, &checkout, &stream, &entitlements],
            &[&put_events, &enable],
            &[],
        );
        let body = json!({
            "body": {
                "subscriptionId": "checkout.orders.OrderNotification",
                "consumerDomain": "checkout",
                "environments": { "prod": { "enabled": true } }
            }
        });
        let rec = record(
            "msg-enable",
            iam_envelope("POST", "/subscriptions/state"),
            body,
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(
            enable.num_calls(),
            0,
            "uncovered enable must not flip the rule"
        );
    }

    #[tokio::test]
    async fn enable_with_binding_and_entitlement_flips_rule() {
        set_env();
        let binding = binding_rule("checkout");
        let checkout = domain_rule("checkout");
        let stream = active_stream_rule();
        let entitlements = entitlements_rule(true);
        let update = update_rule();
        let put_events = put_events_rule();
        let enable = mock!(aws_sdk_eventbridge::Client::enable_rule)
            .then_output(|| EnableRuleOutput::builder().build());
        let c = clients(
            &[&binding, &checkout, &stream, &entitlements, &update],
            &[&put_events, &enable],
            &[],
        );
        let body = json!({
            "body": {
                "subscriptionId": "checkout.orders.OrderNotification",
                "consumerDomain": "checkout",
                "environments": { "prod": { "enabled": true } }
            }
        });
        let rec = record(
            "msg-enable-ok",
            iam_envelope("POST", "/subscriptions/state"),
            body,
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(enable.num_calls(), 1);
    }

    #[tokio::test]
    async fn disable_needs_binding_only_not_entitlement() {
        set_env();
        let binding = binding_rule("checkout");
        let checkout = domain_rule("checkout");
        let stream = active_stream_rule();
        let update = update_rule();
        let put_events = put_events_rule();
        let disable = mock!(aws_sdk_eventbridge::Client::disable_rule)
            .then_output(|| DisableRuleOutput::builder().build());
        // No entitlement query rule at all: disable must never consult coverage.
        let c = clients(
            &[&binding, &checkout, &stream, &update],
            &[&put_events, &disable],
            &[],
        );
        let body = json!({
            "body": {
                "subscriptionId": "checkout.orders.OrderNotification",
                "consumerDomain": "checkout",
                "environments": { "prod": { "enabled": false } }
            }
        });
        let rec = record(
            "msg-disable",
            iam_envelope("POST", "/subscriptions/state"),
            body,
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(disable.num_calls(), 1);
    }

    #[tokio::test]
    async fn pausing_an_awaiting_env_records_the_flag_without_eventbridge() {
        set_env();
        let binding = binding_rule("checkout");
        let checkout = domain_rule("checkout");
        let stream = stream_rule(stream_item_with_env(StreamStatus::AwaitingPromotion, true));
        let update = set_env_enabled_rule("prod", false);
        let put_events = put_events_rule();
        // No enable/disable rule mocks: an awaiting env has no rule to flip.
        let c = clients(&[&binding, &checkout, &stream, &update], &[&put_events], &[]);
        let body = json!({
            "body": {
                "subscriptionId": "checkout.orders.OrderNotification",
                "consumerDomain": "checkout",
                "environments": { "prod": { "enabled": false } }
            }
        });
        let rec = record("msg-pause-awaiting", iam_envelope("POST", "/subscriptions/state"), body);

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(update.num_calls(), 1);
    }

    #[tokio::test]
    async fn resuming_an_awaiting_env_records_the_flag_without_materializing() {
        set_env();
        let binding = binding_rule("checkout");
        let checkout = domain_rule("checkout");
        let stream = stream_rule(stream_item_with_env(StreamStatus::AwaitingPromotion, false));
        let entitlements = entitlements_rule(true);
        let update = set_env_enabled_rule("prod", true);
        let put_events = put_events_rule();
        // No lambda mock: promotion-following extension, not this command,
        // materializes an awaiting env.
        let c = clients(
            &[&binding, &checkout, &stream, &entitlements, &update],
            &[&put_events],
            &[],
        );
        let body = json!({
            "body": {
                "subscriptionId": "checkout.orders.OrderNotification",
                "consumerDomain": "checkout",
                "environments": { "prod": { "enabled": true } }
            }
        });
        let rec = record("msg-resume-awaiting", iam_envelope("POST", "/subscriptions/state"), body);

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(update.num_calls(), 1);
    }

    #[tokio::test]
    async fn enabling_a_failed_env_reinvokes_materialization() {
        set_env();
        let binding = binding_rule("checkout");
        let checkout = domain_rule("checkout");
        let stream = stream_rule(stream_item_with_env(StreamStatus::Failed, false));
        let entitlements = entitlements_rule(true);
        let invoke = mock!(aws_sdk_lambda::Client::invoke)
            .match_requests(|req| {
                req.function_name()
                    == Some("arn:aws:lambda:us-east-1:123:function:materialize")
                    && req
                        .payload()
                        .map(|b| {
                            let text = String::from_utf8_lossy(b.as_ref()).into_owned();
                            text.contains("\"environment\":\"prod\"") && text.contains("\"enabled\":true")
                        })
                        .unwrap_or(false)
            })
            .then_output(|| {
                InvokeOutput::builder()
                    .payload(Blob::new(
                        serde_json::to_vec(&json!({
                            "status": "SUCCEEDED",
                            "environment": "prod",
                            "enabled": true,
                            "ruleName": "checkout-OrderNotification",
                            "pinnedVersion": 4,
                        }))
                        .unwrap(),
                    ))
                    .build()
            });
        let put_events = put_events_rule();
        // No update_item rule: the materialize function merges the healed
        // entry itself; the command must not also write it.
        let c = clients_with_lambda(
            &[&binding, &checkout, &stream, &entitlements],
            &[&put_events],
            &[],
            &[&invoke],
        );
        let body = json!({
            "body": {
                "subscriptionId": "checkout.orders.OrderNotification",
                "consumerDomain": "checkout",
                "environments": { "prod": { "enabled": true } }
            }
        });
        let rec = record("msg-heal-failed", iam_envelope("POST", "/subscriptions/state"), body);

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(invoke.num_calls(), 1);
    }

    #[tokio::test]
    async fn disabling_a_failed_env_records_the_flag_only() {
        set_env();
        let binding = binding_rule("checkout");
        let checkout = domain_rule("checkout");
        let stream = stream_rule(stream_item_with_env(StreamStatus::Failed, true));
        let update = set_env_enabled_rule("prod", false);
        let put_events = put_events_rule();
        let c = clients(&[&binding, &checkout, &stream, &update], &[&put_events], &[]);
        let body = json!({
            "body": {
                "subscriptionId": "checkout.orders.OrderNotification",
                "consumerDomain": "checkout",
                "environments": { "prod": { "enabled": false } }
            }
        });
        let rec = record("msg-disable-failed", iam_envelope("POST", "/subscriptions/state"), body);

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(update.num_calls(), 1);
    }

    #[tokio::test]
    async fn mismatched_consumer_domain_is_rejected_deterministically() {
        set_env();
        let put_events = put_events_rule();
        let c = clients(&[], &[&put_events], &[]);
        let body = json!({
            "body": {
                "subscriptionId": "checkout.orders.OrderNotification",
                "consumerDomain": "billing",
                "environments": { "prod": { "enabled": true } }
            }
        });
        let rec = record(
            "msg-mismatch",
            iam_envelope("POST", "/subscriptions/state"),
            body,
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
    }

    #[tokio::test]
    async fn unsubscribe_with_binding_starts_removal_workflow() {
        set_env();
        let binding = binding_rule("checkout");
        let stream = active_stream_rule();
        let put_events = put_events_rule();
        let start = start_execution_rule();
        let c = clients(&[&binding, &stream], &[&put_events], &[&start]);
        let body = json!({
            "body": { "subscriptionId": "checkout.orders.OrderNotification" }
        });
        let rec = record(
            "msg-unsub",
            iam_envelope("DELETE", "/subscriptions/{subscriptionId}"),
            body,
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(start.num_calls(), 1, "removal workflow must start");
    }

    #[tokio::test]
    async fn unsubscribe_without_binding_is_denied() {
        set_env();
        let no_binding = no_binding_rule();
        let put_events = put_events_rule();
        let start = start_execution_rule();
        let c = clients(&[&no_binding], &[&put_events], &[&start]);
        let body = json!({
            "body": { "subscriptionId": "checkout.orders.OrderNotification" }
        });
        let rec = record(
            "msg-unsub-deny",
            iam_envelope("DELETE", "/subscriptions/{subscriptionId}"),
            body,
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(start.num_calls(), 0);
    }

    #[tokio::test]
    async fn subscribe_with_prefix_entitlement_starts_workflow() {
        set_env();
        let binding = binding_rule("checkout");
        let orders = domain_rule("orders");
        let checkout = domain_rule("checkout");
        let no_stream = empty_stream_rule();
        let prefix = entitlements_rule_with_pattern("Order*");
        let put = put_stream_rule();
        let put_events = put_events_rule();
        let start = start_execution_rule();
        let c = clients(
            &[&binding, &orders, &checkout, &no_stream, &prefix, &put],
            &[&put_events],
            &[&start],
        );
        let rec = record(
            "msg-prefix",
            iam_envelope("POST", "/subscriptions"),
            subscribe_body(),
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(
            start.num_calls(),
            1,
            "prefix grant covers OrderNotification"
        );
    }

    #[tokio::test]
    async fn self_subscription_needs_no_entitlement() {
        set_env();
        let binding = binding_rule("orders");
        let orders = domain_rule("orders");
        let no_stream = empty_stream_rule();
        let put = put_stream_rule();
        let put_events = put_events_rule();
        let start = start_execution_rule();
        // No entitlement query rule: self-subscription must not consult coverage.
        let c = clients(
            &[&binding, &orders, &no_stream, &put],
            &[&put_events],
            &[&start],
        );
        let body = json!({
            "body": {
                "consumerDomain": "orders",
                "producerDomain": "orders",
                "schemaName": "acme-orders@OrderNotification"
            }
        });
        let rec = record("msg-self", iam_envelope("POST", "/subscriptions"), body);

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(start.num_calls(), 1);
    }

    #[tokio::test]
    async fn duplicate_subscribe_is_idempotent_and_starts_no_workflow() {
        set_env();
        let binding = binding_rule("checkout");
        let orders = domain_rule("orders");
        let checkout = domain_rule("checkout");
        let existing = active_stream_rule();
        let entitlements = entitlements_rule(true);
        let put_events = put_events_rule();
        let start = start_execution_rule();
        let c = clients(
            &[&binding, &orders, &checkout, &existing, &entitlements],
            &[&put_events],
            &[&start],
        );
        let rec = record(
            "msg-dupe",
            iam_envelope("POST", "/subscriptions"),
            subscribe_body(),
        );

        let result = handle(&c, sqs_event(vec![rec])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
        assert_eq!(
            start.num_calls(),
            0,
            "existing ACTIVE stream short-circuits before workflow start"
        );
    }
}
