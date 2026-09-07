use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error};
use tracing_subscriber::EnvFilter;

mod config;
mod handler;
mod outcome;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let clients = handler::Clients::new(&config);
    let clients_ref = &clients;

    lambda_runtime::run(service_fn(
        move |event: lambda_runtime::LambdaEvent<serde_json::Value>| async move {
            handler::handle(clients_ref, event.payload).await
        },
    ))
    .await
}
