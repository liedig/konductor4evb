use aws_config::BehaviorVersion;
use lambda_runtime::{service_fn, Error};
use tracing_subscriber::EnvFilter;

mod handler;
mod record;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let clients = handler::Clients::new(&config);

    let bindings_table = std::env::var("BINDINGS_TABLE").expect("BINDINGS_TABLE not set");
    let entitlements_table =
        std::env::var("ENTITLEMENTS_TABLE").expect("ENTITLEMENTS_TABLE not set");
    let authorizer = konductor_common::grant_engine::DynamoGrantEngine::new(
        konductor_common::grant_store::BindingStore::new(clients.ddb.clone(), bindings_table),
        konductor_common::entitlement_store::EntitlementStore::new(
            clients.ddb.clone(),
            entitlements_table,
        ),
    );

    let clients_ref = &clients;
    let authorizer_ref = &authorizer;
    lambda_runtime::run(service_fn(move |event| async move {
        handler::handle(clients_ref, authorizer_ref, event).await
    }))
    .await
}
