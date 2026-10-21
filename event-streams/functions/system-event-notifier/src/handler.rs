//! Human-readable upgrade emails, separate from machine delivery:
//!
//! - Consumer-facing: `SubscriptionUpgradeStarted`/`SubscriptionUpgraded`
//!   from the public system bus, but only for `PROMOTION` triggers —
//!   authoring re-pins are routine and stay silent.
//! - Producer-facing: `SchemaPromotionFailed` from the internal bus, so a
//!   producer learns a promotion failed (including divergence detected before
//!   any consumer re-pinning).
//!
//! Emails are observational: they may be delayed, duplicated, or arrive out
//! of order, and never carry workflow authority. Machine consumers use the
//! SNS topic; operators use the console and alarms.

use std::env;

use anyhow::{Context, Result};
use aws_sdk_sesv2::error::ProvideErrorMetadata;
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::Value;
use tracing::info;

use konductor_common::dynamodb::s;
use konductor_common::html::escape;

pub struct Clients {
    pub ddb: aws_sdk_dynamodb::Client,
    pub ses: aws_sdk_sesv2::Client,
}

struct Email {
    to_domain: String,
    subject: String,
    html: String,
}

pub async fn handle(clients: &Clients, event: LambdaEvent<Value>) -> Result<(), Error> {
    let detail_type = event
        .payload
        .get("detail-type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let detail = event
        .payload
        .get("detail")
        .cloned()
        .ok_or("missing detail in EventBridge event")?;

    let email = match detail_type {
        "SubscriptionUpgradeStarted" => consumer_start(&detail),
        "SubscriptionUpgraded" => consumer_outcome(&detail),
        "SchemaPromotionFailed" => Some(producer_failure(&detail)?),
        _ => None,
    };
    let Some(email) = email else {
        return Ok(());
    };

    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let from =
        env::var("NOTIFICATION_SENDER_EMAIL").context("NOTIFICATION_SENDER_EMAIL not set")?;
    let item = clients
        .ddb
        .get_item()
        .table_name(&table)
        .key("domainName", s(&email.to_domain))
        .send()
        .await
        .with_context(|| format!("failed to load domain {}", email.to_domain))?
        .item
        .with_context(|| format!("domain {} not found", email.to_domain))?;
    let recipients = konductor_common::ses::recipients_from_item(&item)
        .with_context(|| format!("missing ownerEmail on domain {}", email.to_domain))?;

    if let Err(error) = send(clients, &from, recipients, &email).await {
        let permanent = error
            .downcast_ref::<aws_sdk_sesv2::error::SdkError<
                aws_sdk_sesv2::operation::send_email::SendEmailError,
            >>()
            .and_then(|e| e.as_service_error())
            .and_then(ProvideErrorMetadata::code)
            .is_some_and(konductor_common::ses::is_permanent_failure);
        if permanent {
            tracing::error!(error = %error, "permanent SES failure, acknowledging without retry");
            return Ok(());
        }
        return Err(error)
            .context("failed to send system event email")
            .map_err(Into::into);
    }
    info!(domain = %email.to_domain, detail_type, "system event email sent");
    Ok(())
}

async fn send(clients: &Clients, from: &str, recipients: Vec<String>, email: &Email) -> Result<()> {
    clients
        .ses
        .send_email()
        .from_email_address(from)
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
                                .data(&email.subject)
                                .build()
                                .context("failed to build subject")?,
                        )
                        .body(
                            Body::builder()
                                .html(
                                    Content::builder()
                                        .data(&email.html)
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
        .map(|_| ())
        .map_err(anyhow::Error::new)
}

fn text<'a>(detail: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut value = detail;
    for key in path {
        value = value.get(*key)?;
    }
    value.as_str()
}

fn consumer_start(detail: &Value) -> Option<Email> {
    if text(detail, &["data", "trigger"])? != "PROMOTION" {
        return None;
    }
    let domain = text(detail, &["data", "domain"])?;
    let producer = text(detail, &["data", "producerDomain"]).unwrap_or("unknown");
    let schema = text(detail, &["data", "schemaName"]).unwrap_or("unknown");
    let environment = text(detail, &["data", "environment"]).unwrap_or("all environments");
    let from_version = detail["data"]["fromVersion"].as_i64().unwrap_or_default();
    let requested = detail["data"]["requestedVersion"]
        .as_i64()
        .unwrap_or_default();
    Some(Email {
        to_domain: domain.to_string(),
        subject: format!("Subscription upgrade started: '{domain}'"),
        html: format!(
            r#"<html>
<body>
<h2>Subscription Upgrade Started</h2>
<p>A promotion is upgrading your domain <strong>{}</strong>'s subscription to
<strong>{}</strong> (<strong>{}</strong>) in <strong>{}</strong> from version
{from_version} to version {requested}.</p>
<p>No action is required. The upgrade completes when the promotion finishes
re-pinning; a follow-up email reports the outcome. A newer promotion can
supersede this one.</p>
</body>
</html>"#,
            escape(domain),
            escape(schema),
            escape(producer),
            escape(environment),
        ),
    })
}

fn consumer_outcome(detail: &Value) -> Option<Email> {
    if text(detail, &["data", "trigger"])? != "PROMOTION" {
        return None;
    }
    let domain = text(detail, &["data", "domain"])?;
    let producer = text(detail, &["data", "producerDomain"]).unwrap_or("unknown");
    let schema = text(detail, &["data", "schemaName"]).unwrap_or("unknown");
    let environment = text(detail, &["data", "environment"]).unwrap_or("all environments");
    let outcome = text(detail, &["data", "outcome"]).unwrap_or("COMPLETED");
    let resulting = detail["data"]["resultingVersion"]
        .as_i64()
        .unwrap_or_default();
    let requested = detail["data"]["requestedVersion"]
        .as_i64()
        .unwrap_or_default();
    let (heading, body) = if outcome == "COMPLETED" {
        (
            "Subscription Upgrade Completed",
            format!(
                "your domain <strong>{}</strong>'s subscription to <strong>{}</strong> \
                 (<strong>{}</strong>) in <strong>{}</strong> now runs version {resulting}.",
                escape(domain),
                escape(schema),
                escape(producer),
                escape(environment),
            ),
        )
    } else {
        let reason = text(detail, &["data", "reason"]).unwrap_or("PARTIAL");
        (
            "Subscription Upgrade Partially Applied",
            format!(
                "your domain <strong>{}</strong>'s subscription to <strong>{}</strong> \
                 (<strong>{}</strong>) in <strong>{}</strong> reached version {resulting} of \
                 the requested {requested} (<strong>{}</strong>). A later promotion can \
                 complete the upgrade; no action is required.",
                escape(domain),
                escape(schema),
                escape(producer),
                escape(environment),
                escape(reason),
            ),
        )
    };
    Some(Email {
        to_domain: domain.to_string(),
        subject: format!("{heading}: '{domain}'"),
        html: format!("<html>\n<body>\n<h2>{heading}</h2>\n<p>A promotion finished: {body}</p>\n</body>\n</html>"),
    })
}

fn producer_failure(detail: &Value) -> Result<Email> {
    let domain = detail
        .get("domain")
        .and_then(Value::as_str)
        .context("missing domain in SchemaPromotionFailed")?;
    let schema = detail
        .get("schemaName")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let target = detail
        .get("targetEnvironment")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let to_version = detail["toVersion"].as_i64().unwrap_or_default();
    let reason = detail.get("reason").and_then(Value::as_str);
    let failed_version = detail.get("failedVersion").and_then(Value::as_i64);

    let reason_paragraph = match reason {
        Some("DIVERGED") => "<p><strong>The target environment diverged</strong> from the expected schema content,              so no consumer was re-pinned. Reconcile the environment before retrying the promotion.</p>"
            .to_string(),
        Some(other) => {
            let version_note = failed_version
                .map(|v| format!(" at version {v}"))
                .unwrap_or_default();
            format!(
                "<p>Failure classification: <strong>{}</strong>{version_note}. Retry the promotion \
                 after resolving the cause.</p>",
                escape(other)
            )
        }
        None => "<p>No failure classification was recorded; inspect the promotion before retrying.</p>".to_string(),
    };

    Ok(Email {
        to_domain: domain.to_string(),
        subject: format!("Schema promotion failed: '{schema}' in {target}"),
        html: format!(
            r#"<html>
<body>
<h2>Schema Promotion Failed</h2>
<p>The promotion of <strong>{}</strong> to version {to_version} in
<strong>{}</strong> failed for your domain <strong>{}</strong>.</p>
{reason_paragraph}
</body>
</html>"#,
            escape(schema),
            escape(target),
            escape(domain),
        ),
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use serde_json::json;

    fn set_env() {
        env::set_var("DOMAINS_TABLE", "konductor-domains");
        env::set_var("NOTIFICATION_SENDER_EMAIL", "noreply@example.com");
    }

    fn domain_item(name: &str) -> HashMap<String, AttributeValue> {
        HashMap::from([
            (
                "domainName".to_string(),
                AttributeValue::S(name.to_string()),
            ),
            (
                "ownerEmail".to_string(),
                AttributeValue::S("owner@example.com".to_string()),
            ),
            (
                "secondaryOwnerEmails".to_string(),
                AttributeValue::Ss(vec!["second@example.com".to_string()]),
            ),
        ])
    }

    fn get_domain(name: &'static str) -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(move |req| {
                req.key()
                    .is_some_and(|k| k["domainName"].as_s().is_ok_and(|s| s == name))
            })
            .then_output(move || {
                aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                    .set_item(Some(domain_item(name)))
                    .build()
            })
    }

    fn expect_email(subject_fragment: &'static str, body_fragment: &'static str) -> Rule {
        mock!(aws_sdk_sesv2::Client::send_email)
            .match_requests(move |req| {
                let to = req.destination().unwrap().to_addresses();
                let message = req.content().unwrap().simple().unwrap();
                to == ["owner@example.com", "second@example.com"]
                    && message.subject().unwrap().data().contains(subject_fragment)
                    && message
                        .body()
                        .unwrap()
                        .html()
                        .unwrap()
                        .data()
                        .contains(body_fragment)
            })
            .then_output(|| {
                aws_sdk_sesv2::operation::send_email::SendEmailOutput::builder().build()
            })
    }

    fn clients(ddb_rules: &[&Rule], ses_rules: &[&Rule]) -> Clients {
        Clients {
            ddb: mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, ddb_rules),
            ses: mock_client!(aws_sdk_sesv2, RuleMode::MatchAny, ses_rules),
        }
    }

    fn system_event(detail_type: &str, data: Value) -> LambdaEvent<Value> {
        LambdaEvent::new(
            json!({
                "detail-type": detail_type,
                "source": "konductor.system",
                "detail": { "metadata": {}, "data": data }
            }),
            Default::default(),
        )
    }

    fn promotion_data(trigger: &str, extra: Value) -> Value {
        let mut data = json!({
            "domain": "checkout",
            "environment": "prod",
            "producerDomain": "orders",
            "schemaName": "acme-orders@OrderNotification",
            "trigger": trigger,
            "fromVersion": 3,
            "requestedVersion": 4
        });
        data.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        data
    }

    #[tokio::test]
    async fn consumer_start_email_for_promotion_trigger() {
        set_env();
        let get = get_domain("checkout");
        let send = expect_email("Subscription upgrade started", "from version");
        let clients = clients(&[&get], &[&send]);

        handle(
            &clients,
            system_event(
                "SubscriptionUpgradeStarted",
                promotion_data("PROMOTION", json!({})),
            ),
        )
        .await
        .unwrap();

        assert_eq!(send.num_calls(), 1);
    }

    #[tokio::test]
    async fn consumer_outcome_emails_cover_completed_and_partial() {
        set_env();
        let get = get_domain("checkout");
        let completed = expect_email("Subscription Upgrade Completed", "now runs version 4");
        let partial = expect_email(
            "Subscription Upgrade Partially Applied",
            "reached version 3 of",
        );
        let clients = clients(&[&get], &[&completed, &partial]);

        handle(
            &clients,
            system_event(
                "SubscriptionUpgraded",
                promotion_data(
                    "PROMOTION",
                    json!({ "outcome": "COMPLETED", "resultingVersion": 4 }),
                ),
            ),
        )
        .await
        .unwrap();
        handle(
            &clients,
            system_event(
                "SubscriptionUpgraded",
                promotion_data(
                    "PROMOTION",
                    json!({ "outcome": "PARTIAL", "reason": "REPLAY_PARTIAL", "resultingVersion": 3 }),
                ),
            ),
        )
        .await
        .unwrap();

        assert_eq!(completed.num_calls(), 1);
        assert_eq!(partial.num_calls(), 1);
    }

    #[tokio::test]
    async fn authoring_triggers_never_email() {
        set_env();
        let quiet = mock!(aws_sdk_sesv2::Client::send_email)
            .then_output(|| unreachable!("no email for authoring triggers"));
        let ddb = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| unreachable!("no recipient lookup for authoring"));
        let clients = clients(&[&ddb], &[&quiet]);

        handle(
            &clients,
            system_event(
                "SubscriptionUpgradeStarted",
                promotion_data("AUTHORING", json!({})),
            ),
        )
        .await
        .unwrap();
        handle(
            &clients,
            system_event(
                "SubscriptionUpgraded",
                promotion_data("AUTHORING", json!({})),
            ),
        )
        .await
        .unwrap();

        assert_eq!(quiet.num_calls(), 0);
        assert_eq!(ddb.num_calls(), 0);
    }

    #[tokio::test]
    async fn producer_failure_email_reports_divergence_before_repinning() {
        set_env();
        let get = get_domain("orders");
        let send = expect_email("Schema promotion failed", "diverged");
        let clients = clients(&[&get], &[&send]);
        let event = LambdaEvent::new(
            json!({
                "detail-type": "SchemaPromotionFailed",
                "source": "konductor.schema-management",
                "detail": {
                    "promotionId": "p-1",
                    "domain": "orders",
                    "schemaName": "acme-orders@OrderNotification",
                    "sourceEnvironment": "dev",
                    "targetEnvironment": "prod",
                    "toVersion": 4,
                    "reason": "DIVERGED"
                }
            }),
            Default::default(),
        );

        handle(&clients, event).await.unwrap();

        assert_eq!(send.num_calls(), 1);
    }

    #[tokio::test]
    async fn producer_failure_email_tolerates_absent_optional_detail() {
        set_env();
        let get = get_domain("orders");
        let send = expect_email("Schema promotion failed", "No failure classification");
        let clients = clients(&[&get], &[&send]);
        let event = LambdaEvent::new(
            json!({
                "detail-type": "SchemaPromotionFailed",
                "detail": { "domain": "orders", "schemaName": "s", "targetEnvironment": "prod", "toVersion": 2 }
            }),
            Default::default(),
        );

        handle(&clients, event).await.unwrap();

        assert_eq!(send.num_calls(), 1);
    }

    #[tokio::test]
    async fn permanent_ses_failure_is_acknowledged_transient_propagates() {
        set_env();
        let get = get_domain("checkout");
        let permanent = mock!(aws_sdk_sesv2::Client::send_email).then_error(|| {
            aws_sdk_sesv2::operation::send_email::SendEmailError::generic(
                aws_sdk_sesv2::error::ErrorMetadata::builder()
                    .code("MessageRejected")
                    .message("unverified")
                    .build(),
            )
        });
        let permanent_clients = clients(&[&get], &[&permanent]);
        handle(
            &permanent_clients,
            system_event(
                "SubscriptionUpgradeStarted",
                promotion_data("PROMOTION", json!({})),
            ),
        )
        .await
        .unwrap();

        let transient = mock!(aws_sdk_sesv2::Client::send_email).then_error(|| {
            aws_sdk_sesv2::operation::send_email::SendEmailError::generic(
                aws_sdk_sesv2::error::ErrorMetadata::builder()
                    .code("Throttling")
                    .message("slow down")
                    .build(),
            )
        });
        let transient_clients = clients(&[&get], &[&transient]);
        let error = handle(
            &transient_clients,
            system_event(
                "SubscriptionUpgradeStarted",
                promotion_data("PROMOTION", json!({})),
            ),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("failed to send"), "{error}");
    }

    #[test]
    fn bodies_escape_html() {
        let email = consumer_start(&json!({
            "data": {
                "domain": "check<b>out",
                "producerDomain": "or<ders",
                "schemaName": "s@T",
                "trigger": "PROMOTION",
                "fromVersion": 1,
                "requestedVersion": 2
            }
        }))
        .unwrap();
        assert!(email.html.contains("check&lt;b&gt;out"));
        assert!(!email.html.contains("check<b>out"));
    }
}
