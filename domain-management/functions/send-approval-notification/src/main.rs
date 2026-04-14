use anyhow::{Context, Result};
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::Value;
use std::env;

use control_plane_common::dynamodb::s;

async fn handler(event: LambdaEvent<Value>) -> Result<(), Error> {
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let ddb = aws_sdk_dynamodb::Client::new(&config);
    let ses = aws_sdk_sesv2::Client::new(&config);

    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let to_email = env::var("APPROVAL_TO_EMAIL").context("APPROVAL_TO_EMAIL not set")?;
    let from_email = env::var("APPROVAL_FROM_EMAIL").context("APPROVAL_FROM_EMAIL not set")?;

    let payload = &event.payload;

    let task_token = payload
        .get("taskToken")
        .and_then(|v| v.as_str())
        .context("missing taskToken")?;

    let domain_id = payload
        .get("domainId")
        .and_then(|v| v.as_str())
        .context("missing domainId")?;

    let domain_name = payload
        .get("domainName")
        .and_then(|v| v.as_str())
        .context("missing domainName")?;

    let owner_account_id = payload
        .get("ownerAccountId")
        .and_then(|v| v.as_str())
        .context("missing ownerAccountId")?;

    let owner_email = payload
        .get("ownerEmail")
        .and_then(|v| v.as_str())
        .context("missing ownerEmail")?;

    ddb.update_item()
        .table_name(&table)
        .key("domainId", s(domain_id))
        .update_expression("SET taskToken = :token")
        .expression_attribute_values(":token", s(task_token))
        .send()
        .await
        .context("failed to persist taskToken to domain record")?;

    let html_body = format!(
        r#"<html>
<body>
<h2>Domain Registration Approval Required</h2>
<p>A new domain registration request requires your approval.</p>
<table>
<tr><td><strong>Domain Name</strong></td><td>{domain_name}</td></tr>
<tr><td><strong>Owner Account</strong></td><td>{owner_account_id}</td></tr>
<tr><td><strong>Owner Email</strong></td><td>{owner_email}</td></tr>
<tr><td><strong>Task Token</strong></td><td>{task_token}</td></tr>
</table>
</body>
</html>"#
    );

    ses.send_email()
        .from_email_address(&from_email)
        .destination(Destination::builder().to_addresses(&to_email).build())
        .content(
            EmailContent::builder()
                .simple(
                    Message::builder()
                        .subject(
                            Content::builder()
                                .data(format!("Approval Required: Domain '{domain_name}'"))
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
        .context("failed to send approval email")?;

    tracing::info!(domain_id, domain_name, "approval notification sent");

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
