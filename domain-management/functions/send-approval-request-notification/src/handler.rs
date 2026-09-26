use anyhow::{Context, Result};
use aws_sdk_sesv2::error::ProvideErrorMetadata;
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use konductor_common::dynamodb::s;
use konductor_common::html::escape;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::Value;
use std::env;

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
    let console_url = env::var("WEB_CONSOLE_URL").context("WEB_CONSOLE_URL not set")?;

    let fields = extract_event_fields(&event.payload)?;
    let is_new_account = check_is_new_account(ddb, &table, &fields.owner_account_id).await?;

    ddb.update_item()
        .table_name(&table)
        .key("domainName", s(&fields.domain_name))
        .update_expression("SET taskToken = :token")
        .condition_expression("attribute_exists(domainName)")
        .expression_attribute_values(":token", s(&fields.task_token))
        .send()
        .await
        .context("failed to persist taskToken to domain record")?;

    let effective_accounts = konductor_common::environment_accounts::effective_accounts(
        fields.environment_accounts.as_ref(),
        &fields.owner_account_id,
        &fields.environments,
    );

    let html_body = build_approval_email_body(
        &fields.domain_name,
        &fields.owner_account_id,
        &fields.owner_email,
        is_new_account,
        &fields.environments,
        &effective_accounts,
        &console_url,
    );

    send_approval_email(ses, &from_email, &to_email, &fields.domain_name, html_body).await
}

struct EventFields {
    task_token: String,
    domain_name: String,
    owner_account_id: String,
    owner_email: String,
    environments: Vec<String>,
    environment_accounts: Option<std::collections::HashMap<String, String>>,
}

fn extract_event_fields(payload: &Value) -> Result<EventFields, Error> {
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
    let environments = payload
        .get("environments")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let environment_accounts = payload
        .get("environmentAccounts")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        });
    Ok(EventFields {
        task_token: task_token.to_string(),
        domain_name: domain_name.to_string(),
        owner_account_id: owner_account_id.to_string(),
        owner_email: owner_email.to_string(),
        environments,
        environment_accounts,
    })
}

async fn check_is_new_account(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    owner_account_id: &str,
) -> Result<bool, Error> {
    let active_domains = ddb
        .query()
        .table_name(table)
        .index_name("by-owner")
        .key_condition_expression("ownerAccountId = :acct")
        .filter_expression("#s = :active")
        .expression_attribute_names("#s", "status")
        .expression_attribute_values(":acct", s(owner_account_id))
        .expression_attribute_values(":active", s("ACTIVE"))
        .send()
        .await
        .context("failed to query by-owner GSI")?;
    Ok(active_domains.count() == 0)
}

