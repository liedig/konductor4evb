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

    let environments: Vec<String> = detail
        .get("environments")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let html_body = build_approval_outcome_email_body(domain_name, &environments);

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
                                .data(format!("Domain Approved: '{domain_name}'"))
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
            .context("failed to send approval-outcome email")
            .map_err(Into::into);
    }

    tracing::info!(domain_name, "approval-outcome notification sent");

    Ok(())
}

fn build_approval_outcome_email_body(domain_name: &str, environments: &[String]) -> String {
    let domain_name = escape(domain_name);
    let environments_display = if environments.is_empty() {
        "(none configured)".to_string()
    } else {
        environments.join(", ")
    };
    let environments_display = escape(&environments_display);

    format!(
        r#"<html>
<body>
<h2>Domain Registration Approved</h2>
<p>Your domain <strong>{domain_name}</strong> has been approved.</p>
<p>Provisioning is now underway across the following environments: <strong>{environments_display}</strong>.</p>
<p>You will receive a follow-up notification if provisioning encounters any issues.</p>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envs() -> Vec<String> {
        vec!["dev".into(), "staging".into(), "prod".into()]
    }

    #[test]
    fn body_mentions_domain_name() {
        let body = build_approval_outcome_email_body("billing", &envs());
        assert!(body.contains("billing"));
    }

    #[test]
    fn body_lists_environments() {
        let body = build_approval_outcome_email_body("billing", &envs());
        assert!(body.contains("dev, staging, prod"));
    }

    #[test]
    fn body_handles_empty_environments() {
        let body = build_approval_outcome_email_body("billing", &[]);
        assert!(body.contains("(none configured)"));
    }

    #[test]
    fn body_escapes_html_input() {
        let body = build_approval_outcome_email_body("<billing>", &["dev<script>".to_string()]);
        assert!(body.contains("&lt;billing&gt;"));
        assert!(body.contains("dev&lt;script&gt;"));
        assert!(!body.contains("<billing>"));
    }

    #[test]
    fn body_is_html() {
        let body = build_approval_outcome_email_body("billing", &envs());
        assert!(body.starts_with("<html>"));
        assert!(body.trim_end().ends_with("</html>"));
    }

    #[test]
    fn subject_is_success_toned() {
        let body = build_approval_outcome_email_body("billing", &envs());
        assert!(body.contains("Approved"));
        assert!(body.contains("underway"));
    }
}
