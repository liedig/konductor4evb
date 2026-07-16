use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use nexus_common::api;
use nexus_common::dynamodb::{
    attr_to_resource_arns_map, attr_to_string_map, get_s, get_string_list, get_string_set, s,
};
use nexus_common::error::ControlPlaneError;
use serde_json::{json, Value};
use std::env;

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    Ok(fetch_domain(ddb, &table, &event.payload).await)
}

async fn fetch_domain(ddb: &aws_sdk_dynamodb::Client, table: &str, event: &Value) -> Value {
    let domain_name = match api::path_param(event, "domainName") {
        Some(name) => name,
        None => {
            return api::from_control_plane_error(&api::validation_error(
                "Missing domainName path parameter",
            ));
        }
    };

    let result = ddb
        .get_item()
        .table_name(table)
        .key("domainName", s(&domain_name))
        .send()
        .await;

    match result {
        Ok(output) => match output.item {
            Some(item) => {
                let resource_arns =
                    attr_to_resource_arns_map(&item, "resourceArns").map(|map| json!(map));
                let environments = get_string_list(&item, "environments").unwrap_or_default();
                let secondary_owner_emails = get_string_set(&item, "secondaryOwnerEmails");

                let mut body = json!({
                    "domainName": get_s(&item, "domainName"),
                    "description": get_s(&item, "description"),
                    "ownerAccountId": get_s(&item, "ownerAccountId"),
                    "ownerEmail": get_s(&item, "ownerEmail"),
                    "secondaryOwnerEmails": secondary_owner_emails,
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

                api::success_response(200, &body)
            }
            None => {
                api::from_control_plane_error(&ControlPlaneError::DomainNotFound { domain_name })
            }
        },
        Err(e) => {
            tracing::error!(error = %e, "DynamoDB get_item failed");
            api::from_control_plane_error(&ControlPlaneError::DynamoDb(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client};
    use std::collections::HashMap;

    fn domain_item() -> HashMap<String, AttributeValue> {
        HashMap::from([
            ("domainName".to_string(), s("billing")),
            ("description".to_string(), s("Billing domain")),
            ("ownerAccountId".to_string(), s("111122223333")),
            ("ownerEmail".to_string(), s("owner@example.com")),
            ("status".to_string(), s("ACTIVE")),
            ("createdAt".to_string(), s("2024-01-01T00:00:00Z")),
            ("updatedAt".to_string(), s("2024-01-01T00:00:00Z")),
        ])
    }

    #[tokio::test]
    async fn path_param_present_returns_domain() {
        let item = domain_item();
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let event = json!({"pathParameters": {"domainName": "billing"}});
        let resp = fetch_domain(&ddb, "domains", &event).await;

        assert_eq!(resp["statusCode"], 200);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["domainName"], "billing");
        assert_eq!(body["status"], "ACTIVE");
        assert_eq!(body["ownerEmail"], "owner@example.com");
        assert_eq!(get_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn path_param_absent_returns_validation_error() {
        let ddb = mock_client!(aws_sdk_dynamodb, &[]);
        let event = json!({});

        let resp = fetch_domain(&ddb, "domains", &event).await;

        assert_eq!(resp["statusCode"], 400);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "ValidationException");
        assert!(body["message"].as_str().unwrap().contains("domainName"));
    }

    #[tokio::test]
    async fn domain_not_found_returns_404_via_from_control_plane_error() {
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| GetItemOutput::builder().build());
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let event = json!({"pathParameters": {"domainName": "missing"}});
        let resp = fetch_domain(&ddb, "domains", &event).await;

        assert_eq!(resp["statusCode"], 404);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "ResourceNotFoundException");
        assert_eq!(body["message"], "domain not found: missing");
        assert_eq!(get_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn dynamodb_error_maps_to_control_plane_error_dynamo_db() {
        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_error(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemError::InternalServerError(
                aws_sdk_dynamodb::types::error::InternalServerError::builder().build(),
            )
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let event = json!({"pathParameters": {"domainName": "billing"}});
        let resp = fetch_domain(&ddb, "domains", &event).await;

        assert_eq!(resp["statusCode"], 500);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["code"], "InternalServerException");
        assert_eq!(body["message"], "An internal error occurred");
        assert_eq!(get_rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn optional_fields_included_when_present() {
        let mut item = domain_item();
        item.insert("decidedBy".to_string(), s("admin@example.com"));
        item.insert("justification".to_string(), s("approved by team"));

        let get_rule = mock!(aws_sdk_dynamodb::Client::get_item).then_output(move || {
            GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        });
        let ddb = mock_client!(aws_sdk_dynamodb, [&get_rule]);

        let event = json!({"pathParameters": {"domainName": "billing"}});
        let resp = fetch_domain(&ddb, "domains", &event).await;

        assert_eq!(resp["statusCode"], 200);
        let body: Value = serde_json::from_str(resp["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["decidedBy"], "admin@example.com");
        assert_eq!(body["justification"], "approved by team");
    }
}
