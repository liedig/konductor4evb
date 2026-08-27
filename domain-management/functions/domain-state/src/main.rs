use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error};
use tracing_subscriber::EnvFilter;

mod handler;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let ddb = aws_sdk_dynamodb::Client::new(&config);
    let events = aws_sdk_eventbridge::Client::new(&config);
    let ddb_ref = &ddb;
    let events_ref = &events;

    lambda_runtime::run(service_fn(move |event| async move {
        handler::handle(ddb_ref, events_ref, event).await
    }))
    .await
}
