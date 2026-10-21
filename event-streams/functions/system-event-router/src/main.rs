use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error};
use tracing_subscriber::EnvFilter;

use system_event_router::handler;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let clients = std::sync::Arc::new(handler::Clients {
        ddb: aws_sdk_dynamodb::Client::new(&config),
        sns: aws_sdk_sns::Client::new(&config),
    });

    lambda_runtime::run(service_fn(move |event| {
        let clients = std::sync::Arc::clone(&clients);
        async move { handler::handle(&clients, event).await }
    }))
    .await
}
