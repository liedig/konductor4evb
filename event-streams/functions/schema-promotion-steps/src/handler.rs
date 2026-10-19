//! Step Functions task entrypoint for the promotion workflow. The `action`
//! field of the state-machine input selects the step; every step is
//! idempotent so a retried task converges.

use lambda_runtime::{Error, LambdaEvent};
use serde_json::Value;

use crate::{plan, repin, replay, store::Clients};

pub async fn handle(clients: &Clients, event: LambdaEvent<Value>) -> Result<Value, Error> {
    let input = event.payload;
    match input.get("action").and_then(Value::as_str) {
        Some("plan") => plan::run(clients, &input).await,
        Some("replay") => replay::run(clients, &input).await,
        Some("widen") => repin::widen(clients, &input).await,
        Some("narrow") => repin::narrow(clients, &input).await,
        other => Err(anyhow::anyhow!("unknown action: {other:?}").into()),
    }
}
