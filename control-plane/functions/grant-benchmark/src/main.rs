mod handler;

use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::config::retry::RetryConfig;
use lambda_runtime::{service_fn, Error};

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // The spike's measurement procedure disables SDK retries on decision reads
    // so latency reflects a single round trip, not retry backoff.
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let ddb = aws_sdk_dynamodb::Client::from_conf(
        aws_sdk_dynamodb::config::Builder::from(&config)
            .retry_config(RetryConfig::standard().with_max_attempts(1))
            .build(),
    );
    let ddb_ref = &ddb;

    lambda_runtime::run(service_fn(move |event| async move {
        handler::handle(ddb_ref, event).await
    }))
    .await
}
