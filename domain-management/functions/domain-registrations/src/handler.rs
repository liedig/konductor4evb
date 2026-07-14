use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use crate::register;
use crate::update;
use nexus_common::error::ControlPlaneError;
use nexus_common::models::RegisterDomainRequest;

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

    match http_method {
        "POST" => {
            let request: RegisterDomainRequest =
                serde_json::from_str(body_str).context("invalid RegisterDomainRequest")?;
            register::handle(
                &clients.ddb,
                &clients.eb,
                &clients.sfn,
                &request,
                message_id,
            )
            .await
        }
        "PUT" => {
            let request: update::UpdateDomainMessage =
                serde_json::from_str(body_str).context("invalid UpdateDomainMessage")?;
            update::handle(&clients.ddb, &clients.eb, &request).await
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
    use aws_sdk_dynamodb::operation::put_item::PutItemOutput;
    use aws_sdk_eventbridge::operation::put_events::PutEventsOutput;
    use aws_sdk_sfn::operation::start_execution::StartExecutionOutput;
    use aws_smithy_mocks::{mock, mock_client};
    use lambda_runtime::Context as LambdaContext;

    fn set_env() {
        std::env::set_var("DOMAINS_TABLE", "domains");
        std::env::set_var(
            "APPROVAL_STATE_MACHINE_ARN",
            "arn:aws:states:us-east-1:123:stateMachine:approval",
        );
        std::env::set_var("NEXUS_EVENT_BUS", "nexus-bus");
        std::env::set_var("ENVIRONMENTS", "dev,staging,prod");
    }

    fn happy_clients() -> Clients {
        let put_rule = mock!(aws_sdk_dynamodb::Client::put_item)
            .then_output(|| PutItemOutput::builder().build());
        let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
            .then_output(|| PutEventsOutput::builder().build());
        let sfn_rule = mock!(aws_sdk_sfn::Client::start_execution).then_output(|| {
            StartExecutionOutput::builder()
                .execution_arn("arn:exec")
                .start_date(aws_sdk_sfn::primitives::DateTime::from_secs(0))
                .build()
                .unwrap()
        });
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, [&put_rule]),
            eb: mock_client!(aws_sdk_eventbridge, [&eb_rule]),
            sfn: mock_client!(aws_sdk_sfn, [&sfn_rule]),
        }
    }

    fn register_record(message_id: &str, body: &Value) -> Value {
        json!({
            "messageId": message_id,
            "body": body.to_string(),
            "messageAttributes": { "HttpMethod": { "stringValue": "POST" } }
        })
    }

    fn valid_register_body() -> Value {
        json!({
            "domainName": "billing",
            "ownerAccountId": "111122223333",
            "ownerEmail": "owner@example.com",
            "secondaryOwnerEmails": ["backup@example.com"]
        })
    }

    fn sqs_event(records: Vec<Value>) -> LambdaEvent<Value> {
        LambdaEvent::new(json!({ "Records": records }), LambdaContext::default())
    }

    #[tokio::test]
    async fn successful_batch_reports_no_failures() {
        set_env();
        let clients = happy_clients();

        let event = sqs_event(vec![register_record("msg-1", &valid_register_body())]);
        let result = handle(&clients, event).await.unwrap();

        assert_eq!(result["batchItemFailures"], json!([]));
    }

    #[tokio::test]
    async fn malformed_body_is_reported_as_item_failure() {
        set_env();
        let clients = happy_clients();

        let record = json!({
            "messageId": "msg-bad",
            "body": "not json {",
            "messageAttributes": { "HttpMethod": { "stringValue": "POST" } }
        });
        let result = handle(&clients, sqs_event(vec![record])).await.unwrap();

        assert_eq!(
            result["batchItemFailures"],
            json!([{ "itemIdentifier": "msg-bad" }])
        );
    }

    #[tokio::test]
    async fn failed_record_does_not_fail_the_rest_of_the_batch() {
        set_env();
        let clients = happy_clients();

        let event = sqs_event(vec![
            json!({
                "messageId": "msg-bad",
                "body": "{}",
                "messageAttributes": { "HttpMethod": { "stringValue": "DELETE" } }
            }),
            register_record("msg-good", &valid_register_body()),
        ]);
        let result = handle(&clients, event).await.unwrap();

        assert_eq!(
            result["batchItemFailures"],
            json!([{ "itemIdentifier": "msg-bad" }])
        );
    }
}
