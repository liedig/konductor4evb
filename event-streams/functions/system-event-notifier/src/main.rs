use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error};
use tracing_subscriber::EnvFilter;

use system_event_notifier::handler;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let clients = std::sync::Arc::new(handler::Clients {
        ddb: aws_sdk_dynamodb::Client::new(&config),
        ses: aws_sdk_sesv2::Client::new(&config),
    });

    lambda_runtime::run(service_fn(move |event| {
        let clients = std::sync::Arc::clone(&clients);
        async move { handler::handle(&clients, event).await }
    }))
    .await
}
