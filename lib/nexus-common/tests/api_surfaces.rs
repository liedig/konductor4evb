//! Contract and template tests for the separated OAuth and AWS_IAM API
//! surfaces (Section 4). These reject unintended unauthenticated methods,
//! mixed-auth methods, and IAM-surface approval exposure.

mod support;

use serde_yaml::Value;
use support::{at, boolean, core_template, int, mapping, props, repo_yaml, resource, seq, text};

fn oauth_contract() -> Value {
    repo_yaml("api/openapi-oauth.yaml")
}

fn iam_contract() -> Value {
    repo_yaml("api/openapi-iam.yaml")
}

/// One flattened operation from an OpenAPI document.
struct Op {
    method: String,
    path: String,
    uses_iam: bool,
    explicitly_unauthenticated: bool,
}

const HTTP_METHODS: [&str; 6] = ["get", "post", "put", "patch", "delete", "head"];

fn operations(doc: &Value) -> Vec<Op> {
    let mut out = Vec::new();
    for (path, item) in mapping(at(doc, "paths")) {
        let path = path.as_str().expect("path key").to_string();
        for (method, op) in mapping(item) {
            let method = method.as_str().expect("method key").to_string();
            if !HTTP_METHODS.contains(&method.as_str()) {
                continue;
            }
            let op_map = mapping(op);
            out.push(Op {
                method,
                path: path.clone(),
                uses_iam: op_map.get("x-amazon-apigateway-auth").is_some(),
                explicitly_unauthenticated: op_map
                    .get("security")
                    .map(|s| seq(s).is_empty())
                    .unwrap_or(false),
            });
        }
    }
    out
}

#[test]
fn two_apis_serve_environment_named_stages() {
    let doc = core_template();
    for (logical_id, location) in [
        ("NexusOAuthApi", "./api/openapi-oauth.yaml"),
        ("NexusIamApi", "./api/openapi-iam.yaml"),
    ] {
        let api = resource(&doc, logical_id);
        assert_eq!(text(at(api, "Type")), "AWS::Serverless::Api");
        assert_eq!(text(at(props(api), "StageName")), "dev");
        let include = at(at(props(api), "DefinitionBody"), "Fn::Transform");
        assert_eq!(text(at(at(include, "Parameters"), "Location")), location);

        for (env, suffix) in [("staging", "Staging"), ("prod", "Prod")] {
            let stage = resource(&doc, &format!("{logical_id}{suffix}Stage"));
            assert_eq!(text(at(stage, "Type")), "AWS::ApiGateway::Stage");
            assert_eq!(text(at(props(stage), "StageName")), env);
            assert_eq!(text(at(props(stage), "RestApiId")), logical_id);
            assert_eq!(
                text(at(props(stage), "DeploymentId")),
                format!("{logical_id}.Deployment")
            );
        }
    }
}

#[test]
fn oauth_api_attaches_request_authorizer_with_caching_disabled() {
    let doc = core_template();
    let api = resource(&doc, "NexusOAuthApi");
    let auth = at(props(api), "Auth");
    assert_eq!(
        text(at(auth, "DefaultAuthorizer")),
        "OAuthRequestAuthorizer"
    );
    let authorizer = at(at(auth, "Authorizers"), "OAuthRequestAuthorizer");
    assert_eq!(text(at(authorizer, "FunctionPayloadType")), "REQUEST");
    assert_eq!(
        int(at(authorizer, "AuthorizerResultTtlInSeconds")),
        0,
        "authorizer result caching must be disabled by default"
    );
    let identity = at(authorizer, "Identity");
    let headers: Vec<&str> = seq(at(identity, "Headers")).iter().map(text).collect();
    assert_eq!(
        headers,
        ["Authorization"],
        "REQUEST authorizer identity must use the Headers list (the singular Header key is rejected by the SAM transform)"
    );
    assert_eq!(
        text(at(authorizer, "FunctionArn")),
        "OAuthAuthorizerFunction.Arn"
    );
}

#[test]
fn iam_api_uses_aws_iam_authorization() {
    let doc = core_template();
    let api = resource(&doc, "NexusIamApi");
    let auth = at(props(api), "Auth");
    assert_eq!(text(at(auth, "DefaultAuthorizer")), "AWS_IAM");
}

