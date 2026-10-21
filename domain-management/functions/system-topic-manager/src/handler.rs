//! Single-domain system-events topic actions for the provisioning workflow
//! and the owner-change reconciliation rule. The helper performs SNS effects
//! only; workflows and the update path own domain-record commits (and only
//! record the ARN once the topic is ready).

use std::collections::HashMap;
use std::env;

use aws_sdk_sns::Client;
use lambda_runtime::{Context, Error, LambdaEvent};
use serde::Deserialize;
use serde_json::{json, Value};

use konductor_common::system_events::{self, TopicError};

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
enum Request {
    Ensure {
        #[serde(rename = "domainName")]
        domain_name: String,
        #[serde(rename = "ownerAccountId")]
        owner_account_id: String,
        #[serde(rename = "environmentAccounts")]
        environment_accounts: Option<HashMap<String, String>>,
        #[serde(rename = "costCenter")]
        cost_center: Option<String>,
        #[serde(rename = "ownerEmail")]
        owner_email: String,
    },
    Delete {
        #[serde(rename = "topicArn")]
        topic_arn: String,
        #[serde(rename = "domainName")]
        domain_name: String,
    },
    ReconcileOwnerEmail {
        #[serde(rename = "domainName")]
        domain_name: String,
        #[serde(rename = "previousOwnerEmail")]
        previous_owner_email: Option<String>,
        #[serde(rename = "ownerEmail")]
        owner_email: String,
    },
}

fn organization() -> Result<String, Error> {
    env::var("ORGANIZATION").map_err(|_| "ORGANIZATION environment variable is required".into())
}

/// Derive the managed topic ARN for a domain from this function's own ARN
/// (`arn:{partition}:lambda:{region}:{account}:function:...`), which supplies
/// the partition, region, and platform account.
fn managed_topic_arn(context: &Context, organization: &str, domain: &str) -> Result<String, Error> {
    let parts: Vec<&str> = context.invoked_function_arn.split(':').collect();
    if parts.len() < 7 || parts.get(2) != Some(&"lambda") {
        return Err(format!(
            "cannot derive platform coordinates from invoked ARN {}",
            context.invoked_function_arn
        )
        .into());
    }
    Ok(system_events::expected_topic_arn(
        parts[1],
        parts[3],
        parts[4],
        organization,
        domain,
    ))
}

pub async fn handle(sns: &Client, event: LambdaEvent<Value>) -> Result<Value, Error> {
    let context = event.context.clone();
    let request: Request = serde_json::from_value(event.payload)
        .map_err(|e| format!("invalid system-topic-manager request: {e}"))?;
    match request {
        Request::Ensure {
            domain_name,
            owner_account_id,
            environment_accounts,
            cost_center,
            owner_email,
        } => {
            let accounts = std::iter::once(owner_account_id)
                .chain(environment_accounts.unwrap_or_default().into_values());
            let topic_arn = system_events::ensure(
                sns,
                &organization()?,
                &domain_name,
                accounts,
                cost_center.as_deref(),
                &owner_email,
            )
            .await
            .map_err(render_error)?;
            Ok(json!({ "topicArn": topic_arn }))
        }
        Request::Delete {
            topic_arn,
            domain_name,
        } => {
            let deleted = system_events::delete_if_owned(sns, &topic_arn, &domain_name)
                .await
                .map_err(render_error)?;
            Ok(json!({ "deleted": deleted }))
        }
        Request::ReconcileOwnerEmail {
            domain_name,
            previous_owner_email,
            owner_email,
        } => {
            let topic_arn = managed_topic_arn(&context, &organization()?, &domain_name)?;
            let outcome = system_events::reconcile_owner_email(
                sns,
                &topic_arn,
                previous_owner_email.as_deref(),
                &owner_email,
            )
            .await
            .map_err(|e| format!("{e:#}"))?;
            Ok(json!({
                "topicArn": topic_arn,
                "subscribed": outcome.subscribed,
                "unsubscribed": outcome.unsubscribed,
                "topicAbsent": outcome.topic_absent,
            }))
        }
    }
}

