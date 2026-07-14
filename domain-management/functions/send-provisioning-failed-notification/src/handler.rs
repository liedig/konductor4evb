use anyhow::{Context, Result};
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use lambda_runtime::{Error, LambdaEvent};
use nexus_common::dynamodb::s;
use nexus_common::html::escape;
use serde_json::Value;
use std::env;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    ses: &aws_sdk_sesv2::Client,
    event: LambdaEvent<Value>,
) -> Result<(), Error> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let from_email =
        env::var("NOTIFICATION_SENDER_EMAIL").context("NOTIFICATION_SENDER_EMAIL not set")?;

    let detail = event
        .payload
        .get("detail")
        .context("missing detail in EventBridge event")?;

    let domain_name = detail
        .get("domainName")
        .and_then(|v| v.as_str())
        .context("missing domainName in detail")?;

    let item = ddb
        .get_item()
        .table_name(&table)
        .key("domainName", s(domain_name))
        .send()
        .await
        .context("failed to load domain record")?
        .item
        .context("domain record not found")?;

    let owner_email = item
        .get("ownerEmail")
        .and_then(|v| v.as_s().ok())
        .context("missing ownerEmail on domain record")?;

    let html_body = build_provisioning_failed_email_body(domain_name);

    ses.send_email()
        .from_email_address(&from_email)
        .destination(Destination::builder().to_addresses(owner_email).build())
        .content(
            EmailContent::builder()
                .simple(
                    Message::builder()
                        .subject(
                            Content::builder()
                                .data(format!("Domain Provisioning Issue: '{domain_name}'"))
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
        .context("failed to send provisioning-failed email")?;

    tracing::info!(domain_name, "provisioning-failed notification sent");

    Ok(())
}

fn build_provisioning_failed_email_body(domain_name: &str) -> String {
    let domain_name = escape(domain_name);
    format!(
        r#"<html>
<body>
<h2>Domain Provisioning Issue</h2>
<p>Your domain <strong>{domain_name}</strong> encountered a problem during provisioning.</p>
<p>The platform team has been notified and will follow up. No action is required from you at this time.</p>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_mentions_domain_name() {
        let body = build_provisioning_failed_email_body("billing");
        assert!(body.contains("billing"));
    }

    #[test]
    fn body_escapes_html_input() {
        let body = build_provisioning_failed_email_body("<billing>");
        assert!(body.contains("&lt;billing&gt;"));
        assert!(!body.contains("<billing>"));
    }

    #[test]
    fn body_is_vague_and_has_no_technical_detail() {
        let body = build_provisioning_failed_email_body("billing");
        assert!(body.contains("encountered a problem"));
        assert!(body.contains("platform team has been notified"));
        let lower = body.to_lowercase();
        assert!(!lower.contains("error"));
        assert!(!lower.contains("exception"));
        assert!(!lower.contains("stack"));
        assert!(!lower.contains("trace"));
        assert!(!lower.contains("failedstep"));
    }

    #[test]
    fn body_is_html() {
        let body = build_provisioning_failed_email_body("billing");
        assert!(body.starts_with("<html>"));
        assert!(body.trim_end().ends_with("</html>"));
    }
}
