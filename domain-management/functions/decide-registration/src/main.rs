use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error};

mod handler;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let sfn = aws_sdk_sfn::Client::new(&config);
    let sfn_ref = &sfn;

    lambda_runtime::run(service_fn(move |event| async move {
        handler::handle(sfn_ref, event).await
    }))
    .await
}
