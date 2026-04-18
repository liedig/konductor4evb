use anyhow::{Context, Result};
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::Value;
use std::env;

use nexus_common::dynamodb::s;

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

    let active_domains = ddb
        .query()
        .table_name(&table)
        .index_name("by-owner")
        .key_condition_expression("ownerAccountId = :acct")
        .filter_expression("#s = :active")
        .expression_attribute_names("#s", "status")
        .expression_attribute_values(":acct", s(owner_account_id))
        .expression_attribute_values(":active", s("ACTIVE"))
        .send()
        .await
        .context("failed to query by-owner GSI")?;

    let is_new_account = active_domains.count() == 0;

    ddb.update_item()
        .table_name(&table)
        .key("domainId", s(domain_id))
        .update_expression("SET taskToken = :token")
        .expression_attribute_values(":token", s(task_token))
        .send()
        .await
        .context("failed to persist taskToken to domain record")?;

    let html_body = build_approval_email_body(
        domain_name,
        owner_account_id,
        owner_email,
        task_token,
        is_new_account,
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

fn build_approval_email_body(
    domain_name: &str,
    owner_account_id: &str,
    owner_email: &str,
    task_token: &str,
    is_new_account: bool,
) -> String {
    let new_account_notice = if is_new_account {
        r#"<div style="background-color:#fff3cd;border:1px solid #ffc107;padding:12px;margin:12px 0;border-radius:4px">
<strong>New Account Detected:</strong> This is the first domain registration from account {owner_account_id}.
Ensure the trust-stack has been deployed to this account before approving.
</div>"#
            .replace("{owner_account_id}", owner_account_id)
    } else {
        String::new()
    };

    format!(
        r#"<html>
<body>
<h2>Domain Registration Approval Required</h2>
<p>A new domain registration request requires your approval.</p>
{new_account_notice}
<table>
<tr><td><strong>Domain Name</strong></td><td>{domain_name}</td></tr>
<tr><td><strong>Owner Account</strong></td><td>{owner_account_id}</td></tr>
<tr><td><strong>Owner Email</strong></td><td>{owner_email}</td></tr>
<tr><td><strong>Task Token</strong></td><td>{task_token}</td></tr>
</table>
</body>
</html>"#
    )
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    lambda_runtime::run(service_fn(handler)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_email_body_contains_domain_name() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
        );
        assert!(body.contains("billing"));
    }

    #[test]
    fn approval_email_body_contains_owner_account_id() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
        );
        assert!(body.contains("111122223333"));
    }

    #[test]
    fn approval_email_body_contains_owner_email() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
        );
        assert!(body.contains("owner@example.com"));
    }

    #[test]
    fn approval_email_body_contains_task_token() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
        );
        assert!(body.contains("tok-1"));
    }

    #[test]
    fn approval_email_body_contains_heading() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
        );
        assert!(body.contains("Approval Required"));
    }

    #[test]
    fn new_account_shows_trust_stack_notice() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            true,
        );
        assert!(body.contains("New Account Detected"));
        assert!(body.contains("trust-stack"));
    }

    #[test]
    fn known_account_omits_trust_stack_notice() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
        );
        assert!(!body.contains("New Account Detected"));
    }
}
