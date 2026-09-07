use anyhow::{Context, Result};
use lambda_runtime::Error;
use serde::Deserialize;
use serde_json::Value;

use konductor_common::entitlement_store::EntitlementStore;
use konductor_common::grant_engine::covers;
use konductor_common::streams::{EnvStream, StreamStatus};
use konductor_common::streams_db::merge_env_stream;

use crate::config::Config;
use crate::outcome::{Outcome, StepError};

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub schemas: aws_sdk_schemas::Client,
    pub eventbridge: aws_sdk_eventbridge::Client,
    pub cloudwatch: aws_sdk_cloudwatch::Client,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: aws_sdk_dynamodb::Client::new(config),
            schemas: aws_sdk_schemas::Client::new(config),
            eventbridge: aws_sdk_eventbridge::Client::new(config),
            cloudwatch: aws_sdk_cloudwatch::Client::new(config),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterializeInput {
    pub(crate) producer_domain: String,
    pub(crate) consumer_domain: String,
    pub(crate) consumer_account_id: String,
    pub(crate) source: String,
    pub(crate) detail_type: String,
    pub(crate) schema_name: String,
    pub(crate) rule_name: String,
    pub(crate) environment: String,
    pub(crate) enabled: bool,
}

pub async fn handle(clients: &Clients, event: Value) -> Result<Value, Error> {
    let input: MaterializeInput =
        serde_json::from_value(event).context("invalid materialization input")?;
    let config = Config::from_env()?;
    let outcome = materialize(clients, &config, &input).await?;
    Ok(serde_json::to_value(outcome).context("failed to serialize materialization outcome")?)
}

async fn materialize(
    clients: &Clients,
    config: &Config,
    input: &MaterializeInput,
) -> Result<Outcome> {
    let registry = config.producer_registry(input);
    let pinned = match describe_latest_version(clients, &registry, &input.schema_name).await {
        Ok(version) => version,
        Err(DescribeFailure::NotFound) => {
            let entry = record_entry(input, StreamStatus::AwaitingPromotion, None, None, None);
            return finish(clients, config, input, entry, None).await;
        }
        Err(DescribeFailure::Service(cause)) => {
            let entry = failed_entry(input, "ResolveLatestSchemaVersion", &cause);
            return finish(
                clients,
                config,
                input,
                entry,
                step_error("ResolveLatestSchemaVersion", cause),
            )
            .await;
        }
    };

    if input.enabled && input.consumer_domain != input.producer_domain {
        match coverage_holds(clients, config, input).await {
            Ok(true) => {}
            Ok(false) => {
                let cause = format!(
                    "no covering entitlement for consumer {} on {}",
                    input.consumer_domain, input.schema_name
                );
                let entry = failed_entry(input, "EntitlementCoverage", &cause);
                return finish(
                    clients,
                    config,
                    input,
                    entry,
                    step_error("EntitlementCoverage", cause),
                )
                .await;
            }
            Err(err) => return Err(err),
        }
    }

    let bus = config.producer_bus(input);
    let rule_arn = match put_rule(clients, &bus, input, pinned).await {
        Ok(arn) => arn,
        Err(cause) => {
            let entry = failed_entry(input, "CreateSubscriptionRule", &cause);
            return finish(
                clients,
                config,
                input,
                entry,
                step_error("CreateSubscriptionRule", cause),
            )
            .await;
        }
    };
    if let Err(cause) = put_target(clients, config, &bus, input).await {
        let entry = failed_entry(input, "AddConsumerBusTarget", &cause);
        return finish(
            clients,
            config,
            input,
            entry,
            step_error("AddConsumerBusTarget", cause),
        )
        .await;
    }
    if let Err(cause) = put_zero_events_alarm(clients, config, &bus, input).await {
        let entry = failed_entry(input, "CreateZeroEventsAlarm", &cause);
        return finish(
            clients,
            config,
            input,
            entry,
            step_error("CreateZeroEventsAlarm", cause),
        )
        .await;
    }

    let entry = record_entry(
        input,
        StreamStatus::Active,
        Some(pinned),
        Some(rule_arn),
        None,
    );
    finish(clients, config, input, entry, None).await
}

enum DescribeFailure {
    NotFound,
    Service(String),
}

