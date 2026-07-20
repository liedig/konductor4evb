use anyhow::{Context, Result};
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use lambda_runtime::{Error, LambdaEvent};
use nexus_common::dynamodb::s;
use nexus_common::html::escape;
use serde_json::Value;
use std::env;
use std::fmt::Write;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    ses: &aws_sdk_sesv2::Client,
    event: LambdaEvent<Value>,
) -> Result<(), Error> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let to_email =
        env::var("PLATFORM_OPERATOR_EMAIL").context("PLATFORM_OPERATOR_EMAIL not set")?;
    let from_email =
        env::var("NOTIFICATION_SENDER_EMAIL").context("NOTIFICATION_SENDER_EMAIL not set")?;
    let decide_endpoint_url =
        env::var("DECIDE_ENDPOINT_URL").context("DECIDE_ENDPOINT_URL not set")?;

    let payload = &event.payload;

    let task_token = payload
        .get("taskToken")
        .and_then(|v| v.as_str())
        .context("missing taskToken")?;

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

    let environments: Vec<String> = payload
        .get("environments")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

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
        .key("domainName", s(domain_name))
        .update_expression("SET taskToken = :token")
        .condition_expression("attribute_exists(domainName)")
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
        &environments,
        &decide_endpoint_url,
    );

    if let Err(e) = ses
        .send_email()
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
    {
        if e.as_service_error()
            .is_some_and(nexus_common::ses::is_permanent_failure)
        {
            tracing::error!(error = %e, "permanent SES failure, acknowledging without retry");
            return Ok(());
        }
        return Err(e)
            .context("failed to send approval email")
            .map_err(Into::into);
    }

    tracing::info!(domain_name, "approval notification sent");

    Ok(())
}

fn build_approval_email_body(
    domain_name: &str,
    owner_account_id: &str,
    owner_email: &str,
    task_token: &str,
    is_new_account: bool,
    environments: &[String],
    decide_endpoint_url: &str,
) -> String {
    let domain_name = escape(domain_name);
    let owner_account_id = escape(owner_account_id);
    let owner_email = escape(owner_email);
    let new_account_notice = if is_new_account {
        r#"<div style="background-color:#fff3cd;border:1px solid #ffc107;padding:12px;margin:12px 0;border-radius:4px">
<strong>New Account Detected:</strong> This is the first domain registration from account {owner_account_id}.
Ensure the trust-stack has been deployed to this account before approving.
</div>"#
            .replace("{owner_account_id}", &owner_account_id)
    } else {
        String::new()
    };

    let environments_display = if environments.is_empty() {
        "(none configured)".to_string()
    } else {
        environments.join(", ")
    };
    let environments_display = escape(&environments_display);

    let encoded_token = url_encode(task_token);
    let approve_url = escape(&format!(
        "{decide_endpoint_url}?decision=approve&token={encoded_token}"
    ));
    let reject_url = escape(&format!(
        "{decide_endpoint_url}?decision=reject&token={encoded_token}"
    ));

    format!(
        r#"<html>
<body>
<h2>Domain Registration Approval Required</h2>
<p>A new domain registration request requires your approval.</p>
<p>On approval, provisioning will cover the following environments: <strong>{environments_display}</strong>.</p>
{new_account_notice}
<table>
<tr><td><strong>Domain Name</strong></td><td>{domain_name}</td></tr>
<tr><td><strong>Owner Account</strong></td><td>{owner_account_id}</td></tr>
<tr><td><strong>Owner Email</strong></td><td>{owner_email}</td></tr>
<tr><td><strong>Environments</strong></td><td>{environments_display}</td></tr>
</table>
<p style="margin-top:24px">
  <a href="{approve_url}" style="display:inline-block;background-color:#1d8102;color:#ffffff;padding:10px 20px;margin-right:8px;text-decoration:none;border-radius:4px">Approve</a>
  <a href="{reject_url}" style="display:inline-block;background-color:#b31b1b;color:#ffffff;padding:10px 20px;text-decoration:none;border-radius:4px">Reject</a>
</p>
</body>
</html>"#
    )
}
fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        let is_unreserved = byte.is_ascii_alphanumeric()
            || byte == b'-'
            || byte == b'_'
            || byte == b'.'
            || byte == b'~';
        if is_unreserved {
            out.push(byte as char);
        } else {
            out.push('%');
            write!(&mut out, "{byte:02X}").unwrap();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envs() -> Vec<String> {
        vec!["dev".into(), "staging".into(), "prod".into()]
    }

    const DECIDE_URL: &str = "https://api.example.com/dev/domains/registrations/decide";

    #[test]
    fn approval_email_body_contains_domain_name() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
            &envs(),
            DECIDE_URL,
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
            &envs(),
            DECIDE_URL,
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
            &envs(),
            DECIDE_URL,
        );
        assert!(body.contains("owner@example.com"));
    }

    #[test]
    fn approval_email_body_contains_approve_and_reject_links() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
            &envs(),
            DECIDE_URL,
        );
        assert!(body.contains(&format!("{DECIDE_URL}?decision=approve&amp;token=tok-1")));
        assert!(body.contains(&format!("{DECIDE_URL}?decision=reject&amp;token=tok-1")));
    }

    #[test]
    fn approval_email_body_url_encodes_task_token_special_chars() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "AQC+abc/def=",
            false,
            &envs(),
            DECIDE_URL,
        );
        assert!(body.contains("token=AQC%2Babc%2Fdef%3D"));
        assert!(!body.contains("token=AQC+abc/def="));
    }

    #[test]
    fn approval_email_body_escapes_html_input() {
        let body = build_approval_email_body(
            "<billing>",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
            &["dev<script>".to_string()],
            DECIDE_URL,
        );
        assert!(body.contains("&lt;billing&gt;"));
        assert!(!body.contains("<billing>"));
        assert!(body.contains("dev&lt;script&gt;"));
    }

    #[test]
    fn approval_email_body_contains_heading() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
            &envs(),
            DECIDE_URL,
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
            &envs(),
            DECIDE_URL,
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
            &envs(),
            DECIDE_URL,
        );
        assert!(!body.contains("New Account Detected"));
    }

    #[test]
    fn approval_email_body_lists_environments() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
            &envs(),
            DECIDE_URL,
        );
        assert!(body.contains("dev, staging, prod"));
    }

    #[test]
    fn approval_email_body_handles_empty_environments() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            "tok-1",
            false,
            &[],
            DECIDE_URL,
        );
        assert!(body.contains("(none configured)"));
    }

    #[test]
    fn url_encode_preserves_unreserved_chars() {
        assert_eq!(url_encode("abcXYZ123-_.~"), "abcXYZ123-_.~");
    }

    #[test]
    fn url_encode_encodes_reserved_chars() {
        assert_eq!(url_encode("+/=&? "), "%2B%2F%3D%26%3F%20");
    }
}
