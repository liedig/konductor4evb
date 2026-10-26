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
    let settings = Settings::from_env()?;
    process(ddb, ses, &settings, &event.payload).await
}

struct Settings {
    table: String,
    operator_email: String,
    sender_email: String,
    console_url: String,
}

impl Settings {
    fn from_env() -> Result<Self, Error> {
        Ok(Self {
            table: env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?,
            operator_email: env::var("PLATFORM_OPERATOR_EMAIL")
                .context("PLATFORM_OPERATOR_EMAIL not set")?,
            sender_email: env::var("NOTIFICATION_SENDER_EMAIL")
                .context("NOTIFICATION_SENDER_EMAIL not set")?,
            console_url: env::var("WEB_CONSOLE_URL").context("WEB_CONSOLE_URL not set")?,
        })
    }

    fn notifications_enabled(&self) -> bool {
        !self.sender_email.is_empty() && !self.operator_email.is_empty()
    }
}

async fn process(
    ddb: &aws_sdk_dynamodb::Client,
    ses: &aws_sdk_sesv2::Client,
    settings: &Settings,
    payload: &Value,
) -> Result<(), Error> {
    let fields = extract_event_fields(payload)?;
    if !settings.notifications_enabled() {
        persist_task_token(ddb, &settings.table, &fields).await?;
        tracing::info!(
            domain_name = fields.domain_name.as_str(),
            "notifications disabled, task token recorded without an approval email"
        );
        return Ok(());
    }

    let is_new_account =
        check_is_new_account(ddb, &settings.table, &fields.owner_account_id).await?;
    persist_task_token(ddb, &settings.table, &fields).await?;

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
        &settings.console_url,
    );

    send_approval_email(
        ses,
        &settings.sender_email,
        &settings.operator_email,
        &fields.domain_name,
        html_body,
    )
    .await
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

async fn persist_task_token(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    fields: &EventFields,
) -> Result<(), Error> {
    ddb.update_item()
        .table_name(table)
        .key("domainName", s(&fields.domain_name))
        .update_expression("SET taskToken = :token")
        .condition_expression("attribute_exists(domainName)")
        .expression_attribute_values(":token", s(&fields.task_token))
        .send()
        .await
        .context("failed to persist taskToken to domain record")?;
    Ok(())
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
    use aws_sdk_dynamodb::operation::query::QueryOutput;
    use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
    use aws_sdk_sesv2::operation::send_email::SendEmailOutput;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use serde_json::json;

    const CONSOLE_URL: &str = "https://console.example.com";
    const TASK_TOKEN: &str = "AQABAAAA...task-token";

    fn settings(sender_email: &str, operator_email: &str) -> Settings {
        Settings {
            table: "domains".to_string(),
            operator_email: operator_email.to_string(),
            sender_email: sender_email.to_string(),
            console_url: CONSOLE_URL.to_string(),
        }
    }

    fn approval_request() -> Value {
        json!({
            "taskToken": TASK_TOKEN,
            "domainName": "billing",
            "ownerAccountId": "111122223333",
            "ownerEmail": "owner@example.com",
            "environments": ["dev", "staging", "prod"],
        })
    }

    fn task_token_write() -> Rule {
        mock!(aws_sdk_dynamodb::Client::update_item)
            .match_requests(|req| {
                req.table_name() == Some("domains")
                    && req
                        .expression_attribute_values()
                        .and_then(|values| values.get(":token"))
                        == Some(&s(TASK_TOKEN))
            })
            .then_output(|| UpdateItemOutput::builder().build())
    }

    fn active_domains_query() -> Rule {
        mock!(aws_sdk_dynamodb::Client::query)
            .then_output(|| QueryOutput::builder().count(0).build())
    }

    #[tokio::test]
    async fn disabled_notifications_record_the_task_token_without_email() {
        let write = task_token_write();
        let query = active_domains_query();
        let email = mock!(aws_sdk_sesv2::Client::send_email)
            .then_output(|| SendEmailOutput::builder().build());
        let ddb = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&write, &query]);
        let ses = mock_client!(aws_sdk_sesv2, RuleMode::MatchAny, [&email]);

        process(&ddb, &ses, &settings("", ""), &approval_request())
            .await
            .unwrap();

        assert_eq!(
            write.num_calls(),
            1,
            "the decide endpoint resumes the workflow from the stored token"
        );
        assert_eq!(email.num_calls(), 0, "no sender or recipient is configured");
        assert_eq!(
            query.num_calls(),
            0,
            "new-account detection only feeds the email"
        );
    }

    #[tokio::test]
    async fn enabled_notifications_record_the_task_token_and_email_the_operator() {
        let write = task_token_write();
        let query = active_domains_query();
        let email = mock!(aws_sdk_sesv2::Client::send_email)
            .match_requests(|req| {
                req.from_email_address() == Some("sender@example.com")
                    && req
                        .destination()
                        .is_some_and(|d| d.to_addresses() == ["operator@example.com"])
            })
            .then_output(|| SendEmailOutput::builder().build());
        let ddb = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&write, &query]);
        let ses = mock_client!(aws_sdk_sesv2, RuleMode::MatchAny, [&email]);

        process(
            &ddb,
            &ses,
            &settings("sender@example.com", "operator@example.com"),
            &approval_request(),
        )
        .await
        .unwrap();

        assert_eq!(write.num_calls(), 1);
        assert_eq!(email.num_calls(), 1);
    }

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
