use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use crate::subscribe;
use crate::unsubscribe;
use crate::update_state;
use nexus_common::error::ControlPlaneError;
use nexus_common::principal::TrustedPrincipal;
use nexus_common::principal_context::sqs_attribute_map;

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub eb: aws_sdk_eventbridge::Client,
    pub sfn: aws_sdk_sfn::Client,
}

impl Clients {
    pub fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: aws_sdk_dynamodb::Client::new(config),
            eb: aws_sdk_eventbridge::Client::new(config),
            sfn: aws_sdk_sfn::Client::new(config),
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

    let body_str = record
        .get("body")
        .and_then(|b| b.as_str())
        .context("missing SQS record body")?;

    let attributes = sqs_attribute_map(record);
    let principal = TrustedPrincipal::from_sqs_attributes(&attributes)
        .context("invalid trusted-principal envelope")?;

    match http_method {
        "POST" => {
            let message: subscribe::SubscribeMessage =
                serde_json::from_str(body_str).context("invalid SubscribeMessage")?;
            subscribe::handle(clients, &message, &principal, message_id).await
        }
        "PATCH" => {
            let message: update_state::UpdateStateMessage =
                serde_json::from_str(body_str).context("invalid UpdateStateMessage")?;
            update_state::handle(clients, &message, &principal).await
        }
        "DELETE" => {
            let message: unsubscribe::UnsubscribeMessage =
                serde_json::from_str(body_str).context("invalid UnsubscribeMessage")?;
            unsubscribe::handle(clients, &message, &principal).await
        }
        other => Err(ControlPlaneError::ValidationException {
            message: format!("unrecognised HttpMethod: {other}"),
            reason: nexus_common::error::ValidationExceptionReason::UnknownOperation,
            field_list: None,
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_eventbridge::operation::put_events::PutEventsOutput;
    use aws_smithy_mocks::{mock, mock_client};
    use lambda_runtime::Context as LambdaContext;

    fn set_env() {
        std::env::set_var("DOMAINS_TABLE", "domains");
        std::env::set_var("EVENT_STREAMS_TABLE", "streams");
        std::env::set_var("NEXUS_EVENT_BUS", "nexus-bus");
        std::env::set_var("ENVIRONMENTS", "dev,staging,prod");
        std::env::set_var("ORGANIZATION", "acme");
        std::env::set_var(
            "ACTIVATE_SUBSCRIPTION_STATE_MACHINE_ARN",
            "arn:aws:states:us-east-1:123:stateMachine:activate",
        );
        std::env::set_var(
            "REMOVE_SUBSCRIPTION_STATE_MACHINE_ARN",
            "arn:aws:states:us-east-1:123:stateMachine:remove",
        );
    }

    fn outcome_only_clients() -> Clients {
        let ddb_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| GetItemOutput::builder().build());
        let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| PutEventsOutput::builder().build());
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, [&ddb_rule]),
            eb: mock_client!(aws_sdk_eventbridge, [&eb_rule]),
            sfn: mock_client!(aws_sdk_sfn, []),
        }
    }

    fn iam_envelope(method: &str) -> Value {
        json!({
            "HttpMethod": { "stringValue": method },
            "PrincipalVersion": { "stringValue": "1" },
            "AuthMechanism": { "stringValue": "iam" },
            "AccountId": { "stringValue": "111122223333" }
        })
    }

    fn sqs_event(records: Vec<Value>) -> LambdaEvent<Value> {
        LambdaEvent::new(json!({ "Records": records }), LambdaContext::default())
    }

    #[tokio::test]
    async fn unknown_http_method_is_reported_as_item_failure() {
        set_env();
        let clients = outcome_only_clients();
        let record = json!({
            "messageId": "msg-1",
            "body": "{}",
            "messageAttributes": iam_envelope("OPTIONS")
        });

        let result = handle(&clients, sqs_event(vec![record])).await.unwrap();

        assert_eq!(
            result["batchItemFailures"],
            json!([{ "itemIdentifier": "msg-1" }])
        );
    }

    #[tokio::test]
    async fn malformed_body_is_reported_as_item_failure() {
        set_env();
        let clients = outcome_only_clients();
        let record = json!({
            "messageId": "msg-bad",
            "body": "not json {",
            "messageAttributes": iam_envelope("POST")
        });

        let result = handle(&clients, sqs_event(vec![record])).await.unwrap();

        assert_eq!(
            result["batchItemFailures"],
            json!([{ "itemIdentifier": "msg-bad" }])
        );
    }

    #[tokio::test]
    async fn subscribe_to_unknown_domain_emits_failed_outcome_not_retry() {
        set_env();
        let clients = outcome_only_clients();
        let body = json!({
            "callerAccountId": "111122223333",
            "body": {
                "consumerDomain": "checkout",
                "producerDomain": "orders",
                "schemaName": "acme-orders@OrderNotification"
            }
        });
        let record = json!({
            "messageId": "msg-1",
            "body": body.to_string(),
            "messageAttributes": iam_envelope("POST")
        });

        let result = handle(&clients, sqs_event(vec![record])).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
    }

    #[tokio::test]
    async fn oauth_subscribe_is_denied_without_mutation() {
        set_env();
        let clients = outcome_only_clients();
        let body = json!({
            "callerAccountId": "",
            "body": {
                "consumerDomain": "checkout",
                "producerDomain": "orders",
                "schemaName": "acme-orders@OrderNotification"
            }
        });
        let record = json!({
            "messageId": "msg-oauth",
            "body": body.to_string(),
            "messageAttributes": {
                "HttpMethod": { "stringValue": "POST" },
                "PrincipalVersion": { "stringValue": "1" },
                "AuthMechanism": { "stringValue": "oauth" },
                "PrincipalId": { "stringValue": "a1b2c3d4-1111" },
                "Issuer": { "stringValue": "https://cognito-idp.example/pool" },
                "ClientId": { "stringValue": "client123" },
                "GlobalRoles": { "stringValue": "[]" },
                "TeamGroups": { "stringValue": "[]" }
            }
        });
        let result = handle(&clients, sqs_event(vec![record])).await.unwrap();
        assert_eq!(result["batchItemFailures"], json!([]));
    }
}
