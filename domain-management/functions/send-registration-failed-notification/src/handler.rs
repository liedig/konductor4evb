use anyhow::{Context, Result};
use aws_sdk_sesv2::error::ProvideErrorMetadata;
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use lambda_runtime::{Error, LambdaEvent};
use nexus_common::html::escape;
use serde_json::Value;
use std::env;

pub async fn handle(ses: &aws_sdk_sesv2::Client, event: LambdaEvent<Value>) -> Result<(), Error> {
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

    let owner_email = detail
        .get("ownerEmail")
        .and_then(|v| v.as_str())
        .context("missing ownerEmail in detail")?;

    let reason = detail
        .get("reason")
        .and_then(|v| v.as_str())
        .context("missing reason in detail")?;

    let html_body = build_failure_email_body(domain_name, reason);

    if let Err(e) = ses
        .send_email()
        .from_email_address(&from_email)
        .destination(Destination::builder().to_addresses(owner_email).build())
        .content(
            EmailContent::builder()
                .simple(
                    Message::builder()
                        .subject(
                            Content::builder()
                                .data(format!("Domain Registration Failed: '{domain_name}'"))
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
    {
        if e.as_service_error()
            .and_then(ProvideErrorMetadata::code)
            .is_some_and(nexus_common::ses::is_permanent_failure)
        {
            tracing::error!(error = %e, "permanent SES failure, acknowledging without retry");
            return Ok(());
        }
        return Err(e)
            .context("failed to send registration-failed email")
            .map_err(Into::into);
    }

    tracing::info!(domain_name, "registration-failed notification sent");

    Ok(())
}

fn build_failure_email_body(domain_name: &str, reason: &str) -> String {
    let domain_name = escape(domain_name);
    let reason = escape(reason);
    format!(
        r#"<html>
<body>
<h2>Domain Registration Failed</h2>
<p>Your domain registration request was not accepted.</p>
<table>
<tr><td><strong>Domain Name</strong></td><td>{domain_name}</td></tr>
<tr><td><strong>Reason</strong></td><td>{reason}</td></tr>
</table>
<p>If you believe this was in error, please contact the platform team. You may correct the submission and re-register the domain.</p>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_email_body_contains_domain_name() {
        let body = build_failure_email_body("billing", "domain already exists");
        assert!(body.contains("billing"));
    }

    #[test]
    fn failure_email_body_contains_reason() {
        let body = build_failure_email_body("billing", "domain already exists");
        assert!(body.contains("domain already exists"));
    }

    #[test]
    fn failure_email_body_contains_heading() {
        let body = build_failure_email_body("billing", "domain already exists");
        assert!(body.contains("Registration Failed"));
    }

    #[test]
    fn failure_email_body_escapes_html_input() {
        let body = build_failure_email_body("<billing>", "<script>alert(1)</script>");
        assert!(body.contains("&lt;billing&gt;"));
        assert!(body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!body.contains("<script>alert(1)</script>"));
    }
}
