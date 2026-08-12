use std::env;
use std::str::FromStr;

use anyhow::{Context, Result};
use lambda_runtime::{Error, LambdaEvent};
use serde_json::{json, Value};

use konductor_common::api;
use konductor_common::error::{ControlPlaneError, ValidationExceptionReason};
use konductor_common::grant::ResourceRole;
use konductor_common::grant_engine::has_role_binding;
use konductor_common::grant_store::BindingStore;
use konductor_common::principal::{GlobalRole, TrustedPrincipal};
use konductor_common::principal_context::log_principal;
use konductor_common::streams::{EventStream, StreamStatus};
use konductor_common::streams_db;

/// Which side of the subscription relationship the caller is listing from.
/// Inbound is the consumer view (a `subscriber` binding on the domain);
/// outbound is the producer view (a `publisher` binding), with full consumer
/// topology because producer teams need it for notifications and version
/// migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Inbound,
    Outbound,
}

impl FromStr for Direction {
    type Err = ControlPlaneError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "inbound" => Ok(Direction::Inbound),
            "outbound" => Ok(Direction::Outbound),
            other => Err(ControlPlaneError::ValidationException {
                message: format!("direction must be 'inbound' or 'outbound', got '{other}'"),
                reason: ValidationExceptionReason::FieldValidationFailed,
                field_list: None,
            }),
        }
    }
}

impl Direction {
    fn required_role(self) -> ResourceRole {
        match self {
            Direction::Inbound => ResourceRole::Subscriber,
            Direction::Outbound => ResourceRole::Publisher,
        }
    }
}

/// Platform roles read either side without a domain binding (operations and
/// audit); everyone else needs the direction's binding on the domain.
fn is_platform(principal: &TrustedPrincipal) -> bool {
    principal.is_platform_admin()
        || principal
            .global_roles()
            .contains(&GlobalRole::PlatformAuditor)
}

pub async fn handle(
    ddb: &aws_sdk_dynamodb::Client,
    event: LambdaEvent<Value>,
) -> Result<Value, Error> {
    let table = env::var("EVENT_STREAMS_TABLE").context("EVENT_STREAMS_TABLE not set")?;
    let bindings_table = env::var("BINDINGS_TABLE").context("BINDINGS_TABLE not set")?;

    let request_context = event.payload.get("requestContext").unwrap_or(&Value::Null);
    let principal = match log_principal(request_context) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "list subscriptions: deny (no trusted principal)");
            return Ok(api::from_control_plane_error(
                &ControlPlaneError::AccessDenied,
            ));
        }
    };

    let query = match parse_query(&event.payload) {
        Ok(q) => q,
        Err(e) => return Ok(api::from_control_plane_error(&e)),
    };

    if !authorize_direction(ddb, &bindings_table, &principal, &query).await {
        return Ok(api::from_control_plane_error(
            &ControlPlaneError::AccessDenied,
        ));
    }

    let result = match query.direction {
        Direction::Inbound => {
            streams_db::query_streams_by_consumer(ddb, &table, &query.domain).await
        }
        Direction::Outbound => {
            streams_db::query_streams_by_producer(ddb, &table, &query.domain).await
        }
    };

    match result {
        Ok(streams) => {
            let items: Vec<Value> = streams
                .iter()
                .filter(|s| matches_filters(s, &query.schema, &query.version, &query.status))
                .map(stream_summary)
                .collect();
            Ok(api::success_response(200, &json!({ "items": items })))
        }
        Err(e) => {
            tracing::error!(error = %e, "event streams query failed");
            Ok(api::from_control_plane_error(&ControlPlaneError::DynamoDb(
                e.to_string(),
            )))
        }
    }
}

struct ListQuery {
    domain: String,
    direction: Direction,
    schema: Option<String>,
    version: Option<i64>,
    status: Option<StreamStatus>,
}

/// Platform roles read either side unbound; everyone else needs the
/// direction's binding on the listed domain.
async fn authorize_direction(
    ddb: &aws_sdk_dynamodb::Client,
    bindings_table: &str,
    principal: &TrustedPrincipal,
    query: &ListQuery,
) -> bool {
    if is_platform(principal) {
        return true;
    }
    let role = query.direction.required_role();
    let allowed = has_role_binding(
        &BindingStore::new(ddb.clone(), bindings_table),
        principal,
        role,
        &query.domain,
    )
    .await;
    if !allowed {
        tracing::warn!(
            domain = query.domain,
            "list subscriptions: deny (no {} binding)",
            role.as_str()
        );
    }
    allowed
}

fn parse_query(payload: &Value) -> Result<ListQuery, ControlPlaneError> {
    let domain = api::query_param(payload, "domain")
        .ok_or_else(|| api::validation_error("domain query parameter is required"))?;
    let direction = match api::query_param(payload, "direction") {
        Some(raw) => Direction::from_str(&raw)?,
        None => {
            return Err(api::validation_error(
                "direction query parameter is required (inbound or outbound)",
            ))
        }
    };
    Ok(ListQuery {
        domain,
        direction,
        schema: api::query_param(payload, "schema"),
        version: api::query_param(payload, "version").and_then(|v| v.parse::<i64>().ok()),
        status: api::query_param(payload, "status").and_then(|v| v.parse::<StreamStatus>().ok()),
    })
}

