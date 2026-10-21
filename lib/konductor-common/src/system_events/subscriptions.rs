//! Default owner-email subscription management for the per-domain
//! system-events topic: idempotent ensure, removal that tolerates SNS's
//! pending-confirmation state, and owner-change reconciliation.

use aws_sdk_sns::error::ProvideErrorMetadata;
use aws_sdk_sns::Client;

/// Subscription ARN SNS reports while an email subscription awaits
/// confirmation. It is not a real ARN and cannot be unsubscribed; unconfirmed
/// subscriptions expire on their own.
const PENDING_CONFIRMATION: &str = "PendingConfirmation";

/// Outcome of [`reconcile_owner_email`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerEmailReconciliation {
    /// The owner address was newly subscribed.
    pub subscribed: bool,
    /// The previous default address was unsubscribed.
    pub unsubscribed: bool,
    /// The domain has no system-events topic; nothing to reconcile.
    pub topic_absent: bool,
}

async fn find_email_subscription(
    sns: &Client,
    topic_arn: &str,
    email: &str,
) -> anyhow::Result<Option<String>> {
    let mut token = None;
    loop {
        let out = sns
            .list_subscriptions_by_topic()
            .topic_arn(topic_arn)
            .set_next_token(token)
            .send()
            .await
            .map_err(|e| {
                anyhow::anyhow!(e).context(format!("list subscriptions on {topic_arn}"))
            })?;
        for sub in out.subscriptions() {
            if sub.protocol() == Some("email") && sub.endpoint() == Some(email) {
                return Ok(sub.subscription_arn().map(str::to_string));
            }
        }
        token = out.next_token().map(str::to_string);
        if token.is_none() {
            return Ok(None);
        }
    }
}

/// Subscribe `email` to the topic unless an email subscription for that
/// address already exists (confirmed or pending). Returns `true` when a new
/// subscription was created. SNS requires the address owner to confirm before
/// delivery starts; a pending subscription is a normal outcome.
pub async fn ensure_email_subscription(
    sns: &Client,
    topic_arn: &str,
    email: &str,
) -> anyhow::Result<bool> {
    if find_email_subscription(sns, topic_arn, email)
        .await?
        .is_some()
    {
        return Ok(false);
    }
    sns.subscribe()
        .topic_arn(topic_arn)
        .protocol("email")
        .endpoint(email)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!(e).context(format!("subscribe {email} to {topic_arn}")))?;
    Ok(true)
}

/// Remove the email subscription for `email` when present. A pending
/// subscription cannot be unsubscribed (SNS reports no real ARN for it) and
/// is left to expire. Returns `true` when a subscription was removed.
pub async fn remove_email_subscription(
    sns: &Client,
    topic_arn: &str,
    email: &str,
) -> anyhow::Result<bool> {
    let Some(arn) = find_email_subscription(sns, topic_arn, email).await? else {
        return Ok(false);
    };
    if arn == PENDING_CONFIRMATION {
        tracing::warn!(
            topic = topic_arn,
            "owner email subscription is pending confirmation; leaving it to expire"
        );
        return Ok(false);
    }
    sns.unsubscribe()
        .subscription_arn(&arn)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!(e).context(format!("unsubscribe {arn}")))?;
    Ok(true)
}

