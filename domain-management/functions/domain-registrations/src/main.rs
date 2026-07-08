use lambda_runtime::{service_fn, Error};
use tracing_subscriber::EnvFilter;

mod handler;
mod register;
mod update;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    lambda_runtime::run(service_fn(handler::handle)).await
}