#[test]
fn approval_route_is_exposed_only_on_the_oauth_surface() {
    let oauth = operations(&oauth_contract());
    let iam = operations(&iam_contract());

    let oauth_decide: Vec<&Op> = oauth
        .iter()
        .filter(|op| op.path == "/domains/registrations/decide")
        .collect();
    assert_eq!(
        oauth_decide.len(),
        1,
        "OAuth must expose exactly one decide op"
    );
    assert_eq!(oauth_decide[0].method, "post");

    assert!(
        iam.iter()
            .all(|op| op.path != "/domains/registrations/decide"),
        "IAM surface must not expose the approval route"
    );
}

#[test]
fn api_contracts_have_no_unauthenticated_or_mixed_methods() {
    for op in operations(&oauth_contract()) {
        assert!(
            !op.explicitly_unauthenticated,
            "OAuth {} {} must not be unauthenticated",
            op.method, op.path
        );
        assert!(
            !op.uses_iam,
            "OAuth {} {} must not also accept AWS_IAM (mixed auth)",
            op.method, op.path
        );
    }
    for op in operations(&iam_contract()) {
        assert!(
            !op.explicitly_unauthenticated,
            "IAM {} {} must not be unauthenticated",
            op.method, op.path
        );
        assert!(
            op.uses_iam,
            "IAM {} {} must use AWS_IAM",
            op.method, op.path
        );
    }
}

#[test]
fn read_functions_are_exposed_on_both_surfaces() {
    let doc = core_template();
    for logical_id in [
        "ListDomainsFunction",
        "DescribeDomainFunction",
        "ListSubscriptionsFunction",
        "DescribeSubscriptionFunction",
        "ListSchemasFunction",
        "DescribeSchemaFunction",
    ] {
        let func = resource(&doc, logical_id);
        let events = mapping(at(props(func), "Events"));
        for event_name in ["OAuthApi", "IamApi"] {
            let event = events
                .get(event_name)
                .unwrap_or_else(|| panic!("{logical_id} missing {event_name} event"));
            assert_eq!(text(at(event, "Type")), "Api");
        }
    }
}

#[test]
fn decide_function_is_oauth_post_only() {
    let doc = core_template();
    let func = resource(&doc, "DecideFunction");
    let events = mapping(at(props(func), "Events"));
    assert_eq!(events.len(), 1, "decide must have exactly one API event");
    let event = at(events.get("ApiPost").expect("ApiPost event"), "Properties");
    assert_eq!(text(at(event, "Method")), "POST");
    assert_eq!(text(at(event, "RestApiId")), "NexusOAuthApi");
    assert!(
        events.get("ApiGet").is_none(),
        "bearer GET decide must be removed"
    );
}

#[test]
fn binding_administration_routes_are_oauth_only() {
    let oauth = operations(&oauth_contract());
    let iam = operations(&iam_contract());

    let mut oauth_ops: Vec<(&str, &str)> = oauth
        .iter()
        .filter(|op| op.path.starts_with("/bindings"))
        .map(|op| (op.method.as_str(), op.path.as_str()))
        .collect();
    oauth_ops.sort();
    assert_eq!(
        oauth_ops,
        [
            ("delete", "/bindings/{bindingId}"),
            ("get", "/bindings"),
            ("post", "/bindings"),
        ],
        "OAuth surface must expose exactly the three binding administration operations"
    );

    assert!(
        iam.iter().all(|op| !op.path.starts_with("/bindings")),
        "IAM surface must not expose binding administration"
    );
}

#[test]
fn binding_admin_function_is_wired_to_the_oauth_surface_only() {
    let doc = core_template();
    let func = resource(&doc, "BindingAdminFunction");
    let events = mapping(at(props(func), "Events"));
    assert_eq!(
        events.len(),
        3,
        "binding admin must have exactly the three route events"
    );
    for (name, event) in events {
        let event_props = at(event, "Properties");
        assert_eq!(
            text(at(event_props, "RestApiId")),
            "NexusOAuthApi",
            "event {:?} must target the OAuth API",
            name
        );
    }
}

#[test]
fn binding_administration_uses_lambda_proxy_not_sqs_templates() {
    let templates = write_templates(&oauth_contract());
    assert!(
        templates
            .iter()
            .all(|(path, _, _)| !path.starts_with("/bindings")),
        "binding administration must be synchronous Lambda proxy, not an SQS command"
    );
}

