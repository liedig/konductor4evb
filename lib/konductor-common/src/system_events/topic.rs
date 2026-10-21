//! Per-domain system-events SNS topic lifecycle: idempotent ensure, ownership
//! verification, restricted subscribe policy, and ownership-checked delete.
//! The helper performs SNS effects only; callers own any domain-record
//! commits before or after.

use std::collections::BTreeSet;

use super::subscriptions::ensure_email_subscription;
use aws_sdk_sns::error::ProvideErrorMetadata;
use aws_sdk_sns::types::Tag;
use aws_sdk_sns::Client;
use serde_json::{json, Value};
use thiserror::Error;

/// Tag proving the topic is platform-managed.
pub const MANAGED_BY_TAG: &str = "konductor:managed-by";
/// Tag naming the domain the topic belongs to.
pub const DOMAIN_TAG: &str = "konductor:domain";
/// Optional cost-center tag, mirroring the domain record.
pub const COST_CENTER_TAG: &str = "cost-center";
/// Value of [`MANAGED_BY_TAG`] on Konductor-provisioned resources.
pub const MANAGED_BY_VALUE: &str = "konductor";

#[derive(Debug, Error)]
pub enum TopicError {
    /// A topic with the managed name exists without the ownership tags; the
    /// helper never takes it over.
    #[error("topic {0} exists but is not a Konductor-managed system-events topic for this domain")]
    Collision(String),
    /// Delete was requested for a topic the platform does not manage for the
    /// given domain.
    #[error("topic {0} is not managed by Konductor for domain {1}; refusing to delete")]
    NotOwned(String, String),
    #[error(transparent)]
    Sns(#[from] anyhow::Error),
}

/// Deterministic topic name for a domain's system events.
pub fn topic_name(organization: &str, domain: &str) -> String {
    format!("{organization}-{domain}-system-events")
}

/// Deterministic ARN for the domain topic, used to verify recorded values.
pub fn expected_topic_arn(
    partition: &str,
    region: &str,
    account_id: &str,
    organization: &str,
    domain: &str,
) -> String {
    format!(
        "arn:{partition}:sns:{region}:{account_id}:{}",
        topic_name(organization, domain)
    )
}

fn ownership_tags(domain: &str, cost_center: Option<&str>) -> Vec<Tag> {
    let mut tags = vec![
        Tag::builder()
            .key(MANAGED_BY_TAG)
            .value(MANAGED_BY_VALUE)
            .build()
            .expect("tag"),
        Tag::builder()
            .key(DOMAIN_TAG)
            .value(domain)
            .build()
            .expect("tag"),
    ];
    if let Some(cost_center) = cost_center {
        tags.push(
            Tag::builder()
                .key(COST_CENTER_TAG)
                .value(cost_center)
                .build()
                .expect("tag"),
        );
    }
    tags
}

fn arn_parts(topic_arn: &str) -> anyhow::Result<(&str, &str)> {
    let mut parts = topic_arn.splitn(6, ':');
    let _arn = parts.next();
    let partition = parts.next();
    let _service = parts.next();
    let _region = parts.next();
    let account = parts.next();
    match (partition, account) {
        (Some(partition), Some(account)) => Ok((partition, account)),
        _ => Err(anyhow::anyhow!("malformed topic ARN: {topic_arn}")),
    }
}

/// Subscribe-only policy: the topic owner keeps full access; the deduplicated
/// registered accounts may subscribe SQS queues or Lambda functions whose
/// endpoints live in those same accounts, or email endpoints. Email addresses
/// are not ARNs, so sns:Endpoint cannot constrain them — a registered account
/// may subscribe any address (accepted trade-off; payloads are low-sensitivity
/// version facts). HTTPS/SMS and every other protocol stay denied.
pub fn subscribe_policy(topic_arn: &str, accounts: &BTreeSet<String>) -> anyhow::Result<Value> {
    let (partition, owner) = arn_parts(topic_arn)?;
    let principals: Vec<String> = accounts
        .iter()
        .map(|account| format!("arn:{partition}:iam::{account}:root"))
        .collect();
    let mut statements = vec![json!({
        "Sid": "AllowTopicOwner",
        "Effect": "Allow",
        "Principal": { "AWS": format!("arn:{partition}:iam::{owner}:root") },
        "Action": "sns:*",
        "Resource": topic_arn,
    })];
    if !accounts.is_empty() {
        statements.push(json!({
            "Sid": "AllowRegisteredAccountsSqsSubscribe",
            "Effect": "Allow",
            "Principal": { "AWS": principals },
            "Action": "sns:Subscribe",
            "Resource": topic_arn,
            "Condition": {
                "StringEquals": { "sns:Protocol": "sqs" },
                "StringLike": {
                    "sns:Endpoint": accounts
                        .iter()
                        .map(|account| format!("arn:{partition}:sqs:*:{account}:*"))
                        .collect::<Vec<_>>()
                },
            },
        }));
        statements.push(json!({
            "Sid": "AllowRegisteredAccountsLambdaSubscribe",
            "Effect": "Allow",
            "Principal": { "AWS": principals },
            "Action": "sns:Subscribe",
            "Resource": topic_arn,
            "Condition": {
                "StringEquals": { "sns:Protocol": "lambda" },
                "StringLike": {
                    "sns:Endpoint": accounts
                        .iter()
                        .map(|account| format!("arn:{partition}:lambda:*:{account}:function:*"))
                        .collect::<Vec<_>>()
                },
            },
        }));
        statements.push(json!({
            "Sid": "AllowRegisteredAccountsEmailSubscribe",
            "Effect": "Allow",
            "Principal": { "AWS": principals },
            "Action": "sns:Subscribe",
            "Resource": topic_arn,
            "Condition": {
                "StringEquals": { "sns:Protocol": "email" },
            },
        }));
    }
    Ok(json!({ "Version": "2012-10-17", "Statement": statements }))
}

fn is_managed(tags: &[(String, String)], domain: &str) -> bool {
    let get = |key: &str| tags.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    get(MANAGED_BY_TAG) == Some(MANAGED_BY_VALUE) && get(DOMAIN_TAG) == Some(domain)
}

async fn list_tags(sns: &Client, topic_arn: &str) -> anyhow::Result<Vec<(String, String)>> {
    let out = sns
        .list_tags_for_resource()
        .resource_arn(topic_arn)
        .send()
        .await?;
    Ok(out
        .tags()
        .iter()
        .map(|t| (t.key().to_string(), t.value().to_string()))
        .collect())
}

/// Create-or-reconcile the domain topic: create if absent, refuse a colliding
/// unmanaged topic, (re)apply the restricted subscribe policy, upsert the
/// ownership tags (including cost-center convergence), and ensure the default
/// owner email subscription exists. Returns the topic ARN.
pub async fn ensure(
    sns: &Client,
    organization: &str,
    domain: &str,
    accounts: impl IntoIterator<Item = String>,
    cost_center: Option<&str>,
    owner_email: &str,
) -> Result<String, TopicError> {
    let name = topic_name(organization, domain);
    let created = sns
        .create_topic()
        .name(&name)
        .set_tags(Some(ownership_tags(domain, cost_center)))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!(e).context(format!("create topic {name}")))?;
    let topic_arn = created
        .topic_arn()
        .ok_or_else(|| anyhow::anyhow!("create topic {name} returned no ARN"))?
        .to_string();

