mod handler;

use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::{json, Value};

/// Stable physical resource id: bootstrap never replaces itself, and stack
/// deletion must not delete configuration state.
const PHYSICAL_RESOURCE_ID: &str = "konductor-metadata-bootstrap";

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let clients = handler::Clients {
        ddb: aws_sdk_dynamodb::Client::new(&config),
    };
    let http = reqwest::Client::new();
    let clients_ref = &clients;
    let http_ref = &http;

    lambda_runtime::run(service_fn(move |event: LambdaEvent<Value>| async move {
        let status = match handler::BootstrapConfig::from_env() {
            Ok(config) => match handler::handle(clients_ref, &config, &event.payload).await {
                Ok(()) => "SUCCESS",
                Err(error) => {
                    tracing::error!(error = %error, "metadata bootstrap failed");
                    "FAILED"
                }
            },
            Err(missing) => {
                tracing::error!(%missing, "metadata bootstrap is misconfigured");
                "FAILED"
            }
        };
        respond(http_ref, &event.payload, status).await
    }))
    .await
}

/// Puts the CloudFormation custom-resource response to the presigned
/// response URL. The handler owns the seeding semantics; this owns only the
/// protocol.
async fn respond(http: &reqwest::Client, event: &Value, status: &str) -> Result<(), Error> {
    let response = json!({
        "Status": status,
        "PhysicalResourceId": PHYSICAL_RESOURCE_ID,
        "StackId": event.get("StackId").cloned().unwrap_or(Value::Null),
        "RequestId": event.get("RequestId").cloned().unwrap_or(Value::Null),
        "LogicalResourceId": event.get("LogicalResourceId").cloned().unwrap_or(Value::Null),
        "Data": {},
    });
    let url = event
        .get("ResponseURL")
        .and_then(Value::as_str)
        .ok_or("custom resource event is missing ResponseURL")?;
    http.put(url)
        .json(&response)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
