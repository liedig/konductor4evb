use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::Value;

mod claims;
mod error;
mod handler;
mod jwks;
mod policy;

use handler::AuthorizerConfig;
use jwks::CachingResolver;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = std::sync::Arc::new(AuthorizerConfig::from_env()?);

    let http = reqwest::Client::new();
    let jwks_uri = config.jwks_uri.clone();
    let resolver = std::sync::Arc::new(CachingResolver::new(move || {
        let http = http.clone();
        let jwks_uri = jwks_uri.clone();
        async move { jwks::fetch_jwks(&http, &jwks_uri).await }
    }));

    lambda_runtime::run(service_fn(move |event: LambdaEvent<Value>| {
        let config = config.clone();
        let resolver = resolver.clone();
        async move { handler::handle(resolver.as_ref(), config.as_ref(), event.payload).await }
    }))
    .await
}