async fn send_approval_email(
    ses: &aws_sdk_sesv2::Client,
    from_email: &str,
    to_email: &str,
    domain_name: &str,
    html_body: String,
) -> Result<(), Error> {
    if let Err(e) = ses
        .send_email()
        .from_email_address(from_email)
        .destination(Destination::builder().to_addresses(to_email).build())
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
            .and_then(ProvideErrorMetadata::code)
            .is_some_and(konductor_common::ses::is_permanent_failure)
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
    is_new_account: bool,
    environments: &[String],
    environment_accounts: &std::collections::HashMap<String, String>,
    console_url: &str,
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
    let approvals_url = format!("{}/admin", escape(console_url.trim_end_matches('/')));

    let accounts_display = if environment_accounts.is_empty() {
        String::new()
    } else {
        let mut rows: Vec<(&String, &String)> = environment_accounts.iter().collect();
        rows.sort_by_key(|(environment, _)| *environment);
        let rows_html = rows
            .into_iter()
            .map(|(environment, account)| {
                format!(
                    "<tr><td>{}</td><td>{}</td></tr>",
                    escape(environment),
                    escape(account)
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!(
            r#"<p>Provisioning places each environment's resources into the account below. Ensure the trust-stack is deployed to every listed account before approving.</p>
<table>
<tr><td><strong>Environment</strong></td><td><strong>Account</strong></td></tr>
{rows_html}
</table>"#
        )
    };

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
{accounts_display}
<p style="margin-top:24px">
  <a href="{approvals_url}">Open Pending approvals</a> in the Konductor for Amazon EventBridge console
  to review and approve or reject this registration.
</p>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONSOLE_URL: &str = "https://console.example.com";

    fn envs() -> Vec<String> {
        vec!["dev".into(), "staging".into(), "prod".into()]
    }

    fn default_accounts() -> std::collections::HashMap<String, String> {
        envs()
            .into_iter()
            .map(|e| (e, "111122223333".to_string()))
            .collect()
    }

    #[test]
    fn approval_email_body_contains_domain_name() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            false,
            &envs(),
            &default_accounts(),
            CONSOLE_URL,
        );
        assert!(body.contains("billing"));
    }

    #[test]
    fn approval_email_body_contains_owner_account_id() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            false,
            &envs(),
            &default_accounts(),
            CONSOLE_URL,
        );
        assert!(body.contains("111122223333"));
    }

    #[test]
    fn approval_email_body_contains_owner_email() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            false,
            &envs(),
            &default_accounts(),
            CONSOLE_URL,
        );
        assert!(body.contains("owner@example.com"));
    }

    #[test]
    fn approval_email_body_links_to_pending_approvals() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            false,
            &envs(),
            &default_accounts(),
            CONSOLE_URL,
        );
        assert!(body.contains(r#"href="https://console.example.com/admin""#));
        assert!(body.contains("Pending approvals"));
    }

    #[test]
    fn approval_email_body_escapes_console_url() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            false,
            &envs(),
            &default_accounts(),
            "https://evil.example.com/<script>",
        );
        assert!(!body.contains("evil.example.com/<script>"));
    }

    #[test]
    fn approval_email_body_does_not_contain_task_token_links() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            false,
            &envs(),
            &default_accounts(),
            CONSOLE_URL,
        );
        assert!(!body.contains("decision=approve"));
        assert!(!body.contains("decision=reject"));
        assert!(!body.contains("token="));
    }

    #[test]
    fn approval_email_body_escapes_html_input() {
        let hostile_accounts =
            std::collections::HashMap::from([("dev".to_string(), "1111<script>".to_string())]);
        let body = build_approval_email_body(
            "<billing>",
            "111122223333",
            "owner@example.com",
            false,
            &["dev<script>".to_string()],
            &hostile_accounts,
            CONSOLE_URL,
        );
        assert!(body.contains("&lt;billing&gt;"));
        assert!(!body.contains("<billing>"));
        assert!(body.contains("dev&lt;script&gt;"));
        assert!(
            body.contains("1111&lt;script&gt;"),
            "account map values are escaped like every other interpolated field"
        );
        assert!(!body.contains("1111<script>"));
    }

    #[test]
    fn approval_email_body_contains_heading() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            false,
            &envs(),
            &default_accounts(),
            CONSOLE_URL,
        );
        assert!(body.contains("Approval Required"));
    }

    #[test]
    fn new_account_shows_trust_stack_notice() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            true,
            &envs(),
            &default_accounts(),
            CONSOLE_URL,
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
            false,
            &envs(),
            &default_accounts(),
            CONSOLE_URL,
        );
        assert!(!body.contains("New Account Detected"));
    }

    #[test]
    fn approval_email_body_lists_environments() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            false,
            &envs(),
            &default_accounts(),
            CONSOLE_URL,
        );
        assert!(body.contains("dev, staging, prod"));
    }

    #[test]
    fn approval_email_body_handles_empty_environments() {
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            false,
            &[],
            &std::collections::HashMap::new(),
            CONSOLE_URL,
        );
        assert!(body.contains("(none configured)"));
        assert!(
            !body.contains("<strong>Account</strong>"),
            "no account table without a resolved map"
        );
    }

    #[test]
    fn approval_email_body_renders_the_resolved_account_per_environment() {
        let accounts = std::collections::HashMap::from([
            ("dev".to_string(), "111122223333".to_string()),
            ("staging".to_string(), "111122223333".to_string()),
            ("prod".to_string(), "999988887777".to_string()),
        ]);
        let body = build_approval_email_body(
            "billing",
            "111122223333",
            "owner@example.com",
            false,
            &envs(),
            &accounts,
            CONSOLE_URL,
        );
        assert!(
            body.contains("<td>prod</td><td>999988887777</td>"),
            "one line per environment, env then account: {body}"
        );
        assert!(body.contains("<td>dev</td><td>111122223333</td>"));
        assert!(body.contains("<td>staging</td><td>111122223333</td>"));
        assert!(
            body.contains("trust-stack is deployed to every listed account"),
            "the approver sees the trust prerequisite for override accounts"
        );
    }
}