#[test]
fn bindings_table_matches_documented_schema() {
    let doc = core_template();
    let table = resource(&doc, "AuthorizationBindingsTable");
    let p = props(table);

    let key_schema = seq(at(p, "KeySchema"));
    assert_eq!(key_schema.len(), 2);
    assert_eq!(text(at(p, "BillingMode")), "PAY_PER_REQUEST");

    let gsis = seq(at(p, "GlobalSecondaryIndexes"));
    assert_eq!(gsis.len(), 2, "by-domain and by-binding-id GSIs");
    for name in ["by-domain", "by-binding-id"] {
        let gsi = gsis
            .iter()
            .find(|g| text(at(g, "IndexName")) == name)
            .unwrap_or_else(|| panic!("missing GSI {name}"));
        assert_eq!(
            text(at(at(gsi, "Projection"), "ProjectionType")),
            "ALL",
            "GSI {name} must project all attributes"
        );
    }

    let sse = at(p, "SSESpecification");
    assert_eq!(text(at(sse, "SSEType")), "KMS");
    assert!(boolean(at(
        at(p, "PointInTimeRecoverySpecification"),
        "PointInTimeRecoveryEnabled"
    )));
}

#[test]
fn binding_admin_function_has_least_privilege_table_access() {
    let doc = core_template();
    let func = resource(&doc, "BindingAdminFunction");
    let policies = seq(at(props(func), "Policies"));
    assert_eq!(policies.len(), 1, "exactly one scoped policy statement set");
    let statements = seq(at(&policies[0], "Statement"));
    let actions: Vec<&str> = seq(at(&statements[0], "Action")).iter().map(text).collect();
    for required in [
        "dynamodb:GetItem",
        "dynamodb:Query",
        "dynamodb:TransactWriteItems",
    ] {
        assert!(actions.contains(&required), "missing {required}");
    }
    for forbidden in [
        "dynamodb:PutItem",
        "dynamodb:DeleteItem",
        "dynamodb:UpdateItem",
        "dynamodb:Scan",
    ] {
        assert!(
            !actions.contains(&forbidden),
            "mutations must go through TransactWriteItems, not {forbidden}"
        );
    }
}

#[test]
fn bindings_table_read_policy_is_reserved_and_unattached() {
    let doc = core_template();
    let policy = resource(&doc, "BindingsTableReadPolicy");
    let p = props(policy);
    let statements = seq(at(at(p, "PolicyDocument"), "Statement"));
    assert_eq!(statements.len(), 1);
    let actions: Vec<&str> = seq(at(&statements[0], "Action")).iter().map(text).collect();
    assert_eq!(
        actions,
        ["dynamodb:GetItem"],
        "decision reads are point gets only"
    );
    for attachment in ["Roles", "Users", "Groups"] {
        assert!(
            mapping(p).get(attachment).is_none(),
            "reserved policy must not attach to any {attachment} in this change"
        );
    }
}

/// One SQS write template extracted from an OpenAPI document: (path, method, template).
fn write_templates(doc: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (path, item) in mapping(at(doc, "paths")) {
        let path = path.as_str().expect("path key").to_string();
        for (method, op) in mapping(item) {
            let method = method.as_str().expect("method key").to_string();
            if !HTTP_METHODS.contains(&method.as_str()) {
                continue;
            }
            let integration = op
                .get("x-amazon-apigateway-integration")
                .expect("integration");
            if let Some(rt) = integration.get("requestTemplates") {
                out.push((
                    path.clone(),
                    method,
                    text(at(rt, "application/json")).to_string(),
                ));
            }
        }
    }
    out
}

#[test]
fn write_templates_inject_trusted_principal_envelope_from_context() {
    let oauth = write_templates(&oauth_contract());
    let iam = write_templates(&iam_contract());
    assert_eq!(oauth.len(), 6, "OAuth must expose 6 SQS write templates");
    assert_eq!(iam.len(), 6, "IAM must expose 6 SQS write templates");

    for (path, method, t) in &oauth {
        assert!(
            t.contains("Name=PrincipalVersion&MessageAttribute.2.Value.StringValue=1"),
            "OAuth {method} {path} missing envelope version"
        );
        assert!(
            t.contains("Name=AuthMechanism&MessageAttribute.3.Value.StringValue=oauth"),
            "OAuth {method} {path} missing mechanism"
        );
        for ctx in [
            "$context.authorizer.principalId",
            "$context.authorizer.issuer",
            "$context.authorizer.clientId",
            "$context.authorizer.globalRoles",
            "$context.authorizer.teamGroups",
        ] {
            assert!(t.contains(ctx), "OAuth {method} {path} missing {ctx}");
        }
    }

    for (path, method, t) in &iam {
        assert!(
            t.contains("Name=AuthMechanism&MessageAttribute.3.Value.StringValue=iam"),
            "IAM {method} {path} missing mechanism"
        );
        for ctx in ["$context.identity.accountId", "$context.identity.userArn"] {
            assert!(t.contains(ctx), "IAM {method} {path} missing {ctx}");
        }
    }
}

