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
    let sns = aws_sdk_sns::Client::new(&config);
    let sns_ref = &sns;

    lambda_runtime::run(service_fn(move |event| async move {
        handler::handle(sns_ref, event).await
    }))
    .await
}
