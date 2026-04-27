use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::Value;

use nexus_common::models::RegisterDomainRequest;

mod register;
mod update;

struct Clients {
    ddb: aws_sdk_dynamodb::Client,
    eb: aws_sdk_eventbridge::Client,
    sfn: aws_sdk_sfn::Client,
}

async fn handler(event: LambdaEvent<Value>) -> Result<(), Error> {
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let clients = Clients {
        ddb: aws_sdk_dynamodb::Client::new(&config),
        eb: aws_sdk_eventbridge::Client::new(&config),
        sfn: aws_sdk_sfn::Client::new(&config),
    };

    let records = event
        .payload
        .get("Records")
        .and_then(|r| r.as_array())
        .context("missing Records array")?;

    for record in records {
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
                if let Err(e) =
                    register::handle(&clients.ddb, &clients.eb, &clients.sfn, &request).await
                {
                    tracing::error!("register failed: {e:?}");
                    return Err(e.into());
                }
            }
            "PUT" => {
                let request: update::UpdateDomainMessage =
                    serde_json::from_str(body_str).context("invalid UpdateDomainMessage")?;
                if let Err(e) = update::handle(&clients.ddb, &clients.eb, &request).await {
                    tracing::error!("update failed: {e:?}");
                    return Err(e.into());
                }
            }
            other => {
                return Err(format!("unrecognised HttpMethod: {other}").into());
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    lambda_runtime::run(service_fn(handler)).await
}