#[test]
fn write_templates_never_source_identity_attributes_from_caller_body() {
    for doc in [oauth_contract(), iam_contract()] {
        for (path, method, t) in write_templates(&doc) {
            assert!(
                !t.contains("Value.StringValue=$input")
                    && !t.contains("Value.StringValue=$util.urlEncode($input"),
                "{method} {path} sources a trusted message attribute from the caller body"
            );
        }
    }
}

fn publish_vtl(doc: &Value) -> String {
    let op = at(at(at(doc, "paths"), "/publish"), "post");
    let integration = at(op, "x-amazon-apigateway-integration");
    text(at(
        at(integration, "requestTemplates"),
        "application/json",
    ))
    .to_string()
}

#[test]
fn publish_routes_use_sqs_store_and_forward_with_stage_environment() {
    for doc in [oauth_contract(), iam_contract()] {
        let op = at(at(at(&doc, "paths"), "/publish"), "post");
        assert_eq!(
            text(at(op, "x-amazon-apigateway-request-validator")),
            "validate-body"
        );
        let integration = at(op, "x-amazon-apigateway-integration");
        assert_eq!(text(at(integration, "type")), "aws");
        assert!(
            text(at(at(integration, "uri"), "Fn::Sub")).contains("${PublishQueue.QueueName}"),
            "publish must integrate with the publish queue"
        );
        assert_eq!(
            text(at(at(at(integration, "responses"), "default"), "statusCode")),
            "202"
        );

        let vtl = publish_vtl(&doc);
        assert!(
            vtl.contains("Name=Environment&") && vtl.contains("Value.StringValue=$context.stage"),
            "publish must inject the stage as the Environment attribute"
        );
        assert!(
            vtl.contains("Name=IdempotencyKey&")
                && vtl.contains("$.idempotencyKey")
                && vtl.contains("$context.requestId"),
            "publish must inject the caller key or fall back to the request ID"
        );
    }
}

#[test]
fn publish_credentials_are_not_interchangeable() {
    let iam_doc = iam_contract();
    let iam_op = at(at(at(&iam_doc, "paths"), "/publish"), "post");
    assert_eq!(
        text(at(at(iam_op, "x-amazon-apigateway-auth"), "type")),
        "AWS_IAM"
    );
    let iam_vtl = publish_vtl(&iam_doc);
    assert!(iam_vtl.contains("Value.StringValue=iam&"));
    assert!(iam_vtl.contains("$context.identity.accountId"));
    assert!(!iam_vtl.contains("$context.authorizer."));

    let oauth_doc = oauth_contract();
    let oauth_op = at(at(at(&oauth_doc, "paths"), "/publish"), "post");
    assert!(
        oauth_op.get("x-amazon-apigateway-auth").is_none(),
        "OAuth publish relies on the default REQUEST authorizer, not IAM"
    );
    let oauth_vtl = publish_vtl(&oauth_doc);
    assert!(oauth_vtl.contains("Value.StringValue=oauth&"));
    assert!(oauth_vtl.contains("$context.authorizer.principalId"));
    assert!(!oauth_vtl.contains("$context.identity.userArn"));
}

#[test]
fn publish_request_schema_rejects_platform_owned_fields() {
    for doc in [oauth_contract(), iam_contract()] {
        let schema = at(at(at(&doc, "components"), "schemas"), "PublishRequest");
        assert!(
            !boolean(at(schema, "additionalProperties")),
            "PublishRequest must reject unknown (platform-owned) fields"
        );
        let mut required: Vec<&str> = seq(at(schema, "required"))
            .iter()
            .map(|v| text(v))
            .collect();
        required.sort_unstable();
        assert_eq!(required, ["data", "domain", "eventName"]);
    }
}
