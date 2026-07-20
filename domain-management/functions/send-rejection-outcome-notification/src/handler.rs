use anyhow::{Context, Result};
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

    let html_body =
        build_rejection_email_body(domain_name, owner_account_id, rejected_by, justification);

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
    {
        if e.as_service_error()
            .is_some_and(nexus_common::ses::is_permanent_failure)
        {
            tracing::error!(error = %e, "permanent SES failure, acknowledging without retry");
            return Ok(());
        }
        return Err(e)
            .context("failed to send rejection email")
            .map_err(Into::into);
    }

    tracing::info!(domain_name, "rejection notification sent");

    Ok(())
}

fn build_rejection_email_body(
    domain_name: &str,
    owner_account_id: &str,
    rejected_by: &str,
    justification: &str,
) -> String {
    let domain_name = escape(domain_name);
    let owner_account_id = escape(owner_account_id);
    let rejected_by = escape(rejected_by);
    let justification = escape(justification);
    format!(
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
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_email_body_contains_domain_name() {
        let body = build_rejection_email_body(
            "billing",
            "111122223333",
            "admin@example.com",
            "Policy violation",
        );
        assert!(body.contains("billing"));
    }

    #[test]
    fn rejection_email_body_contains_owner_account_id() {
        let body = build_rejection_email_body(
            "billing",
            "111122223333",
            "admin@example.com",
            "Policy violation",
        );
        assert!(body.contains("111122223333"));
    }

    #[test]
    fn rejection_email_body_contains_rejected_by() {
        let body = build_rejection_email_body(
            "billing",
            "111122223333",
            "admin@example.com",
            "Policy violation",
        );
        assert!(body.contains("admin@example.com"));
    }

    #[test]
    fn rejection_email_body_contains_justification() {
        let body = build_rejection_email_body(
            "billing",
            "111122223333",
            "admin@example.com",
            "Policy violation",
        );
        assert!(body.contains("Policy violation"));
    }

    #[test]
    fn rejection_email_body_contains_heading() {
        let body = build_rejection_email_body(
            "billing",
            "111122223333",
            "admin@example.com",
            "Policy violation",
        );
        assert!(body.contains("Registration Rejected"));
    }

    #[test]
    fn rejection_email_body_escapes_html_input() {
        let body = build_rejection_email_body(
            "<billing>",
            "111122223333",
            "admin@example.com",
            "<script>alert(1)</script>",
        );
        assert!(body.contains("&lt;billing&gt;"));
        assert!(body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!body.contains("<script>alert(1)</script>"));
    }
}
