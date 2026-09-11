mod contract;
mod handler;
mod preview;

use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error};

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let ddb = aws_sdk_dynamodb::Client::new(&config);
    let schemas = aws_sdk_schemas::Client::new(&config);
    let ddb_ref = &ddb;
    let schemas_ref = &schemas;

    lambda_runtime::run(service_fn(move |event| async move {
        handler::handle(ddb_ref, schemas_ref, event).await
    }))
    .await
}
