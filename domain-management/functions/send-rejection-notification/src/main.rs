use anyhow::{Context, Result};
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::Value;
use std::env;

async fn handler(event: LambdaEvent<Value>) -> Result<(), Error> {
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let ses = aws_sdk_sesv2::Client::new(&config);

    let from_email = env::var("APPROVAL_FROM_EMAIL").context("APPROVAL_FROM_EMAIL not set")?;

    let detail = event
        .payload
        .get("detail")
        .context("missing detail in EventBridge event")?;

    let domain_name = detail
        .get("domainName")
        .and_then(|v| v.as_str())
        .context("missing domainName in detail")?;

    let owner_email = detail
        .get("ownerEmail")
        .and_then(|v| v.as_str())
        .context("missing ownerEmail in detail")?;

    let rejected_by = detail
        .get("rejectedBy")
        .and_then(|v| v.as_str())
        .context("missing rejectedBy in detail")?;

    let justification = detail
        .get("justification")
        .and_then(|v| v.as_str())
        .unwrap_or("No justification provided");

    let owner_account_id = detail
        .get("ownerAccountId")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let html_body = format!(
        r#"<html>
<body>
<h2>Domain Registration Rejected</h2>
<p>Your domain registration request has been rejected.</p>
<table>
<tr><td><strong>Domain Name</strong></td><td>{domain_name}</td></tr>
<tr><td><strong>Owner Account</strong></td><td>{owner_account_id}</td></tr>
<tr><td><strong>Rejected By</strong></td><td>{rejected_by}</td></tr>
<tr><td><strong>Justification</strong></td><td>{justification}</td></tr>
</table>
</body>
</html>"#
    );

    ses.send_email()
        .from_email_address(&from_email)
        .destination(
            Destination::builder()
                .to_addresses(owner_email)
                .build(),
        )
        .content(
            EmailContent::builder()
                .simple(
                    Message::builder()
                        .subject(
                            Content::builder()
                                .data(format!("Domain Registration Rejected: '{domain_name}'"))
                                .build()
                                .context("failed to build subject")?,
                        )
                        .body(
                            Body::builder()
                                .html(
                                    Content::builder()
                                        .data(html_body)
                                        .build()
                                        .context("failed to build html body")?,
                                )
                                .build(),
                        )
                        .build(),
                )
                .build(),
        )
        .send()
        .await
        .context("failed to send rejection email")?;

    tracing::info!(domain_name, owner_email, "rejection notification sent");

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
