use anyhow::Result;
use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::Value;

mod decide;

async fn handler(event: LambdaEvent<Value>) -> Result<(), Error> {
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let ddb = aws_sdk_dynamodb::Client::new(&config);
    let sfn = aws_sdk_sfn::Client::new(&config);
    let eb = aws_sdk_eventbridge::Client::new(&config);

    let records = event
        .payload
        .get("Records")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();

    for record in records {
        let http_method = record
            .get("messageAttributes")
            .and_then(|a| a.get("HttpMethod"))
            .and_then(|m| m.get("stringValue"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let body_str = record
            .get("body")
            .and_then(|b| b.as_str())
            .unwrap_or("{}");

        let body: Value = serde_json::from_str(body_str).unwrap_or(Value::Object(Default::default()));

        match http_method {
            "POST" => {
                if let Err(e) = decide::handle(&ddb, &sfn, &eb, &body).await {
                    tracing::error!("decide failed: {e:?}");
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
