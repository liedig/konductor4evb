mod handler;
mod model;

use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::config::retry::RetryConfig;
use aws_sdk_schemas::config::retry::RetryConfig as SchemasRetryConfig;
use lambda_runtime::{service_fn, Error};

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Same measurement procedure as the grant-engine benchmark: one round
    // trip per sample, so latency reflects the service, not retry backoff.
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let ddb = aws_sdk_dynamodb::Client::from_conf(
        aws_sdk_dynamodb::config::Builder::from(&config)
            .retry_config(RetryConfig::standard().with_max_attempts(1))
            .build(),
    );
    let schemas = aws_sdk_schemas::Client::from_conf(
        aws_sdk_schemas::config::Builder::from(&config)
            .retry_config(SchemasRetryConfig::standard().with_max_attempts(1))
            .build(),
    );
    let (ddb_ref, schemas_ref) = (&ddb, &schemas);

    lambda_runtime::run(service_fn(move |event| async move {
        handler::handle(ddb_ref, schemas_ref, event).await
    }))
    .await
}