fn matches_filters(
    stream: &EventStream,
    schema: &Option<String>,
    version: &Option<i64>,
    status: &Option<StreamStatus>,
) -> bool {
    if let Some(schema) = schema {
        if &stream.schema_name != schema {
            return false;
        }
    }
    if let Some(status) = status {
        if &stream.status != status {
            return false;
        }
    }
    if let Some(version) = version {
        let pinned_anywhere = stream
            .environments
            .values()
            .any(|env| env.pinned_version == Some(*version));
        if !pinned_anywhere {
            return false;
        }
    }
    true
}

fn stream_summary(stream: &EventStream) -> Value {
    json!({
        "subscriptionId": stream.subscription_id().to_string(),
        "consumerDomain": stream.consumer_domain,
        "producerDomain": stream.producer_domain,
        "schemaName": stream.schema_name,
        "status": stream.status,
        "environments": stream.environments,
        "createdAt": stream.created_at.to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
    use aws_sdk_dynamodb::operation::query::QueryOutput;
    use aws_sdk_dynamodb::types::AttributeValue;
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use chrono::Utc;
    use lambda_runtime::Context as LambdaContext;

    use konductor_common::grant::{BindingPrincipal, RoleBinding};
    use konductor_common::grant_store::binding_to_item;
    use konductor_common::streams::EnvStream;
    use konductor_common::streams_db::stream_to_item;

    fn set_env() {
        std::env::set_var("EVENT_STREAMS_TABLE", "streams");
        std::env::set_var("BINDINGS_TABLE", "bindings");
    }

    fn oauth_request_context(team_groups: &[&str], global_roles: &[&str]) -> Value {
        json!({
            "authorizer": {
                "authenticationMechanism": "oauth",
                "principalId": "user-1",
                "issuer": "https://idp.example.com",
                "clientId": "client-1",
                "globalRoles": serde_json::to_string(global_roles).unwrap(),
                "teamGroups": serde_json::to_string(team_groups).unwrap(),
            }
        })
    }

    fn iam_request_context(account_id: &str) -> Value {
        json!({
            "identity": {
                "accountId": account_id,
                "userArn": format!("arn:aws:iam::{account_id}:role/operator"),
            }
        })
    }

    fn request(context: Value, params: Value) -> LambdaEvent<Value> {
        LambdaEvent::new(
            json!({
                "requestContext": context,
                "queryStringParameters": params,
            }),
            LambdaContext::default(),
        )
    }

    fn stream(consumer: &str, detail_type: &str, status: StreamStatus, pinned: i64) -> EventStream {
        let mut environments = HashMap::new();
        environments.insert(
            "prod".to_string(),
            EnvStream {
                pinned_version: Some(pinned),
                rule_name: format!("{consumer}-{detail_type}"),
                rule_arn: None,
                enabled: true,
                status: StreamStatus::Active,
            },
        );
        EventStream {
            producer_domain: "orders".to_string(),
            consumer_domain: consumer.to_string(),
            source: "acme-orders".to_string(),
            detail_type: detail_type.to_string(),
            schema_name: format!("acme-orders@{detail_type}"),
            status,
            environments,
            request_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
            expires_at: None,
        }
    }

    fn binding_rule(principal: BindingPrincipal, role: ResourceRole, domain: &str) -> Rule {
        let binding = RoleBinding::new("b-1", principal, role, domain, Utc::now(), "test").unwrap();
        let item = binding_to_item(&binding);
        let expected = item.clone();
        mock!(aws_sdk_dynamodb::Client::get_item)
            .match_requests(move |req| {
                let key = req.key().unwrap();
                key.get("PK").and_then(|v| v.as_s().ok())
                    == expected.get("PK").and_then(|v| v.as_s().ok())
                    && key.get("SK").and_then(|v| v.as_s().ok())
                        == expected.get("SK").and_then(|v| v.as_s().ok())
            })
            .then_output(move || {
                GetItemOutput::builder()
                    .set_item(Some(item.clone()))
                    .build()
            })
    }

    /// Streams query whose items are the given streams; matches by whichever
    /// direction key the handler chose (consumer GSI or producer partition).
    fn streams_rule(
        key_name: &'static str,
        key_value: &'static str,
        streams: Vec<EventStream>,
    ) -> Rule {
        let items: Vec<HashMap<String, AttributeValue>> =
            streams.iter().map(stream_to_item).collect();
        mock!(aws_sdk_dynamodb::Client::query)
            .match_requests(move |req| {
                req.expression_attribute_values()
                    .and_then(|v| v.get(key_name))
                    .and_then(|v| v.as_s().ok())
                    .map(|v| v == key_value)
                    .unwrap_or(false)
            })
            .then_output(move || {
                QueryOutput::builder()
                    .set_items(Some(items.clone()))
                    .build()
            })
    }

    fn ddb_client(rules: &[&Rule]) -> aws_sdk_dynamodb::Client {
        mock_client!(aws_sdk_dynamodb, RuleMode::MatchAny, rules)
    }

    fn body(response: &Value) -> Value {
        serde_json::from_str(response["body"].as_str().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn oauth_inbound_lists_the_consumer_domains_subscriptions() {
        set_env();
        let binding = binding_rule(
            BindingPrincipal::cognito_group("https://idp.example.com", "team-checkout").unwrap(),
            ResourceRole::Subscriber,
            "checkout",
        );
        let streams = streams_rule(
            ":consumer",
            "checkout",
            vec![stream(
                "checkout",
                "OrderNotification",
                StreamStatus::Active,
                3,
            )],
        );
        let ddb = ddb_client(&[&binding, &streams]);

        let resp = handle(
            &ddb,
            request(
                oauth_request_context(&["team-checkout"], &[]),
                json!({ "domain": "checkout", "direction": "inbound" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 200);
        let items = body(&resp)["items"].clone();
        assert_eq!(items.as_array().unwrap().len(), 1);
        assert_eq!(items[0]["consumerDomain"], "checkout");
    }

    #[tokio::test]
    async fn iam_outbound_lists_the_producers_full_consumer_topology() {
        set_env();
        let binding = binding_rule(
            BindingPrincipal::aws_account("111122223333").unwrap(),
            ResourceRole::Publisher,
            "orders",
        );
        let streams = streams_rule(
            ":producer",
            "orders",
            vec![
                stream("checkout", "OrderNotification", StreamStatus::Active, 3),
                stream("billing", "OrderNotification", StreamStatus::Active, 2),
            ],
        );
        let ddb = ddb_client(&[&binding, &streams]);

        let resp = handle(
            &ddb,
            request(
                iam_request_context("111122223333"),
                json!({ "domain": "orders", "direction": "outbound" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 200);
        let items = body(&resp)["items"].clone();
        assert_eq!(items.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn platform_auditor_reads_without_a_binding() {
        set_env();
        let streams = streams_rule(
            ":producer",
            "orders",
            vec![stream(
                "checkout",
                "OrderNotification",
                StreamStatus::Active,
                3,
            )],
        );
        // No binding rule: an unexpected binding lookup fails the mock client.
        let ddb = ddb_client(&[&streams]);

        let resp = handle(
            &ddb,
            request(
                oauth_request_context(&[], &["platform-auditor"]),
                json!({ "domain": "orders", "direction": "outbound" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 200);
        assert_eq!(body(&resp)["items"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn missing_direction_is_a_validation_error() {
        set_env();
        let ddb = ddb_client(&[]);
        let resp = handle(
            &ddb,
            request(
                iam_request_context("111122223333"),
                json!({ "domain": "checkout" }),
            ),
        )
        .await
        .unwrap();
        assert_eq!(resp["statusCode"], 400);
        assert_eq!(body(&resp)["code"], "ValidationException");
    }

    #[tokio::test]
    async fn invalid_direction_is_a_validation_error() {
        set_env();
        let ddb = ddb_client(&[]);
        let resp = handle(
            &ddb,
            request(
                iam_request_context("111122223333"),
                json!({ "domain": "checkout", "direction": "sideways" }),
            ),
        )
        .await
        .unwrap();
        assert_eq!(resp["statusCode"], 400);
    }

    #[tokio::test]
    async fn no_binding_is_forbidden_not_hidden() {
        set_env();
        let no_binding = mock!(aws_sdk_dynamodb::Client::get_item)
            .then_output(|| GetItemOutput::builder().build());
        let ddb = ddb_client(&[&no_binding]);

        let resp = handle(
            &ddb,
            request(
                iam_request_context("999900001111"),
                json!({ "domain": "checkout", "direction": "inbound" }),
            ),
        )
        .await
        .unwrap();

        assert_eq!(resp["statusCode"], 403);
        assert_eq!(body(&resp)["code"], "AccessDeniedException");
    }

    #[tokio::test]
    async fn schema_version_and_status_filters_narrow_results() {
        set_env();
        let binding = binding_rule(
            BindingPrincipal::aws_account("111122223333").unwrap(),
            ResourceRole::Subscriber,
            "checkout",
        );
        let streams = streams_rule(
            ":consumer",
            "checkout",
            vec![
                stream("checkout", "OrderNotification", StreamStatus::Active, 3),
                stream("checkout", "OrderRefunded", StreamStatus::Active, 1),
                stream("checkout", "OrderDisputed", StreamStatus::Removed, 3),
            ],
        );
        let ddb = ddb_client(&[&binding, &streams]);

        let resp = handle(
            &ddb,
            request(
                iam_request_context("111122223333"),
                json!({
                    "domain": "checkout",
                    "direction": "inbound",
                    "schema": "acme-orders@OrderNotification",
                    "version": "3",
                    "status": "ACTIVE",
                }),
            ),
        )
        .await
        .unwrap();

        let items = body(&resp)["items"].clone();
        assert_eq!(items.as_array().unwrap().len(), 1);
        assert_eq!(items[0]["schemaName"], "acme-orders@OrderNotification");
    }
}