async fn describe_latest_version(
    clients: &Clients,
    registry: &str,
    schema_name: &str,
) -> std::result::Result<i64, DescribeFailure> {
    use aws_sdk_schemas::operation::describe_schema::DescribeSchemaError;
    match clients
        .schemas
        .describe_schema()
        .registry_name(registry)
        .schema_name(schema_name)
        .send()
        .await
    {
        Ok(output) => output
            .schema_version()
            .and_then(|v| v.parse::<i64>().ok())
            .ok_or_else(|| DescribeFailure::Service("unparsable schemaVersion".to_string())),
        Err(err) => {
            let not_found = err
                .as_service_error()
                .is_some_and(|e| matches!(e, DescribeSchemaError::NotFoundException(_)));
            if not_found {
                Err(DescribeFailure::NotFound)
            } else {
                Err(DescribeFailure::Service(sdk_cause(&err)))
            }
        }
    }
}

async fn coverage_holds(
    clients: &Clients,
    config: &Config,
    input: &MaterializeInput,
) -> Result<bool> {
    let store = EntitlementStore::new(clients.ddb.clone(), config.entitlements_table.clone());
    let entitlements = store
        .entitlements_for_consumer(&input.consumer_domain)
        .await
        .context("failed to query consumer entitlements")?;
    Ok(covers(
        &entitlements,
        &input.producer_domain,
        &input.detail_type,
    ))
}

async fn put_rule(
    clients: &Clients,
    bus: &str,
    input: &MaterializeInput,
    pinned: i64,
) -> std::result::Result<String, String> {
    use aws_sdk_eventbridge::types::RuleState;
    let pattern = serde_json::json!({
        "source": [input.source],
        "detail-type": [input.detail_type],
        "detail": { "metadata": { "schemaVersion": [pinned] } },
    })
    .to_string();
    let state = if input.enabled {
        RuleState::Enabled
    } else {
        RuleState::Disabled
    };
    clients
        .eventbridge
        .put_rule()
        .name(&input.rule_name)
        .event_bus_name(bus)
        .event_pattern(pattern)
        .state(state)
        .send()
        .await
        .map_err(|err| sdk_cause(&err))?
        .rule_arn()
        .map(str::to_string)
        .ok_or_else(|| "putRule returned no RuleArn".to_string())
}

async fn put_target(
    clients: &Clients,
    config: &Config,
    bus: &str,
    input: &MaterializeInput,
) -> std::result::Result<(), String> {
    use aws_sdk_eventbridge::types::{DeadLetterConfig, Target};
    let target = Target::builder()
        .id("consumer-app-bus")
        .arn(config.consumer_bus_arn(input))
        .role_arn(&config.delivery_role_arn)
        .dead_letter_config(
            DeadLetterConfig::builder()
                .arn(config.subscription_dlq_arn(input))
                .build(),
        )
        .build()
        .map_err(|err| err.to_string())?;
    clients
        .eventbridge
        .put_targets()
        .rule(&input.rule_name)
        .event_bus_name(bus)
        .targets(target)
        .send()
        .await
        .map(|_| ())
        .map_err(|err| sdk_cause(&err))
}

async fn put_zero_events_alarm(
    clients: &Clients,
    config: &Config,
    bus: &str,
    input: &MaterializeInput,
) -> std::result::Result<(), String> {
    use aws_sdk_cloudwatch::types::{ComparisonOperator, Dimension, Statistic};
    let alarm_name = format!(
        "{}-{}-{}-zero-events-alarm",
        config.organization, input.environment, input.rule_name
    );
    let description = format!(
        "No events delivered for subscription {} on {} ({}) for 24h — possible version pin mismatch",
        input.rule_name, input.producer_domain, input.environment
    );
    clients
        .cloudwatch
        .put_metric_alarm()
        .alarm_name(alarm_name)
        .alarm_description(description)
        .namespace("AWS/Events")
        .metric_name("Invocations")
        .dimensions(Dimension::builder().name("EventBusName").value(bus).build())
        .dimensions(
            Dimension::builder()
                .name("RuleName")
                .value(&input.rule_name)
                .build(),
        )
        .statistic(Statistic::Sum)
        .period(86400)
        .evaluation_periods(1)
        .threshold(1.0)
        .comparison_operator(ComparisonOperator::LessThanThreshold)
        .treat_missing_data("breaching")
        .send()
        .await
        .map(|_| ())
        .map_err(|err| sdk_cause(&err))
}

fn sdk_cause<E: std::error::Error>(err: &aws_sdk_eventbridge::error::SdkError<E>) -> String {
    err.as_service_error()
        .map(|service| service.to_string())
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| err.to_string())
}

fn record_entry(
    input: &MaterializeInput,
    status: StreamStatus,
    pinned_version: Option<i64>,
    rule_arn: Option<String>,
    error_detail: Option<String>,
) -> EnvStream {
    EnvStream {
        pinned_version,
        rule_name: input.rule_name.clone(),
        rule_arn,
        enabled: input.enabled,
        status,
        error_detail,
    }
}

