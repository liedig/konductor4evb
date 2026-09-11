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
    let clients = handler::Clients::new(&config);
    let validators = konductor_common::schema_validation::ValidatorCache::new(
        konductor_common::schema_validation::DEFAULT_VALIDATOR_CACHE_CAPACITY,
    );

    let clients_ref = &clients;
    let validators_ref = &validators;
    lambda_runtime::run(service_fn(move |event| async move {
        handler::handle(clients_ref, validators_ref, event).await
    }))
    .await
}