fn render_error(error: TopicError) -> Error {
    match error {
        TopicError::Collision(arn) => format!("TopicNameCollision: {arn}").into(),
        other => format!("{other}").into(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    use aws_sdk_sns::types::Tag;
    use aws_smithy_mocks::{mock, mock_client, RuleMode};
    use serde_json::json;

    const ARN: &str = "arn:aws:sns:eu-west-1:999988887777:acme-checkout-system-events";

    fn managed_tags() -> Vec<Tag> {
        vec![
            Tag::builder()
                .key("konductor:managed-by")
                .value("konductor")
                .build()
                .unwrap(),
            Tag::builder()
                .key("konductor:domain")
                .value("checkout")
                .build()
                .unwrap(),
        ]
    }

    fn ensure_event(environment_accounts: Option<Value>) -> LambdaEvent<Value> {
        LambdaEvent::new(
            json!({
                "action": "ensure",
                "domainName": "checkout",
                "ownerAccountId": "111122223333",
                "environmentAccounts": environment_accounts,
                "costCenter": "cc-9",
                "ownerEmail": "owner@example.com"
            }),
            Default::default(),
        )
    }

    fn ensure_rules(principals: usize) -> Vec<aws_smithy_mocks::Rule> {
        vec![
            mock!(aws_sdk_sns::Client::create_topic).then_output(|| {
                aws_sdk_sns::operation::create_topic::CreateTopicOutput::builder()
                    .topic_arn(ARN)
                    .build()
            }),
            mock!(aws_sdk_sns::Client::list_tags_for_resource).then_output(|| {
                aws_sdk_sns::operation::list_tags_for_resource::ListTagsForResourceOutput::builder()
                    .set_tags(Some(managed_tags()))
                    .build()
            }),
            mock!(aws_sdk_sns::Client::set_topic_attributes)
                .match_requests(move |req| {
                    let policy: Value =
                        serde_json::from_str(req.attribute_value().unwrap()).unwrap();
                    let subscribe = &policy["Statement"][1];
                    subscribe["Principal"]["AWS"].as_array().unwrap().len() == principals
                })
                .then_output(|| {
                    aws_sdk_sns::operation::set_topic_attributes::SetTopicAttributesOutput::builder(
                    )
                    .build()
                }),
            mock!(aws_sdk_sns::Client::tag_resource).then_output(|| {
                aws_sdk_sns::operation::tag_resource::TagResourceOutput::builder().build()
            }),
            mock!(aws_sdk_sns::Client::list_subscriptions_by_topic).then_output(|| {
                aws_sdk_sns::operation::list_subscriptions_by_topic::ListSubscriptionsByTopicOutput::builder()
                    .build()
            }),
            mock!(aws_sdk_sns::Client::subscribe)
                .match_requests(|req| {
                    req.protocol() == Some("email") && req.endpoint() == Some("owner@example.com")
                })
                .then_output(|| {
                    aws_sdk_sns::operation::subscribe::SubscribeOutput::builder()
                        .subscription_arn("PendingConfirmation")
                        .build()
                }),
        ]
    }

    fn env_guard() {
        env::set_var("ORGANIZATION", "acme");
    }

    #[tokio::test]
    async fn ensure_unions_owner_and_environment_accounts() {
        env_guard();
        let rules = ensure_rules(3);
        let refs: Vec<&aws_smithy_mocks::Rule> = rules.iter().collect();
        let sns = mock_client!(aws_sdk_sns, RuleMode::Sequential, &refs);

        let out = handle(
            &sns,
            ensure_event(Some(
                json!({ "dev": "444455556666", "prod": "555566667777" }),
            )),
        )
        .await
        .unwrap();

        assert_eq!(out["topicArn"], ARN);
    }

    #[tokio::test]
    async fn ensure_dedupes_when_owner_doubles_as_environment_account() {
        env_guard();
        let rules = ensure_rules(1);
        let refs: Vec<&aws_smithy_mocks::Rule> = rules.iter().collect();
        let sns = mock_client!(aws_sdk_sns, RuleMode::Sequential, &refs);

        handle(&sns, ensure_event(Some(json!({ "prod": "111122223333" }))))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn delete_reports_whether_the_topic_was_removed() {
        let list = mock!(aws_sdk_sns::Client::list_tags_for_resource).then_output(|| {
            aws_sdk_sns::operation::list_tags_for_resource::ListTagsForResourceOutput::builder()
                .set_tags(Some(managed_tags()))
                .build()
        });
        let delete = mock!(aws_sdk_sns::Client::delete_topic).then_output(|| {
            aws_sdk_sns::operation::delete_topic::DeleteTopicOutput::builder().build()
        });
        let sns = mock_client!(aws_sdk_sns, RuleMode::Sequential, &[&list, &delete]);
        let event = LambdaEvent::new(
            json!({ "action": "delete", "topicArn": ARN, "domainName": "checkout" }),
            Default::default(),
        );

        let out = handle(&sns, event).await.unwrap();

        assert_eq!(out["deleted"], true);
        assert_eq!(delete.num_calls(), 1);
    }

    #[tokio::test]
    async fn delete_refuses_a_foreign_topic_loudly() {
        let list = mock!(aws_sdk_sns::Client::list_tags_for_resource).then_output(|| {
            aws_sdk_sns::operation::list_tags_for_resource::ListTagsForResourceOutput::builder()
                .set_tags(Some(vec![]))
                .build()
        });
        let delete = mock!(aws_sdk_sns::Client::delete_topic)
            .then_output(|| unreachable!("never delete a foreign topic"));
        let sns = mock_client!(aws_sdk_sns, RuleMode::Sequential, &[&list, &delete]);
        let event = LambdaEvent::new(
            json!({ "action": "delete", "topicArn": ARN, "domainName": "checkout" }),
            Default::default(),
        );

        let error = handle(&sns, event).await.unwrap_err();

        assert!(error.to_string().contains("refusing to delete"), "{error}");
        assert_eq!(delete.num_calls(), 0);
    }
    #[tokio::test]
    async fn reconcile_owner_email_moves_the_default_subscription() {
        env_guard();
        let list_new = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic).then_output(|| {
            aws_sdk_sns::operation::list_subscriptions_by_topic::ListSubscriptionsByTopicOutput::builder()
                .build()
        });
        let subscribe = mock!(aws_sdk_sns::Client::subscribe)
            .match_requests(|req| {
                req.topic_arn() == Some(ARN) && req.endpoint() == Some("new-owner@example.com")
            })
            .then_output(|| {
                aws_sdk_sns::operation::subscribe::SubscribeOutput::builder()
                    .subscription_arn("PendingConfirmation")
                    .build()
            });
        let list_old = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic).then_output(|| {
            aws_sdk_sns::operation::list_subscriptions_by_topic::ListSubscriptionsByTopicOutput::builder()
                .subscriptions(
                    aws_sdk_sns::types::Subscription::builder()
                        .protocol("email")
                        .endpoint("owner@example.com")
                        .subscription_arn(format!("{ARN}:sub-9"))
                        .build(),
                )
                .build()
        });
        let unsubscribe = mock!(aws_sdk_sns::Client::unsubscribe)
            .match_requests(|req| {
                req.subscription_arn()
                    .is_some_and(|a| a.ends_with(":sub-9"))
            })
            .then_output(|| {
                aws_sdk_sns::operation::unsubscribe::UnsubscribeOutput::builder().build()
            });
        let sns = mock_client!(
            aws_sdk_sns,
            RuleMode::Sequential,
            &[&list_new, &subscribe, &list_old, &unsubscribe]
        );
        let mut context = lambda_runtime::Context::default();
        context.invoked_function_arn =
            "arn:aws:lambda:eu-west-1:999988887777:function:konductor-system-topic-manager"
                .to_string();
        let event = LambdaEvent::new(
            json!({
                "action": "reconcileOwnerEmail",
                "domainName": "checkout",
                "previousOwnerEmail": "owner@example.com",
                "ownerEmail": "new-owner@example.com"
            }),
            context,
        );

        let out = handle(&sns, event).await.unwrap();

        assert_eq!(out["topicArn"], ARN);
        assert_eq!(out["subscribed"], true);
        assert_eq!(out["unsubscribed"], true);
        assert_eq!(unsubscribe.num_calls(), 1);
    }
    fn reconcile_event(payload: Value) -> LambdaEvent<Value> {
        let mut context = lambda_runtime::Context::default();
        context.invoked_function_arn =
            "arn:aws:lambda:eu-west-1:999988887777:function:konductor-system-topic-manager"
                .to_string();
        LambdaEvent::new(payload, context)
    }

    #[tokio::test]
    async fn reconcile_accepts_a_null_previous_owner_email() {
        env_guard();
        let list_new = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic).then_output(|| {
            aws_sdk_sns::operation::list_subscriptions_by_topic::ListSubscriptionsByTopicOutput::builder()
                .build()
        });
        let subscribe = mock!(aws_sdk_sns::Client::subscribe).then_output(|| {
            aws_sdk_sns::operation::subscribe::SubscribeOutput::builder()
                .subscription_arn("PendingConfirmation")
                .build()
        });
        let unsubscribe = mock!(aws_sdk_sns::Client::unsubscribe)
            .then_output(|| unreachable!("no previous address, nothing to remove"));
        let sns = mock_client!(
            aws_sdk_sns,
            RuleMode::Sequential,
            &[&list_new, &subscribe, &unsubscribe]
        );
        let event = reconcile_event(json!({
            "action": "reconcileOwnerEmail",
            "domainName": "checkout",
            "previousOwnerEmail": null,
            "ownerEmail": "owner@example.com"
        }));

        let out = handle(&sns, event).await.unwrap();

        assert_eq!(out["subscribed"], true);
        assert_eq!(out["unsubscribed"], false);
        assert_eq!(unsubscribe.num_calls(), 0);
    }

    #[tokio::test]
    async fn reconcile_reports_an_absent_topic_as_a_skip() {
        env_guard();
        let list = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic).then_error(|| {
            aws_sdk_sns::operation::list_subscriptions_by_topic::ListSubscriptionsByTopicError::generic(
                aws_sdk_sns::error::ErrorMetadata::builder()
                    .code("NotFound")
                    .message("topic does not exist")
                    .build(),
            )
        });
        let sns = mock_client!(aws_sdk_sns, RuleMode::Sequential, &[&list]);
        let event = reconcile_event(json!({
            "action": "reconcileOwnerEmail",
            "domainName": "checkout",
            "ownerEmail": "owner@example.com"
        }));

        let out = handle(&sns, event).await.unwrap();

        assert_eq!(out["topicAbsent"], true);
        assert_eq!(out["subscribed"], false);
    }
}
