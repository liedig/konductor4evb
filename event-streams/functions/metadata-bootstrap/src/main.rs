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
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
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
    http.put(checked_response_url(url)?)
        .json(&response)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// Custom-resource response URLs are presigned S3 URLs on AWS hosts; a
/// forged invoke could otherwise aim the PUT (and its redirects) anywhere.
fn checked_response_url(raw: &str) -> Result<&str, Error> {
    let parsed = reqwest::Url::parse(raw).map_err(|e| format!("invalid ResponseURL: {e}"))?;
    let host = parsed.host_str().unwrap_or("");
    if parsed.scheme() != "https" || !host.ends_with(".amazonaws.com") {
        return Err(format!("ResponseURL host '{host}' is not an AWS endpoint").into());
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::checked_response_url;

    #[test]
    fn accepts_cloudformation_response_url() {
        let url = "https://cloudformation-customresource-response-apsoutheast2.s3.ap-southeast-2.amazonaws.com/arn:aws:cloudformation:ap-southeast-2:123456789012:stack/konductor-core/guid?X-Amz-Signature=abc";
        assert_eq!(checked_response_url(url).unwrap(), url);
    }

    #[test]
    fn rejects_non_https() {
        assert!(checked_response_url("http://s3.ap-southeast-2.amazonaws.com/x").is_err());
    }

    #[test]
    fn rejects_non_aws_host() {
        assert!(checked_response_url("https://169.254.169.254/latest/meta-data").is_err());
        assert!(checked_response_url("https://example.com/x").is_err());
    }

    #[test]
    fn rejects_aws_lookalike_host() {
        assert!(checked_response_url("https://amazonaws.com.evil.com/x").is_err());
        assert!(checked_response_url("https://notamazonaws.com/x").is_err());
    }
}
