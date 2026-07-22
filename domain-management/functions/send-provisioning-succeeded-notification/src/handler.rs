use anyhow::{Context, Result};
use aws_sdk_sesv2::error::ProvideErrorMetadata;
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::Value;
use std::env;

use nexus_common::dynamodb::s;
use nexus_common::html::escape;
use nexus_common::ses::recipients_from_item;

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

    let environments: Vec<String> = detail
        .get("environments")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let item = ddb
        .get_item()
        .table_name(&table)
        .key("domainName", s(domain_name))
        .send()
        .await
        .context("failed to load domain record")?
        .item
        .context("domain record not found")?;

    let recipients = recipients_from_item(&item).context("missing ownerEmail on domain record")?;

    let html_body = build_provisioning_succeeded_email_body(domain_name, &environments);

    if let Err(e) = ses
        .send_email()
        .from_email_address(&from_email)
        .destination(
            Destination::builder()
                .set_to_addresses(Some(recipients))
                .build(),
        )
        .content(
            EmailContent::builder()
                .simple(
                    Message::builder()
                        .subject(
                            Content::builder()
                                .data(format!("Domain Live: '{domain_name}'"))
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
            .context("failed to send provisioning-succeeded email")
            .map_err(Into::into);
    }

    tracing::info!(domain_name, "provisioning-succeeded notification sent");

    Ok(())
}

fn build_provisioning_succeeded_email_body(domain_name: &str, environments: &[String]) -> String {
    let domain_name = escape(domain_name);
    let environments_html = if environments.is_empty() {
        String::new()
    } else {
        let items: String = environments
            .iter()
            .map(|env| format!("<li>{}</li>", escape(env)))
            .collect();
        format!("<p>Provisioned environments:</p>\n<ul>{items}</ul>")
    };
    format!(
        r#"<html>
<body>
<h2>Domain Live</h2>
<p>Your domain <strong>{domain_name}</strong> has been provisioned and is now active.</p>
{environments_html}
<p>You can now publish events to your domain and manage subscriptions.</p>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_mentions_domain_name_and_active() {
        let body = build_provisioning_succeeded_email_body("billing", &[]);
        assert!(body.contains("billing"));
        assert!(body.contains("active"));
    }

    #[test]
    fn body_lists_environments() {
        let envs = vec!["dev".to_string(), "prod".to_string()];
        let body = build_provisioning_succeeded_email_body("billing", &envs);
        assert!(body.contains("<li>dev</li>"));
        assert!(body.contains("<li>prod</li>"));
    }

    #[test]
    fn body_omits_environment_list_when_empty() {
        let body = build_provisioning_succeeded_email_body("billing", &[]);
        assert!(!body.contains("<ul>"));
    }

    #[test]
    fn body_escapes_html_input() {
        let envs = vec!["<dev>".to_string()];
        let body = build_provisioning_succeeded_email_body("<billing>", &envs);
        assert!(body.contains("&lt;billing&gt;"));
        assert!(body.contains("&lt;dev&gt;"));
        assert!(!body.contains("<billing>"));
    }

    #[test]
    fn body_is_html() {
        let body = build_provisioning_succeeded_email_body("billing", &[]);
        assert!(body.starts_with("<html>"));
        assert!(body.trim_end().ends_with("</html>"));
    }
}
