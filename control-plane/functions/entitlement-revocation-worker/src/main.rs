use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error};
use tracing_subscriber::EnvFilter;

use entitlement_revocation_worker::handler;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let clients = handler::Clients::new(&config);
    let clients_ref = &clients;

    // Boxed so the handler future's depth does not overflow the compiler's
    // layout query limit under lambda_runtime 1.x's deeper service stack.
    lambda_runtime::run(service_fn(move |event| {
        Box::pin(handler::handle(clients_ref, event))
    }))
    .await
}
