use aws_sdk_dynamodb::Client as DynamoDbClient;
use lambda_runtime::{Error, LambdaEvent};
use serde_json::Value;
use tracing::info;

use konductor_common::dynamodb::{s, DOMAIN_COUNT_COUNTER_KEY};

pub(crate) struct Clients {
    ddb: DynamoDbClient,
}

impl Clients {
    pub(crate) fn new(config: &aws_config::SdkConfig) -> Self {
        Self {
            ddb: DynamoDbClient::new(config),
        }
    }
}

pub(crate) async fn handle(clients: &Clients, _event: LambdaEvent<Value>) -> Result<(), Error> {
    let table = std::env::var("DOMAINS_TABLE")?;
    let count = fetch_count(&clients.ddb, &table).await?;
    // The DomainCount metric filter and 80%-of-cap alarm in
    // konductor-core-stack.yaml key off this exact message and field.
    info!(count, "domain count report");
    Ok(())
}

async fn fetch_count(ddb: &DynamoDbClient, table: &str) -> Result<u32, Error> {
    let output = ddb
        .get_item()
        .table_name(table)
        .key("domainName", s(DOMAIN_COUNT_COUNTER_KEY))
        .consistent_read(true)
        .send()
        .await?;
    let count = output
        .item()
        .and_then(|item| item.get("count"))
        .and_then(|value| value.as_n().ok())
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(0);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};

    fn get_item_rule(count: Option<u32>) -> Rule {
        mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || match count {
            Some(count) => GetItemOutput::builder()
                .item(
                    "domainName",
                    AttributeValue::S(DOMAIN_COUNT_COUNTER_KEY.to_string()),
                )
                .item("count", AttributeValue::N(count.to_string()))
                .build(),
            None => GetItemOutput::builder().build(),
        })
    }

    #[tokio::test]
    async fn reports_counter_value() {
        let rule = get_item_rule(Some(42));
        let ddb = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&rule]);

        let count = fetch_count(&ddb, "domains").await.unwrap();

        assert_eq!(count, 42);
        assert_eq!(rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn missing_counter_reports_zero() {
        let rule = get_item_rule(None);
        let ddb = mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, [&rule]);

        let count = fetch_count(&ddb, "domains").await.unwrap();

        assert_eq!(count, 0);
    }
}
