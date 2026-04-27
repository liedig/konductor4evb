use anyhow::{Context, Result};
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::{json, Value};
use std::env;

use nexus_common::api;
use nexus_common::dynamodb::{
    attr_to_resource_arns_map, attr_to_string_map, get_s, get_string_list, s,
};
use nexus_common::error::ControlPlaneError;
use nexus_common::models::ApiError;

async fn handler(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let ddb = aws_sdk_dynamodb::Client::new(&config);
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;

    let domain_id = match api::path_param(&event.payload, "domainId") {
        Some(id) => id,
        None => {
            return Ok(api::from_control_plane_error(&api::validation_error(
                "Missing domainId path parameter",
            )));
        }
    };

    let result = ddb
        .get_item()
        .table_name(&table)
        .key("domainId", s(&domain_id))
        .send()
        .await;

    match result {
        Ok(output) => match output.item {
            Some(item) => {
                let resource_arns =
                    attr_to_resource_arns_map(&item, "resourceArns").map(|map| json!(map));
                let environments = get_string_list(&item, "environments").unwrap_or_default();

                let mut body = json!({
                    "domainId": get_s(&item, "domainId"),
                    "domainName": get_s(&item, "domainName"),
                    "description": get_s(&item, "description"),
                    "ownerAccountId": get_s(&item, "ownerAccountId"),
                    "ownerEmail": get_s(&item, "ownerEmail"),
                    "status": get_s(&item, "status"),
                    "environments": environments,
                    "tags": attr_to_string_map(&item, "tags"),
                    "resourceArns": resource_arns,
                    "namespaceParameter": get_s(&item, "namespaceParameter"),
                    "createdAt": get_s(&item, "createdAt"),
                    "updatedAt": get_s(&item, "updatedAt"),
                });

                if let Some(v) = get_s(&item, "decidedBy") {
                    body["decidedBy"] = json!(v);
                }
                if let Some(v) = get_s(&item, "justification") {
                    body["justification"] = json!(v);
                }
                if let Some(v) = get_s(&item, "approvedAt") {
                    body["approvedAt"] = json!(v);
                }
                if let Some(v) = get_s(&item, "rejectedAt") {
                    body["rejectedAt"] = json!(v);
                }
                if let Some(v) = get_s(&item, "errorDetail") {
                    body["errorDetail"] = json!(v);
                }

                Ok(api::success_response(200, &body))
            }
            None => Ok(api::from_control_plane_error(
                &ControlPlaneError::DomainNotFound { domain_id },
            )),
        },
        Err(e) => {
            tracing::error!("DynamoDB error: {e:?}");
            Ok(api::error_response(
                500,
                ApiError {
                    code: "InternalServerException".to_string(),
                    message: "Failed to get domain".to_string(),
                    request_id: None,
                    reason: None,
                    field_list: None,
                },
            ))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    lambda_runtime::run(service_fn(handler)).await
}