    let tags = list_tags(sns, &topic_arn)
        .await
        .map_err(|e| e.context(format!("list tags on {topic_arn}")))?;
    if !is_managed(&tags, domain) {
        return Err(TopicError::Collision(topic_arn));
    }

    let accounts: BTreeSet<String> = accounts.into_iter().collect();
    let policy = subscribe_policy(&topic_arn, &accounts)?;
    sns.set_topic_attributes()
        .topic_arn(&topic_arn)
        .attribute_name("Policy")
        .attribute_value(policy.to_string())
        .send()
        .await
        .map_err(|e| anyhow::anyhow!(e).context(format!("set policy on {topic_arn}")))?;

    sns.tag_resource()
        .resource_arn(&topic_arn)
        .set_tags(Some(ownership_tags(domain, cost_center)))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!(e).context(format!("tag {topic_arn}")))?;
    if cost_center.is_none() {
        sns.untag_resource()
            .resource_arn(&topic_arn)
            .tag_keys(COST_CENTER_TAG)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!(e).context(format!("clear cost-center on {topic_arn}")))?;
    }
    ensure_email_subscription(sns, &topic_arn, owner_email).await?;
    Ok(topic_arn)
}

/// Delete the topic only when it is a Konductor-managed topic for this
/// domain. Returns `Ok(false)` when the topic is already gone.
pub async fn delete_if_owned(
    sns: &Client,
    topic_arn: &str,
    domain: &str,
) -> Result<bool, TopicError> {
    let tags = match list_tags(sns, topic_arn).await {
        Ok(tags) => tags,
        Err(error) => {
            let missing = error
                .downcast_ref::<aws_sdk_sns::error::SdkError<
                    aws_sdk_sns::operation::list_tags_for_resource::ListTagsForResourceError,
                >>()
                .and_then(|e| e.code())
                .is_some_and(|code| code == "NotFound");
            if missing {
                return Ok(false);
            }
            return Err(anyhow::anyhow!(error).into());
        }
    };
    if !is_managed(&tags, domain) {
        return Err(TopicError::NotOwned(
            topic_arn.to_string(),
            domain.to_string(),
        ));
    }
    sns.delete_topic()
        .topic_arn(topic_arn)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!(e).context(format!("delete topic {topic_arn}")))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    use aws_smithy_mocks::{mock, mock_client, RuleMode};

    const ORG: &str = "acme";
    const DOMAIN: &str = "checkout";
    const ARN: &str = "arn:aws:sns:eu-west-1:111122223333:acme-checkout-system-events";

    fn managed_tags() -> Vec<aws_sdk_sns::types::Tag> {
        vec![
            Tag::builder()
                .key(MANAGED_BY_TAG)
                .value(MANAGED_BY_VALUE)
                .build()
                .unwrap(),
            Tag::builder()
                .key(DOMAIN_TAG)
                .value(DOMAIN)
                .build()
                .unwrap(),
        ]
    }

    fn created() -> aws_sdk_sns::operation::create_topic::CreateTopicOutput {
        aws_sdk_sns::operation::create_topic::CreateTopicOutput::builder()
            .topic_arn(ARN)
            .build()
    }

    fn tags_out(
        tags: Vec<Tag>,
    ) -> aws_sdk_sns::operation::list_tags_for_resource::ListTagsForResourceOutput {
        aws_sdk_sns::operation::list_tags_for_resource::ListTagsForResourceOutput::builder()
            .set_tags(Some(tags))
            .build()
    }

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
    #[test]
    fn subscribe_policy_restricts_to_registered_accounts_and_supported_protocols() {
        let accounts: BTreeSet<String> = ["111122223333".to_string(), "444455556666".to_string()]
            .into_iter()
            .collect();
        let policy = subscribe_policy(ARN, &accounts).unwrap();
        let rendered = policy.to_string();

        let statements = policy["Statement"].as_array().unwrap();
        assert_eq!(statements.len(), 4, "owner plus one statement per protocol");
        for statement in &statements[1..] {
            assert_eq!(statement["Action"], "sns:Subscribe");
            assert_eq!(statement["Resource"], ARN);
            let principals = statement["Principal"]["AWS"].as_array().unwrap();
            assert_eq!(principals.len(), 2, "accounts deduplicated");
        }
        assert!(rendered.contains("\"sns:Protocol\":\"sqs\""));
        assert!(rendered.contains("\"sns:Protocol\":\"lambda\""));
        assert!(rendered.contains("\"sns:Protocol\":\"email\""));
        assert!(rendered.contains("arn:aws:sqs:*:111122223333:*"));
        assert!(rendered.contains("arn:aws:sqs:*:444455556666:*"));
        assert!(rendered.contains("arn:aws:lambda:*:444455556666:function:*"));
        for denied in ["https", "sms", "sqs:SendMessage", "sns:Publish\""] {
            assert!(
                !rendered.contains(denied),
                "policy must not mention {denied}"
            );
        }
    }

    #[test]
    fn topic_identity_is_deterministic() {
        assert_eq!(topic_name(ORG, DOMAIN), "acme-checkout-system-events");
        assert_eq!(
            expected_topic_arn("aws", "eu-west-1", "111122223333", ORG, DOMAIN),
            ARN
        );
    }

    #[tokio::test]
    async fn ensure_creates_applies_policy_and_converges_tags() {
        let create = mock!(aws_sdk_sns::Client::create_topic)
            .match_requests(|req| {
                req.name() == Some("acme-checkout-system-events")
                    && req
                        .tags()
                        .iter()
                        .any(|t| t.key() == MANAGED_BY_TAG && t.value() == MANAGED_BY_VALUE)
                    && req
                        .tags()
                        .iter()
                        .any(|t| t.key() == COST_CENTER_TAG && t.value() == "cc-9")
            })
            .then_output(created);
        let list = mock!(aws_sdk_sns::Client::list_tags_for_resource)
            .then_output(|| tags_out(managed_tags()));
        let policy = mock!(aws_sdk_sns::Client::set_topic_attributes)
            .match_requests(|req| {
                req.topic_arn() == Some(ARN)
                    && req.attribute_name() == Some("Policy")
                    && req.attribute_value().is_some_and(|v| {
                        let parsed: Value = serde_json::from_str(v).unwrap();
                        parsed["Statement"].as_array().unwrap().len() == 4
                    })
            })
            .then_output(|| {
                aws_sdk_sns::operation::set_topic_attributes::SetTopicAttributesOutput::builder()
                    .build()
            });
        let tag = mock!(aws_sdk_sns::Client::tag_resource)
            .match_requests(|req| {
                req.resource_arn() == Some(ARN)
                    && req
                        .tags()
                        .iter()
                        .any(|t| t.key() == COST_CENTER_TAG && t.value() == "cc-9")
            })
            .then_output(|| {
                aws_sdk_sns::operation::tag_resource::TagResourceOutput::builder().build()
            });
        let untag = mock!(aws_sdk_sns::Client::untag_resource)
            .then_output(|| unreachable!("a cost center is set; nothing to clear"));
        let list_subs = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic)
            .then_output(|| list_subs_out(vec![]));
        let subscribe = subscribe_rule();
        let sns = mock_client!(
            aws_sdk_sns,
            RuleMode::Sequential,
            // untag guards last: with a cost center set it is never called,
            // and Sequential mode cannot skip a mid-list rule.
            &[&create, &list, &policy, &tag, &list_subs, &subscribe, &untag]
        );

        let arn = ensure(
            &sns,
            ORG,
            DOMAIN,
            ["111122223333".to_string(), "444455556666".to_string()],
            Some("cc-9"),
            "owner@example.com",
        )
        .await
        .unwrap();

        assert_eq!(arn, ARN);
        assert_eq!(create.num_calls(), 1);
        assert_eq!(policy.num_calls(), 1);
        assert_eq!(tag.num_calls(), 1);
        assert_eq!(subscribe.num_calls(), 1);
    }

    #[tokio::test]
    async fn ensure_without_cost_center_clears_a_stale_tag() {
        let create = mock!(aws_sdk_sns::Client::create_topic).then_output(created);
        let list = mock!(aws_sdk_sns::Client::list_tags_for_resource)
            .then_output(|| tags_out(managed_tags()));
        let policy = mock!(aws_sdk_sns::Client::set_topic_attributes).then_output(|| {
            aws_sdk_sns::operation::set_topic_attributes::SetTopicAttributesOutput::builder()
                .build()
        });
        let tag = mock!(aws_sdk_sns::Client::tag_resource)
            .match_requests(|req| !req.tags().iter().any(|t| t.key() == COST_CENTER_TAG))
            .then_output(|| {
                aws_sdk_sns::operation::tag_resource::TagResourceOutput::builder().build()
            });
        let untag = mock!(aws_sdk_sns::Client::untag_resource)
            .match_requests(|req| req.tag_keys() == [COST_CENTER_TAG])
            .then_output(|| {
                aws_sdk_sns::operation::untag_resource::UntagResourceOutput::builder().build()
            });
        let list_subs = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic).then_output(|| {
            list_subs_out(vec![email_sub("owner@example.com", "PendingConfirmation")])
        });
        let subscribe = mock!(aws_sdk_sns::Client::subscribe)
            .then_output(|| unreachable!("owner email already subscribed (pending)"));
        let sns = mock_client!(
            aws_sdk_sns,
            RuleMode::Sequential,
            &[&create, &list, &policy, &tag, &untag, &list_subs, &subscribe]
        );

        let arn = ensure(
            &sns,
            ORG,
            DOMAIN,
            ["111122223333".to_string()],
            None,
            "owner@example.com",
        )
        .await
        .unwrap();

        assert_eq!(arn, ARN);
        assert_eq!(untag.num_calls(), 1);
        assert_eq!(subscribe.num_calls(), 0);
    }

    #[tokio::test]
    async fn ensure_refuses_a_colliding_unmanaged_topic() {
        let create = mock!(aws_sdk_sns::Client::create_topic).then_output(created);
        let list =
            mock!(aws_sdk_sns::Client::list_tags_for_resource).then_output(|| tags_out(vec![]));
        let policy = mock!(aws_sdk_sns::Client::set_topic_attributes)
            .then_output(|| unreachable!("never policy a foreign topic"));
        let sns = mock_client!(
            aws_sdk_sns,
            RuleMode::Sequential,
            &[&create, &list, &policy]
        );

        let error = ensure(
            &sns,
            ORG,
            DOMAIN,
            ["111122223333".to_string()],
            None,
            "owner@example.com",
        )
        .await
        .unwrap_err();

        assert!(matches!(error, TopicError::Collision(_)), "{error}");
        assert_eq!(policy.num_calls(), 0);
    }

    #[tokio::test]
    async fn ensure_rerun_is_idempotent() {
        let create = mock!(aws_sdk_sns::Client::create_topic).then_output(created);
        let list = mock!(aws_sdk_sns::Client::list_tags_for_resource)
            .then_output(|| tags_out(managed_tags()));
        let policy = mock!(aws_sdk_sns::Client::set_topic_attributes).then_output(|| {
            aws_sdk_sns::operation::set_topic_attributes::SetTopicAttributesOutput::builder()
                .build()
        });
        let tag = mock!(aws_sdk_sns::Client::tag_resource).then_output(|| {
            aws_sdk_sns::operation::tag_resource::TagResourceOutput::builder().build()
        });
        let untag = mock!(aws_sdk_sns::Client::untag_resource).then_output(|| {
            aws_sdk_sns::operation::untag_resource::UntagResourceOutput::builder().build()
        });
        let list_subs = mock!(aws_sdk_sns::Client::list_subscriptions_by_topic).then_output(|| {
            list_subs_out(vec![email_sub("owner@example.com", "PendingConfirmation")])
        });
        let subscribe = mock!(aws_sdk_sns::Client::subscribe)
            .then_output(|| unreachable!("rerun must not subscribe twice"));
        let sns = mock_client!(
            aws_sdk_sns,
            RuleMode::MatchAny,
            &[&create, &list, &policy, &tag, &untag, &list_subs, &subscribe]
        );

        for _ in 0..2 {
            ensure(
                &sns,
                ORG,
                DOMAIN,
                ["111122223333".to_string()],
                None,
                "owner@example.com",
            )
            .await
            .unwrap();
        }

        assert_eq!(create.num_calls(), 2);
        assert_eq!(policy.num_calls(), 2);
        assert_eq!(subscribe.num_calls(), 0);
    }

    #[tokio::test]
    async fn delete_removes_an_owned_topic() {
        let list = mock!(aws_sdk_sns::Client::list_tags_for_resource)
            .then_output(|| tags_out(managed_tags()));
        let delete = mock!(aws_sdk_sns::Client::delete_topic)
            .match_requests(|req| req.topic_arn() == Some(ARN))
            .then_output(|| {
                aws_sdk_sns::operation::delete_topic::DeleteTopicOutput::builder().build()
            });
        let sns = mock_client!(aws_sdk_sns, RuleMode::Sequential, &[&list, &delete]);

        assert!(delete_if_owned(&sns, ARN, DOMAIN).await.unwrap());
        assert_eq!(delete.num_calls(), 1);
    }

    #[tokio::test]
    async fn delete_tolerates_a_missing_topic() {
        let list = mock!(aws_sdk_sns::Client::list_tags_for_resource).then_error(|| {
            aws_sdk_sns::operation::list_tags_for_resource::ListTagsForResourceError::generic(
                aws_sdk_sns::error::ErrorMetadata::builder()
                    .code("NotFound")
                    .message("topic does not exist")
                    .build(),
            )
        });
        let delete = mock!(aws_sdk_sns::Client::delete_topic)
            .then_output(|| unreachable!("nothing to delete"));
        let sns = mock_client!(aws_sdk_sns, RuleMode::Sequential, &[&list, &delete]);

        assert!(!delete_if_owned(&sns, ARN, DOMAIN).await.unwrap());
        assert_eq!(delete.num_calls(), 0);
    }

    #[tokio::test]
    async fn delete_refuses_a_foreign_topic() {
        let list =
            mock!(aws_sdk_sns::Client::list_tags_for_resource).then_output(|| tags_out(vec![]));
        let delete = mock!(aws_sdk_sns::Client::delete_topic)
            .then_output(|| unreachable!("never delete a foreign topic"));
        let sns = mock_client!(aws_sdk_sns, RuleMode::Sequential, &[&list, &delete]);

        let error = delete_if_owned(&sns, ARN, DOMAIN).await.unwrap_err();

        assert!(matches!(error, TopicError::NotOwned(_, _)), "{error}");
        assert_eq!(delete.num_calls(), 0);
    }
}
