//! Contract and template tests for the separated OAuth and AWS_IAM API
//! surfaces (Section 4). These reject unintended unauthenticated methods,
//! mixed-auth methods, and IAM-surface approval exposure.

mod support;

use serde_yaml::Value;
use support::{at, core_template, int, mapping, props, repo_yaml, resource, seq, text};

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
fn two_apis_exist_at_the_static_v1_stage() {
    let doc = core_template();
    for (logical_id, location) in [
        ("NexusOAuthApi", "./api/openapi-oauth.yaml"),
        ("NexusIamApi", "./api/openapi-iam.yaml"),
    ] {
        let api = resource(&doc, logical_id);
        assert_eq!(text(at(api, "Type")), "AWS::Serverless::Api");
        assert_eq!(text(at(props(api), "StageName")), "v1");
        let include = at(at(props(api), "DefinitionBody"), "Fn::Transform");
        assert_eq!(text(at(at(include, "Parameters"), "Location")), location);
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
    assert_eq!(text(at(identity, "Header")), "Authorization");
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
    assert_eq!(oauth.len(), 5, "OAuth must expose 5 SQS write templates");
    assert_eq!(iam.len(), 5, "IAM must expose 5 SQS write templates");

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
