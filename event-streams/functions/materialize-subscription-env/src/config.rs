use anyhow::{Context, Result};

use crate::handler::MaterializeInput;

pub(crate) struct Config {
    pub(crate) organization: String,
    pub(crate) streams_table: String,
    pub(crate) entitlements_table: String,
    pub(crate) delivery_role_arn: String,
    pub(crate) platform_account_id: String,
    pub(crate) region: String,
}

impl Config {
    pub(crate) fn from_env() -> Result<Self> {
        let read =
            |name: &str| std::env::var(name).with_context(|| format!("missing env var {name}"));
        Ok(Self {
            organization: read("ORGANIZATION")?,
            streams_table: read("EVENT_STREAMS_TABLE")?,
            entitlements_table: read("ENTITLEMENTS_TABLE")?,
            delivery_role_arn: read("SUBSCRIPTION_DELIVERY_ROLE_ARN")?,
            platform_account_id: read("PLATFORM_ACCOUNT_ID")?,
            region: read("AWS_REGION")?,
        })
    }

    pub(crate) fn producer_registry(&self, input: &MaterializeInput) -> String {
        format!(
            "{}-{}-{}-schema-registry",
            self.organization, input.environment, input.producer_domain
        )
    }

    pub(crate) fn producer_bus(&self, input: &MaterializeInput) -> String {
        format!(
            "{}-{}-{}-event-bus",
            self.organization, input.environment, input.producer_domain
        )
    }

    pub(crate) fn consumer_bus_arn(&self, input: &MaterializeInput) -> String {
        format!(
            "arn:aws:events:{}:{}:event-bus/{}-{}-{}-event-bus",
            self.region,
            input.consumer_account_id,
            self.organization,
            input.environment,
            input.consumer_domain
        )
    }

    pub(crate) fn subscription_dlq_arn(&self, input: &MaterializeInput) -> String {
        format!(
            "arn:aws:sqs:{}:{}:{}-{}-{}-subscription-dlq",
            self.region,
            self.platform_account_id,
            self.organization,
            input.environment,
            input.producer_domain
        )
    }
}