/// Move the topic's default email subscription from the previous owner
/// address to the current one. A missing topic (a domain that predates system
/// events) is a skip, not an error.
pub async fn reconcile_owner_email(
    sns: &Client,
    topic_arn: &str,
    previous_owner_email: Option<&str>,
    owner_email: &str,
) -> anyhow::Result<OwnerEmailReconciliation> {
    match ensure_email_subscription(sns, topic_arn, owner_email).await {
        Ok(subscribed) => {
            let unsubscribed = match previous_owner_email {
                Some(previous) if previous != owner_email => {
                    remove_email_subscription(sns, topic_arn, previous).await?
                }
                _ => false,
            };
            Ok(OwnerEmailReconciliation {
                subscribed,
                unsubscribed,
                topic_absent: false,
            })
        }
        Err(error) => {
            let missing = error
                .downcast_ref::<aws_sdk_sns::error::SdkError<
                    aws_sdk_sns::operation::list_subscriptions_by_topic::ListSubscriptionsByTopicError,
                >>()
                .and_then(|e| e.code())
                .is_some_and(|code| code == "NotFound");
            if missing {
                return Ok(OwnerEmailReconciliation {
                    subscribed: false,
                    unsubscribed: false,
                    topic_absent: true,
                });
            }
            Err(error)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    use aws_smithy_mocks::{mock, mock_client, RuleMode};

    const ARN: &str = "arn:aws:sns:eu-west-1:111122223333:acme-checkout-system-events";

    fn email_sub(endpoint: &str, arn: &str) -> aws_sdk_sns::types::Subscription {
        aws_sdk_sns::types::Subscription::builder()
            .protocol("email")
            .endpoint(endpoint)
            .subscription_arn(arn)
            .topic_arn(ARN)
            .build()
    }

    fn list_subs_out(
        subs: Vec<aws_sdk_sns::types::Subscription>,
    ) -> aws_sdk_sns::operation::list_subscriptions_by_topic::ListSubscriptionsByTopicOutput {
        aws_sdk_sns::operation::list_subscriptions_by_topic::ListSubscriptionsByTopicOutput::builder()
            .set_subscriptions(Some(subs))
            .build()
    }

    fn subscribe_rule() -> aws_smithy_mocks::Rule {
        mock!(aws_sdk_sns::Client::subscribe)
            .match_requests(|req| {
                req.protocol() == Some("email") && req.endpoint() == Some("owner@example.com")
            })
            .then_output(|| {
                aws_sdk_sns::operation::subscribe::SubscribeOutput::builder()
                    .subscription_arn("PendingConfirmation")
                    .build()
            })
    }

    #[tokio::test]
    async fn reconcile_moves_the_default_subscription_to_the_new_owner() {
        let list_new = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic)
            .then_output(|| list_subs_out(vec![]));
        let subscribe = subscribe_rule();
        let list_old = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic).then_output(|| {
            list_subs_out(vec![email_sub(
                "previous@example.com",
                "arn:aws:sns:eu-west-1:111122223333:acme-checkout-system-events:sub-1",
            )])
        });
        let unsubscribe = mock!(aws_sdk_sns::Client::unsubscribe)
            .match_requests(|req| {
                req.subscription_arn()
                    .is_some_and(|a| a.ends_with(":sub-1"))
            })
            .then_output(|| {
                aws_sdk_sns::operation::unsubscribe::UnsubscribeOutput::builder().build()
            });
        let sns = mock_client!(
            aws_sdk_sns,
            RuleMode::Sequential,
            &[&list_new, &subscribe, &list_old, &unsubscribe]
        );

        let outcome =
            reconcile_owner_email(&sns, ARN, Some("previous@example.com"), "owner@example.com")
                .await
                .unwrap();

        assert_eq!(
            outcome,
            OwnerEmailReconciliation {
                subscribed: true,
                unsubscribed: true,
                topic_absent: false,
            }
        );
        assert_eq!(unsubscribe.num_calls(), 1);
    }

    #[tokio::test]
    async fn reconcile_leaves_a_pending_previous_subscription_to_expire() {
        let list_new = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic)
            .then_output(|| list_subs_out(vec![]));
        let subscribe = subscribe_rule();
        let list_old = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic).then_output(|| {
            list_subs_out(vec![email_sub(
                "previous@example.com",
                "PendingConfirmation",
            )])
        });
        let unsubscribe = mock!(aws_sdk_sns::Client::unsubscribe)
            .then_output(|| unreachable!("a pending subscription has no ARN to unsubscribe"));
        let sns = mock_client!(
            aws_sdk_sns,
            RuleMode::Sequential,
            &[&list_new, &subscribe, &list_old, &unsubscribe]
        );

        let outcome =
            reconcile_owner_email(&sns, ARN, Some("previous@example.com"), "owner@example.com")
                .await
                .unwrap();

        assert!(outcome.subscribed);
        assert!(!outcome.unsubscribed);
        assert_eq!(unsubscribe.num_calls(), 0);
    }

    #[tokio::test]
    async fn reconcile_skips_a_domain_without_a_topic() {
        let list = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic).then_error(|| {
            aws_sdk_sns::operation::list_subscriptions_by_topic::ListSubscriptionsByTopicError::generic(
                aws_sdk_sns::error::ErrorMetadata::builder()
                    .code("NotFound")
                    .message("topic does not exist")
                    .build(),
            )
        });
        let subscribe = mock!(aws_sdk_sns::Client::subscribe)
            .then_output(|| unreachable!("no topic, nothing to subscribe"));
        let sns = mock_client!(aws_sdk_sns, RuleMode::Sequential, &[&list, &subscribe]);

        let outcome = reconcile_owner_email(&sns, ARN, None, "owner@example.com")
            .await
            .unwrap();

        assert_eq!(
            outcome,
            OwnerEmailReconciliation {
                subscribed: false,
                unsubscribed: false,
                topic_absent: true,
            }
        );
    }

    #[tokio::test]
    async fn reconcile_propagates_non_notfound_list_errors() {
        let list = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic).then_error(|| {
            aws_sdk_sns::operation::list_subscriptions_by_topic::ListSubscriptionsByTopicError::generic(
                aws_sdk_sns::error::ErrorMetadata::builder()
                    .code("AccessDeniedException")
                    .message("denied")
                    .build(),
            )
        });
        let sns = mock_client!(aws_sdk_sns, RuleMode::Sequential, &[&list]);

        let error = reconcile_owner_email(&sns, ARN, None, "owner@example.com")
            .await
            .unwrap_err();

        assert!(format!("{error:#}").contains("AccessDenied"), "{error:#}");
    }

    #[tokio::test]
    async fn find_email_subscription_follows_pagination() {
        let page1 = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic)
            .match_requests(|req| req.next_token().is_none())
            .then_output(|| {
                aws_sdk_sns::operation::list_subscriptions_by_topic::ListSubscriptionsByTopicOutput::builder()
                    .subscriptions(email_sub("other@example.com", "arn:aws:sns:eu-west-1:111122223333:acme-checkout-system-events:sub-1"))
                    .next_token("page2")
                    .build()
            });
        let page2 = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic)
            .match_requests(|req| req.next_token() == Some("page2"))
            .then_output(|| {
                list_subs_out(vec![email_sub(
                    "owner@example.com",
                    "arn:aws:sns:eu-west-1:111122223333:acme-checkout-system-events:sub-2",
                )])
            });
        let subscribe = mock!(aws_sdk_sns::Client::subscribe)
            .then_output(|| unreachable!("the owner subscription is on page 2"));
        let sns = mock_client!(
            aws_sdk_sns,
            RuleMode::Sequential,
            &[&page1, &page2, &subscribe]
        );

        let subscribed = ensure_email_subscription(&sns, ARN, "owner@example.com")
            .await
            .unwrap();

        assert!(!subscribed, "existing page-2 subscription must be found");
        assert_eq!(page1.num_calls(), 1);
        assert_eq!(page2.num_calls(), 1);
        assert_eq!(subscribe.num_calls(), 0);
    }
}