fn failed_entry(input: &MaterializeInput, failed_step: &str, cause: &str) -> EnvStream {
    record_entry(
        input,
        StreamStatus::Failed,
        None,
        None,
        Some(format!("{failed_step}: {cause}")),
    )
}

fn step_error(failed_step: &'static str, cause: String) -> Option<StepError> {
    Some(StepError { failed_step, cause })
}

async fn finish(
    clients: &Clients,
    config: &Config,
    input: &MaterializeInput,
    entry: EnvStream,
    error: Option<StepError>,
) -> Result<Outcome> {
    let status = match entry.status {
        StreamStatus::Active => "SUCCEEDED",
        StreamStatus::AwaitingPromotion => "AWAITING",
        _ => "FAILED",
    };
    let outcome = Outcome {
        status,
        environment: input.environment.clone(),
        enabled: input.enabled,
        rule_name: input.rule_name.clone(),
        pinned_version: entry.pinned_version,
        rule_arn: entry.rule_arn.clone(),
        error,
    };
    merge_env_stream(
        &clients.ddb,
        &config.streams_table,
        &input.producer_domain,
        &input.schema_name,
        &input.consumer_domain,
        &input.environment,
        &entry,
    )
    .await?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use aws_sdk_cloudwatch::operation::put_metric_alarm::PutMetricAlarmOutput;
    use aws_sdk_dynamodb::operation::query::QueryOutput;
    use aws_sdk_dynamodb::operation::update_item::{UpdateItemInput, UpdateItemOutput};
    use aws_sdk_eventbridge::operation::put_rule::PutRuleOutput;
    use aws_sdk_eventbridge::operation::put_targets::PutTargetsOutput;
    use aws_sdk_eventbridge::types::RuleState;
    use aws_sdk_schemas::operation::describe_schema::{DescribeSchemaError, DescribeSchemaOutput};
    use aws_sdk_schemas::types::error::NotFoundException;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use serde_json::json;

    use konductor_common::entitlement::{Entitlement, ScopePattern};
    use konductor_common::entitlement_store::entitlement_to_item;

    const RULE_ARN: &str =
        "arn:aws:events:ap-southeast-2:999988887777:rule/acme-dev-billing-event-bus/shipping-OrderNotification";

    fn set_env() {
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var("EVENT_STREAMS_TABLE", "konductor-event-streams");
        std::env::set_var("ENTITLEMENTS_TABLE", "konductor-authorization-entitlements");
        std::env::set_var(
            "SUBSCRIPTION_DELIVERY_ROLE_ARN",
            "arn:aws:iam::999988887777:role/konductor-subscription-delivery",
        );
        std::env::set_var("PLATFORM_ACCOUNT_ID", "999988887777");
        std::env::set_var("AWS_REGION", "ap-southeast-2");
    }

    fn input(consumer: &str, producer: &str, enabled: bool) -> Value {
        json!({
            "producerDomain": producer,
            "consumerDomain": consumer,
            "consumerAccountId": "111122223333",
            "source": "acme-billing",
            "detailType": "OrderNotification",
            "schemaName": "acme-billing@OrderNotification",
            "ruleName": "shipping-OrderNotification",
            "environment": "dev",
            "enabled": enabled,
        })
    }

    fn covering_entitlement_rule() -> Rule {
        let entitlement = Entitlement::new(
            "shipping",
            "billing",
            ScopePattern::new("OrderNotification").unwrap(),
            Utc::now(),
            "test",
        )
        .unwrap();
        let item = entitlement_to_item(&entitlement);
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req| req.table_name() == Some("konductor-authorization-entitlements"))
            .then_output(move || {
                QueryOutput::builder()
                    .set_items(Some(vec![item.clone()]))
                    .build()
            })
    }

    fn no_entitlements_rule() -> Rule {
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(|req| req.table_name() == Some("konductor-authorization-entitlements"))
            .then_output(|| QueryOutput::builder().set_items(Some(vec![])).build())
    }

    fn describe_rule(version: i64) -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema)
            .match_requests(|req| {
                req.registry_name() == Some("acme-dev-billing-schema-registry")
                    && req.schema_name() == Some("acme-billing@OrderNotification")
            })
            .then_output(move || {
                DescribeSchemaOutput::builder()
                    .schema_version(version.to_string())
                    .build()
            })
    }

    fn describe_not_found_rule() -> Rule {
        mock!(aws_sdk_schemas::Client::describe_schema).then_error(|| {
            DescribeSchemaError::NotFoundException(
                NotFoundException::builder()
                    .message("Schema 'acme-billing@OrderNotification' does not exist")
                    .build(),
            )
        })
    }

    fn put_rule_rule(expected_state: RuleState, version: i64) -> Rule {
        mock!(aws_sdk_eventbridge::Client::put_rule)
            .match_requests(move |req| {
                let pinned = format!("\"schemaVersion\":[{version}]");
                req.name() == Some("shipping-OrderNotification")
                    && req.event_bus_name() == Some("acme-dev-billing-event-bus")
                    && req.state() == Some(&expected_state)
                    && req.event_pattern().is_some_and(|p| p.contains(&pinned))
            })
            .then_output(|| PutRuleOutput::builder().rule_arn(RULE_ARN).build())
    }

    fn put_targets_rule(consumer: &str) -> Rule {
        let consumer_bus = format!(
            "arn:aws:events:ap-southeast-2:111122223333:event-bus/acme-dev-{consumer}-event-bus"
        );
        mock!(aws_sdk_eventbridge::Client::put_targets)
            .match_requests(move |req| {
                let target = &req.targets()[0];
                req.rule() == Some("shipping-OrderNotification")
                    && req.targets().len() == 1
                    && target.id() == "consumer-app-bus"
                    && target.arn() == consumer_bus
                    && target.role_arn().is_some_and(|r| r.contains("delivery"))
                    && target
                        .dead_letter_config()
                        .and_then(|d| d.arn())
                        == Some("arn:aws:sqs:ap-southeast-2:999988887777:acme-dev-billing-subscription-dlq")
            })
            .then_output(|| PutTargetsOutput::builder().failed_entry_count(0).build())
    }

    fn alarm_rule() -> Rule {
        mock!(aws_sdk_cloudwatch::Client::put_metric_alarm)
            .match_requests(|req| {
                req.alarm_name() == Some("acme-dev-shipping-OrderNotification-zero-events-alarm")
                    && req.metric_name() == Some("Invocations")
                    && req.period() == Some(86400)
                    && req.treat_missing_data() == Some("breaching")
            })
            .then_output(|| PutMetricAlarmOutput::builder().build())
    }

    fn entry_field(req: &UpdateItemInput, field: &str) -> Option<String> {
        req.expression_attribute_values()?
            .get(":entry")?
            .as_m()
            .ok()?
            .get(field)?
            .as_s()
            .ok()
            .cloned()
    }

    fn merge_rule(expected_status: &'static str) -> Rule {
        mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(move |req| {
                req.table_name() == Some("konductor-event-streams")
                    && req.update_expression()
                        == Some("SET environments.#env = :entry, updatedAt = :now")
                    && entry_field(req, "status").as_deref() == Some(expected_status)
            })
            .then_output(|| UpdateItemOutput::builder().build())
    }

    fn clients(
        ddb_rules: &[&Rule],
        schemas_rules: &[&Rule],
        eb_rules: &[&Rule],
        cw_rules: &[&Rule],
    ) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
            schemas: mock_client!(aws_sdk_schemas, RuleMode::MatchAny, schemas_rules),
            eventbridge: mock_client!(aws_sdk_eventbridge, RuleMode::MatchAny, eb_rules),
            cloudwatch: mock_client!(aws_sdk_cloudwatch, RuleMode::MatchAny, cw_rules),
        }
    }

    #[tokio::test]
    async fn materializes_an_environment_end_to_end() {
        set_env();
        let entitlements = covering_entitlement_rule();
        let describe = describe_rule(3);
        let rule = put_rule_rule(RuleState::Enabled, 3);
        let targets = put_targets_rule("shipping");
        let alarm = alarm_rule();
        let merge = merge_rule("ACTIVE");
        let clients = clients(
            &[&entitlements, &merge],
            &[&describe],
            &[&rule, &targets],
            &[&alarm],
        );

        let outcome = handle(&clients, input("shipping", "billing", true))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "SUCCEEDED");
        assert_eq!(outcome["environment"], "dev");
        assert_eq!(outcome["pinnedVersion"], 3);
        assert_eq!(outcome["ruleArn"], RULE_ARN);
        assert_eq!(outcome["enabled"], true);
        assert!(outcome.get("error").is_none());
        assert_eq!(rule.num_calls(), 1);
        assert_eq!(targets.num_calls(), 1);
        assert_eq!(alarm.num_calls(), 1);
        assert_eq!(merge.num_calls(), 1);
    }

    #[tokio::test]
    async fn schema_absent_marks_the_environment_awaiting_promotion() {
        set_env();
        let entitlements = covering_entitlement_rule();
        let describe = describe_not_found_rule();
        let rule = put_rule_rule(RuleState::Enabled, 3);
        let alarm = alarm_rule();
        let merge = merge_rule("AWAITING_PROMOTION");
        let clients = clients(&[&entitlements, &merge], &[&describe], &[&rule], &[&alarm]);

        let outcome = handle(&clients, input("shipping", "billing", true))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "AWAITING");
        assert!(outcome.get("pinnedVersion").is_none());
        assert!(outcome.get("error").is_none());
        assert_eq!(rule.num_calls(), 0);
        assert_eq!(alarm.num_calls(), 0);
        assert_eq!(merge.num_calls(), 1);
    }

    #[tokio::test]
    async fn lapsed_entitlement_fails_closed_without_touching_eventbridge() {
        set_env();
        let entitlements = no_entitlements_rule();
        let describe = describe_rule(3);
        let rule = put_rule_rule(RuleState::Enabled, 3);
        let merge = merge_rule("FAILED");
        let clients = clients(&[&entitlements, &merge], &[&describe], &[&rule], &[]);

        let outcome = handle(&clients, input("shipping", "billing", true))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "FAILED");
        assert_eq!(outcome["error"]["failedStep"], "EntitlementCoverage");
        assert!(outcome["error"]["cause"]
            .as_str()
            .unwrap()
            .contains("no covering entitlement"));
        assert_eq!(rule.num_calls(), 0);
        assert_eq!(merge.num_calls(), 1);
    }

    #[tokio::test]
    async fn disabled_environment_skips_coverage_and_creates_a_disabled_rule() {
        set_env();
        let describe = describe_rule(3);
        let rule = put_rule_rule(RuleState::Disabled, 3);
        let targets = put_targets_rule("shipping");
        let alarm = alarm_rule();
        let merge = merge_rule("ACTIVE");
        let clients = clients(&[&merge], &[&describe], &[&rule, &targets], &[&alarm]);

        let outcome = handle(&clients, input("shipping", "billing", false))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "SUCCEEDED");
        assert_eq!(outcome["enabled"], false);
        assert_eq!(rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn self_subscription_skips_the_coverage_check() {
        set_env();
        let describe = describe_rule(3);
        let rule = put_rule_rule(RuleState::Enabled, 3);
        let targets = put_targets_rule("billing");
        let alarm = alarm_rule();
        let merge = merge_rule("ACTIVE");
        let clients = clients(&[&merge], &[&describe], &[&rule, &targets], &[&alarm]);

        let outcome = handle(&clients, input("billing", "billing", true))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "SUCCEEDED");
    }

    #[tokio::test]
    async fn repeated_invocation_reproduces_the_same_outcome() {
        set_env();
        let entitlements = covering_entitlement_rule();
        let describe = describe_rule(3);
        let rule = put_rule_rule(RuleState::Enabled, 3);
        let targets = put_targets_rule("shipping");
        let alarm = alarm_rule();
        let merge = merge_rule("ACTIVE");
        let clients = clients(
            &[&entitlements, &merge],
            &[&describe],
            &[&rule, &targets],
            &[&alarm],
        );

        let first = handle(&clients, input("shipping", "billing", true))
            .await
            .unwrap();
        let second = handle(&clients, input("shipping", "billing", true))
            .await
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(rule.num_calls(), 2);
        assert_eq!(merge.num_calls(), 2);
    }

    #[tokio::test]
    async fn rule_creation_failure_records_the_failed_step() {
        set_env();
        let entitlements = covering_entitlement_rule();
        let describe = describe_rule(3);
        let rule = mock!(aws_sdk_eventbridge::Client::put_rule).then_error(|| {
            aws_sdk_eventbridge::operation::put_rule::PutRuleError::InternalException(
                aws_sdk_eventbridge::types::error::InternalException::builder()
                    .message("boom")
                    .build(),
            )
        });
        let targets = put_targets_rule("shipping");
        let merge = merge_rule("FAILED");
        let clients = clients(
            &[&entitlements, &merge],
            &[&describe],
            &[&rule, &targets],
            &[],
        );

        let outcome = handle(&clients, input("shipping", "billing", true))
            .await
            .unwrap();

        assert_eq!(outcome["status"], "FAILED");
        assert_eq!(outcome["error"]["failedStep"], "CreateSubscriptionRule");
        assert_eq!(targets.num_calls(), 0);
        assert_eq!(merge.num_calls(), 1);
    }

    #[tokio::test]
    async fn malformed_input_is_an_error() {
        set_env();
        let clients = clients(&[], &[], &[], &[]);

        let result = handle(&clients, json!({"nope": true})).await;

        assert!(result.is_err());
    }
}
