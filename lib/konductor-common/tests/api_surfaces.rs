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
        ("KonductorOAuthApi", "./api/openapi-oauth.yaml"),
        ("KonductorIamApi", "./api/openapi-iam.yaml"),
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
fn oauth_api_attaches_request_authorizer_with_route_scoped_caching() {
    let doc = core_template();
    let api = resource(&doc, "KonductorOAuthApi");
    let auth = at(props(api), "Auth");
    assert_eq!(
        text(at(auth, "DefaultAuthorizer")),
        "OAuthRequestAuthorizer"
    );
    let authorizer = at(at(auth, "Authorizers"), "OAuthRequestAuthorizer");
    assert_eq!(text(at(authorizer, "FunctionPayloadType")), "REQUEST");
    assert_eq!(
        int(at(authorizer, "AuthorizerResultTtlInSeconds")),
        300,
        "authorizer caching uses a 300s TTL (the SAM transform drops a declared 0); the route-scoped key below is what keeps route-scoped policies correct"
    );
    let identity = at(authorizer, "Identity");
    let headers: Vec<&str> = seq(at(identity, "Headers")).iter().map(text).collect();
    assert_eq!(
        headers,
        ["Authorization"],
        "REQUEST authorizer identity must use the Headers list (the singular Header key is rejected by the SAM transform)"
    );
    let context: Vec<&str> = seq(at(identity, "Context")).iter().map(text).collect();
    assert_eq!(
        context,
        ["httpMethod", "resourcePath"],
        "cache key must include the route: the authorizer policy is scoped to the invoked route, so a token-only key would poison a caller's other routes"
    );
    assert_eq!(
        text(at(authorizer, "FunctionArn")),
        "OAuthAuthorizerFunction.Arn"
    );
}

#[test]
fn iam_api_uses_aws_iam_authorization() {
    let doc = core_template();
    let api = resource(&doc, "KonductorIamApi");
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
    assert_eq!(text(at(event, "RestApiId")), "KonductorOAuthApi");
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
            "KonductorOAuthApi",
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
    // DynamoDB transactions require both TransactWriteItems and the
    // underlying item actions (PutItem for create+audit, DeleteItem for
    // revoke) — the service evaluates each action in the transaction.
    for required in [
        "dynamodb:GetItem",
        "dynamodb:Query",
        "dynamodb:TransactWriteItems",
        "dynamodb:PutItem",
        "dynamodb:DeleteItem",
    ] {
        assert!(actions.contains(&required), "missing {required}");
    }
    for forbidden in ["dynamodb:UpdateItem", "dynamodb:Scan"] {
        assert!(
            !actions.contains(&forbidden),
            "bindings are created and deleted, never updated or scanned ({forbidden})"
        );
    }
}

/// (inline actions, managed policy refs) for a SAM function's Policies list.
fn function_policy_shape(doc: &Value, logical_id: &str) -> (Vec<String>, Vec<String>) {
    let func = resource(doc, logical_id);
    let mut actions = Vec::new();
    let mut managed = Vec::new();
    for policy in seq(at(props(func), "Policies")) {
        match policy.get("Statement") {
            Some(statements) => {
                for statement in seq(statements) {
                    for action in seq(at(statement, "Action")) {
                        actions.push(text(action).to_string());
                    }
                }
            }
            None => managed.push(text(policy).to_string()),
        }
    }
    (actions, managed)
}

#[test]
fn publish_consumer_has_least_privilege_permissions() {
    let doc = core_template();
    let (mut actions, managed) = function_policy_shape(&doc, "PublishEventsFunction");
    assert_eq!(
        managed,
        ["BindingsTableReadPolicy"],
        "authorization decisions read bindings only through the reserved policy"
    );
    actions.sort();
    assert_eq!(
        actions,
        [
            "dynamodb:GetItem",
            "events:PutEvents",
            "schemas:DescribeSchema",
            "sqs:DeleteMessage",
            "sqs:GetQueueAttributes",
            "sqs:ReceiveMessage",
        ],
        "publish consumer: domain-state read, schema read, org-scoped PutEvents, queue access"
    );
}

#[test]
fn schema_admin_has_least_privilege_permissions() {
    let doc = core_template();
    let (mut actions, managed) = function_policy_shape(&doc, "SchemaAdminFunction");
    assert_eq!(
        managed,
        ["BindingsTableReadPolicy"],
        "authorization decisions read bindings only through the reserved policy"
    );
    actions.sort();
    assert_eq!(
        actions,
        [
            "dynamodb:GetItem",
            "schemas:CreateSchema",
            "schemas:UpdateSchema"
        ],
        "schema handlers: domain-state read and append-only schema writes"
    );
}

#[test]
fn publisher_functions_have_no_administration_permissions() {
    let doc = core_template();
    let forbidden = [
        "schemas:CreateRegistry",
        "schemas:DeleteRegistry",
        "schemas:UpdateRegistry",
        "schemas:PutResourcePolicy",
        "schemas:DeleteSchema",
        "schemas:DeleteSchemaVersion",
        "events:PutRule",
        "events:DeleteRule",
        "events:PutPermission",
        "events:RemovePermission",
        "dynamodb:PutItem",
        "dynamodb:DeleteItem",
        "dynamodb:UpdateItem",
    ];
    for logical_id in ["PublishEventsFunction", "SchemaAdminFunction"] {
        let (actions, _) = function_policy_shape(&doc, logical_id);
        for action in &forbidden {
            assert!(
                !actions.iter().any(|a| a == action),
                "{logical_id} must not hold {action}"
            );
        }
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
    text(at(at(integration, "requestTemplates"), "application/json")).to_string()
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
            text(at(
                at(at(integration, "responses"), "default"),
                "statusCode"
            )),
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

#[test]
fn schema_write_routes_are_proxy_integrations_on_both_surfaces() {
    for doc in [oauth_contract(), iam_contract()] {
        for (path, operation) in [
            ("/schemas", "CreateSchema"),
            ("/schemas/versions", "AppendSchemaVersion"),
        ] {
            let op = at(at(at(&doc, "paths"), path), "post");
            assert_eq!(text(at(op, "operationId")), operation);
            assert_eq!(
                text(at(op, "x-amazon-apigateway-request-validator")),
                "validate-body"
            );
            let integration = at(op, "x-amazon-apigateway-integration");
            assert_eq!(text(at(integration, "type")), "aws_proxy");
            assert!(
                text(at(at(integration, "uri"), "Fn::Sub")).contains("${SchemaAdminFunction.Arn}"),
                "{operation} must integrate with the schema admin function"
            );
        }
    }
}

#[test]
fn schema_write_credentials_are_not_interchangeable() {
    let iam_doc = iam_contract();
    for path in ["/schemas", "/schemas/versions"] {
        let op = at(at(at(&iam_doc, "paths"), path), "post");
        assert_eq!(
            text(at(at(op, "x-amazon-apigateway-auth"), "type")),
            "AWS_IAM"
        );
    }
    let oauth_doc = oauth_contract();
    for path in ["/schemas", "/schemas/versions"] {
        let op = at(at(at(&oauth_doc, "paths"), path), "post");
        assert!(
            op.get("x-amazon-apigateway-auth").is_none(),
            "OAuth schema writes rely on the default REQUEST authorizer"
        );
    }
}

#[test]
fn schema_management_is_append_only() {
    for doc in [oauth_contract(), iam_contract()] {
        for (path, item) in mapping(at(&doc, "paths")) {
            let path = path.as_str().expect("path key");
            if !path.starts_with("/schemas") {
                continue;
            }
            for (method, _) in mapping(item) {
                let method = method.as_str().expect("method key");
                if !HTTP_METHODS.contains(&method) {
                    continue;
                }
                assert!(
                    ["get", "post"].contains(&method),
                    "{method} {path} violates append-only schema management"
                );
            }
        }
    }
}

#[test]
fn schema_request_schemas_reject_platform_owned_fields() {
    for doc in [oauth_contract(), iam_contract()] {
        let schemas = at(at(&doc, "components"), "schemas");
        let create = at(schemas, "CreateSchemaRequest");
        assert!(!boolean(at(create, "additionalProperties")));
        let mut create_required: Vec<&str> = seq(at(create, "required"))
            .iter()
            .map(|v| text(v))
            .collect();
        create_required.sort_unstable();
        assert_eq!(
            create_required,
            ["content", "domain", "eventName", "schemaType"]
        );

        let append = at(schemas, "AppendSchemaVersionRequest");
        assert!(!boolean(at(append, "additionalProperties")));
        let mut append_required: Vec<&str> = seq(at(append, "required"))
            .iter()
            .map(|v| text(v))
            .collect();
        append_required.sort_unstable();
        assert_eq!(append_required, ["content", "domain", "eventName"]);
    }
}

#[test]
fn entitlement_administration_routes_are_oauth_only() {
    let oauth = operations(&oauth_contract());
    let iam = operations(&iam_contract());

    let mut oauth_ops: Vec<(&str, &str)> = oauth
        .iter()
        .filter(|op| op.path.starts_with("/entitlements"))
        .map(|op| (op.method.as_str(), op.path.as_str()))
        .collect();
    oauth_ops.sort();
    assert_eq!(
        oauth_ops,
        [
            ("delete", "/entitlements/{entitlementId}"),
            ("get", "/entitlements"),
            ("post", "/entitlements"),
        ],
        "OAuth surface must expose exactly the three entitlement administration operations"
    );

    assert!(
        iam.iter().all(|op| !op.path.starts_with("/entitlements")),
        "IAM surface must not expose entitlement administration"
    );
}

#[test]
fn entitlement_admin_function_is_wired_to_the_oauth_surface_only() {
    let doc = core_template();
    let func = resource(&doc, "EntitlementAdminFunction");
    let events = mapping(at(props(func), "Events"));
    assert_eq!(
        events.len(),
        3,
        "entitlement admin must have exactly the three route events"
    );
    for (name, event) in events {
        let event_props = at(event, "Properties");
        assert_eq!(
            text(at(event_props, "RestApiId")),
            "KonductorOAuthApi",
            "event {:?} must target the OAuth API",
            name
        );
    }
}

#[test]
fn entitlements_table_matches_documented_schema() {
    let doc = core_template();
    let table = resource(&doc, "AuthorizationEntitlementsTable");
    let p = props(table);

    let key_schema = seq(at(p, "KeySchema"));
    assert_eq!(key_schema.len(), 2);
    assert_eq!(text(at(p, "BillingMode")), "PAY_PER_REQUEST");

    let gsis = seq(at(p, "GlobalSecondaryIndexes"));
    assert_eq!(gsis.len(), 2, "by-producer and by-entitlement-id GSIs");
    for name in ["by-producer", "by-entitlement-id"] {
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
    assert_eq!(
        text(at(at(p, "StreamSpecification"), "StreamViewType")),
        "OLD_IMAGE",
        "the revocation worker consumes each removed entitlement's prior image"
    );
}

#[test]
fn entitlements_table_read_policy_is_query_only() {
    let doc = core_template();
    let policy = resource(&doc, "EntitlementsTableReadPolicy");
    let p = props(policy);
    let statements = seq(at(at(p, "PolicyDocument"), "Statement"));
    assert_eq!(statements.len(), 1);
    let actions: Vec<&str> = seq(at(&statements[0], "Action")).iter().map(text).collect();
    assert_eq!(
        actions,
        ["dynamodb:Query"],
        "coverage decision reads are consumer-partition queries only"
    );
    for attachment in ["Roles", "Users", "Groups"] {
        assert!(
            mapping(p).get(attachment).is_none(),
            "policy attaches through consumers (function Policies, role ManagedPolicyArns), never on the policy resource"
        );
    }
}

#[test]
fn subscription_commands_use_flat_verbs_on_both_surfaces() {
    for doc in [oauth_contract(), iam_contract()] {
        let ops = operations(&doc);
        let has =
            |method: &str, path: &str| ops.iter().any(|op| op.method == method && op.path == path);
        assert!(has("post", "/subscriptions"), "subscribe is a flat verb");
        assert!(
            has("post", "/subscriptions/state"),
            "state change is a flat verb with the id in the body"
        );
        assert!(
            has("delete", "/subscriptions/{subscriptionId}"),
            "unsubscribe is a flat verb"
        );
        assert!(
            !has("patch", "/subscriptions/{subscriptionId}"),
            "PATCH /subscriptions/{{subscriptionId}} is replaced by POST /subscriptions/state"
        );
    }
}

#[test]
fn subscription_write_templates_carry_route_and_no_caller_account_id() {
    for doc in [oauth_contract(), iam_contract()] {
        let templates = write_templates(&doc);
        let mut subscription_templates: Vec<&(String, String, String)> = templates
            .iter()
            .filter(|(path, _, _)| path.starts_with("/subscriptions"))
            .collect();
        subscription_templates.sort();
        assert_eq!(
            subscription_templates.len(),
            3,
            "subscribe, update-state, unsubscribe"
        );
        for (path, method, t) in subscription_templates {
            assert!(
                t.contains("Name=Resource&")
                    && t.contains("Value.StringValue=$context.resourcePath"),
                "{method} {path} must inject the route as the Resource message attribute"
            );
            assert!(
                !t.contains("callerAccountId"),
                "{method} {path} must not carry callerAccountId — authorization is engine-based, not account-ownership"
            );
        }
    }
}

#[test]
fn update_subscription_state_request_schema_is_closed() {
    for doc in [oauth_contract(), iam_contract()] {
        let schemas = at(at(&doc, "components"), "schemas");
        let request = at(schemas, "UpdateSubscriptionStateRequest");
        assert!(!boolean(at(request, "additionalProperties")));
        let mut required: Vec<&str> = seq(at(request, "required")).iter().map(text).collect();
        required.sort_unstable();
        assert_eq!(
            required,
            ["consumerDomain", "environments", "subscriptionId"]
        );
        assert!(
            mapping(at(&doc, "components")).contains_key("schemas"),
            "schemas map must exist"
        );
        assert!(
            mapping(schemas).get("UpdateSubscriptionRequest").is_none(),
            "the old path-parameter PATCH body schema is gone"
        );
    }
}

#[test]
fn subscriptions_function_reads_bindings_and_entitlements() {
    let doc = core_template();
    let func = resource(&doc, "SubscriptionsFunction");
    let env = mapping(at(at(props(func), "Environment"), "Variables"));
    for var in ["BINDINGS_TABLE", "ENTITLEMENTS_TABLE"] {
        assert!(
            env.contains_key(var),
            "subscriptions consumer needs {var} for the grant engine"
        );
    }
    let (_, managed) = function_policy_shape(&doc, "SubscriptionsFunction");
    for policy in ["BindingsTableReadPolicy", "EntitlementsTableReadPolicy"] {
        assert!(
            managed.contains(&policy.to_string()),
            "subscriptions consumer must attach {policy}"
        );
    }
}

#[test]
fn subscription_list_reads_are_flat_and_direction_scoped() {
    for doc in [oauth_contract(), iam_contract()] {
        let op = at(at(at(&doc, "paths"), "/subscriptions"), "get");
        let params = seq(at(op, "parameters"));
        let mut names: Vec<&str> = params.iter().map(|p| text(at(p, "name"))).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            ["direction", "domain", "schema", "status", "version"],
            "list is scoped by domain+direction; consumer/producer/nextToken/maxResults are gone"
        );
        for required in ["domain", "direction"] {
            let param = params
                .iter()
                .find(|p| text(at(p, "name")) == required)
                .unwrap_or_else(|| panic!("missing {required} parameter"));
            assert!(
                boolean(at(param, "required")),
                "{required} must be a required query parameter"
            );
        }
        let responses = mapping(at(op, "responses"));
        assert!(
            responses.contains_key("403"),
            "list denies with 403 when the direction's binding is missing"
        );
    }
}

#[test]
fn describe_subscription_separates_forbidden_from_not_found() {
    for doc in [oauth_contract(), iam_contract()] {
        let op = at(
            at(at(&doc, "paths"), "/subscriptions/{subscriptionId}"),
            "get",
        );
        let responses = mapping(at(op, "responses"));
        for code in ["403", "404"] {
            assert!(
                responses.contains_key(code),
                "describe must distinguish {code} — deny-as-unknown is dropped"
            );
        }
        let not_found = text(at(responses.get("404").unwrap(), "description"));
        assert!(
            !not_found.contains("owns neither"),
            "404 description must not retain deny-as-unknown wording"
        );
    }
}

#[test]
fn list_response_has_no_pagination_token() {
    for doc in [oauth_contract(), iam_contract()] {
        let schema = at(
            at(at(&doc, "components"), "schemas"),
            "ListSubscriptionsResponse",
        );
        let properties = mapping(at(schema, "properties"));
        assert!(
            !properties.contains_key("nextToken"),
            "direction-scoped index queries return full result sets"
        );
    }
}

#[test]
fn subscription_read_functions_read_the_bindings_table() {
    let doc = core_template();
    for logical_id in ["ListSubscriptionsFunction", "DescribeSubscriptionFunction"] {
        let func = resource(&doc, logical_id);
        let env = mapping(at(at(props(func), "Environment"), "Variables"));
        assert!(
            env.contains_key("BINDINGS_TABLE"),
            "{logical_id} needs BINDINGS_TABLE for direction/side authorization"
        );
        let (_, managed) = function_policy_shape(&doc, logical_id);
        assert!(
            managed.contains(&"BindingsTableReadPolicy".to_string()),
            "{logical_id} must attach the bindings read policy"
        );
    }
}

#[test]
fn schema_catalog_is_entitlement_filtered_and_watermark_paginated() {
    for doc in [oauth_contract(), iam_contract()] {
        let op = at(at(at(&doc, "paths"), "/schemas"), "get");
        let params = seq(at(op, "parameters"));
        let mut names: Vec<&str> = params.iter().map(|p| text(at(p, "name"))).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            ["consumerDomain", "domain", "environment", "maxResults", "nextToken"],
            "the catalog is consumer-keyed with producer/environment filters and a watermark cursor"
        );
        for name in &names {
            let param = params
                .iter()
                .find(|p| text(at(p, "name")) == *name)
                .unwrap();
            assert!(
                !boolean(at(param, "required")),
                "{name} must be optional — consumerDomain is omitted only by platform roles"
            );
        }
        let max = at(
            at(
                params
                    .iter()
                    .find(|p| text(at(p, "name")) == "maxResults")
                    .unwrap(),
                "schema",
            ),
            "maximum",
        );
        assert_eq!(int(max), 50, "page size is capped at 50");
        let responses = mapping(at(op, "responses"));
        assert!(
            responses.contains_key("400"),
            "cursor/filter mismatch is a 400"
        );
        assert!(
            responses.contains_key("403"),
            "missing binding or platform role is a 403"
        );
        assert!(
            !responses.contains_key("404"),
            "a producer filter that matches nothing yields an empty list, not a 404"
        );
    }
}

#[test]
fn schema_detail_requires_coverage_or_a_platform_role() {
    for doc in [oauth_contract(), iam_contract()] {
        let op = at(at(at(&doc, "paths"), "/schemas/{schemaName}"), "get");
        let params = seq(at(op, "parameters"));
        let names: Vec<&str> = params.iter().map(|p| text(at(p, "name"))).collect();
        assert!(
            names.contains(&"consumerDomain"),
            "detail authorizes coverage against the consumer domain"
        );
        let responses = mapping(at(op, "responses"));
        for code in ["403", "404"] {
            assert!(
                responses.contains_key(code),
                "detail must distinguish uncovered ({code}) from absent"
            );
        }
    }
}

#[test]
fn catalog_functions_read_bindings_and_entitlements() {
    let doc = core_template();
    for logical_id in ["ListSchemasFunction", "DescribeSchemaFunction"] {
        let func = resource(&doc, logical_id);
        let env = mapping(at(at(props(func), "Environment"), "Variables"));
        for var in ["BINDINGS_TABLE", "ENTITLEMENTS_TABLE"] {
            assert!(
                env.contains_key(var),
                "{logical_id} needs {var} for coverage authorization"
            );
        }
        let (_, managed) = function_policy_shape(&doc, logical_id);
        for policy in ["BindingsTableReadPolicy", "EntitlementsTableReadPolicy"] {
            assert!(
                managed.contains(&policy.to_string()),
                "{logical_id} must attach {policy}"
            );
        }
    }
}

#[test]
fn entitlement_observability_covers_the_revocation_path_and_drift() {
    let doc = core_template();
    let resources = mapping(at(&doc, "Resources"));

    for alarm in [
        "EntitlementRevocationDlqDepthAlarm",
        "EntitlementStreamConsumerIteratorAgeAlarm",
        "EntitlementRevocationWorkerErrorsAlarm",
        "EntitlementCoverageSweepDriftAlarm",
        "SubscriptionAuthorizationDenialsAlarm",
        "CatalogLatencyAlarm",
    ] {
        assert!(
            resources.contains_key(alarm),
            "missing alarm {alarm} — the revocation path and its backstop must be observable"
        );
    }

    for filter in [
        "EntitlementCoverageSweepDriftFilter",
        "SubscriptionAuthorizationDenialsFilter",
    ] {
        let resource = resource(&doc, filter);
        assert_eq!(text(at(resource, "Type")), "AWS::Logs::MetricFilter");
    }

    let drift = resource(&doc, "EntitlementStreamConsumerIteratorAgeAlarm");
    assert_eq!(
        text(at(props(drift), "MetricName")),
        "IteratorAge",
        "a stalled entitlement stream delays revocation notification"
    );
}

#[test]
fn activation_workflow_queries_entitlements_before_enabling() {
    let doc = core_template();
    let workflow = resource(&doc, "ActivateSubscriptionWorkflow");
    let substitutions = mapping(at(props(workflow), "DefinitionSubstitutions"));
    assert!(
        substitutions.contains_key("EntitlementsTable"),
        "the activation workflow needs the entitlements table for its final coverage check"
    );

    let role = resource(&doc, "SubscriptionWorkflowRole");
    let managed: Vec<&str> = seq(at(props(role), "ManagedPolicyArns"))
        .iter()
        .map(text)
        .collect();
    assert!(
        managed.contains(&"EntitlementsTableReadPolicy"),
        "the workflow role must be able to query the entitlements table"
    );

    // DynamoDB Query has no optimized integration (get/put/update/delete
    // only); pinning the SDK ARN rejects the invalid bare form at test time
    // instead of at deployment.
    let asl = repo_yaml("event-streams/state-machines/activate-subscription.asl.yaml");
    let item_states = mapping(at(
        at(
            at(at(&asl, "States"), "ActivateEnvironments"),
            "ItemProcessor",
        ),
        "States",
    ));
    let query_state = item_states
        .iter()
        .find(|(name, _)| name.as_str() == Some("QueryCoveringEntitlements"))
        .map(|(_, state)| state)
        .expect("QueryCoveringEntitlements state");
    assert_eq!(
        text(at(query_state, "Resource")),
        "arn:aws:states:::aws-sdk:dynamodb:query"
    );
}

#[test]
fn approval_workflow_finalizes_through_lambda_transaction() {
    let doc = core_template();
    let workflow = resource(&doc, "ApprovalWorkflow");
    let substitutions = mapping(at(props(workflow), "DefinitionSubstitutions"));
    assert!(
        substitutions.contains_key("FinalizeApprovalFunction"),
        "the approval workflow needs the finalize function for its transactional commit"
    );

    // The approved path must commit domain status + owner binding + audit in
    // one Lambda-owned transaction; a direct DynamoDB updateItem would leave
    // ownership behind. Pin the state shape so the split cannot regress.
    let asl = repo_yaml("domain-management/state-machines/approval.asl.yaml");
    let states = mapping(at(&asl, "States"));
    let finalize = states
        .iter()
        .find(|(name, _)| name.as_str() == Some("FinalizeApproval"))
        .map(|(_, state)| state)
        .expect("FinalizeApproval state");
    assert_eq!(
        text(at(finalize, "Resource")),
        "arn:aws:states:::lambda:invoke"
    );
    assert_eq!(text(at(finalize, "Next")), "EmitApprovedEvent");
    assert!(
        !states
            .iter()
            .any(|(name, _)| name.as_str() == Some("UpdateStatusApproved")),
        "the direct updateItem approved path is retired"
    );

    let check = states
        .iter()
        .find(|(name, _)| name.as_str() == Some("CheckDecision"))
        .map(|(_, state)| state)
        .expect("CheckDecision state");
    let choices = seq(at(check, "Choices"));
    assert!(
        choices
            .iter()
            .any(|c| text(at(c, "Next")) == "FinalizeApproval"),
        "the approved choice must route to FinalizeApproval"
    );
}

#[test]
fn user_pool_client_supports_srp_login() {
    let doc = core_template();
    let client = resource(&doc, "KonductorUserPoolClient");
    let flows: Vec<&str> = seq(at(props(client), "ExplicitAuthFlows"))
        .iter()
        .map(text)
        .collect();
    assert!(
        flows.contains(&"ALLOW_USER_SRP_AUTH"),
        "custom login clients authenticate via SRP: {flows:?}"
    );
    assert!(
        flows.contains(&"ALLOW_REFRESH_TOKEN_AUTH"),
        "sessions refresh without re-login: {flows:?}"
    );
    assert!(
        !flows.contains(&"ALLOW_USER_PASSWORD_AUTH"),
        "raw passwords must not traverse clients: {flows:?}"
    );
}

#[test]
fn domain_state_route_is_oauth_only_and_wired() {
    let oauth = oauth_contract();
    let state = at(at(&oauth, "paths"), "/domains/{domainName}/state");
    let integration = at(at(state, "post"), "x-amazon-apigateway-integration");
    assert_eq!(text(at(integration, "type")), "aws_proxy");

    // State flips are platform-admin territory; IAM automation has no route.
    let iam = iam_contract();
    assert!(
        mapping(at(&iam, "paths"))
            .keys()
            .all(|p| p.as_str() != Some("/domains/{domainName}/state")),
        "domain state must not be exposed on the IAM API"
    );

    let doc = core_template();
    let function = resource(&doc, "DomainStateFunction");
    let events = mapping(at(props(function), "Events"));
    assert_eq!(events.len(), 1, "exactly one API event: OAuth only");
    let event = events.values().next().expect("one event");
    assert_eq!(
        text(at(props(event), "Path")),
        "/domains/{domainName}/state"
    );
}

#[test]
fn pending_registrations_route_is_oauth_only_and_wired() {
    let oauth = oauth_contract();
    let pending = at(at(&oauth, "paths"), "/domains/registrations/pending");
    let integration = at(at(pending, "get"), "x-amazon-apigateway-integration");
    assert_eq!(text(at(integration, "type")), "aws_proxy");

    // The inbox carries workflow task tokens; it must never appear on the
    // IAM surface.
    let iam = iam_contract();
    assert!(
        mapping(at(&iam, "paths"))
            .keys()
            .all(|p| p.as_str() != Some("/domains/registrations/pending")),
        "pending registrations must not be exposed on the IAM API"
    );

    let doc = core_template();
    let function = resource(&doc, "PendingRegistrationsFunction");
    let events = mapping(at(props(function), "Events"));
    assert_eq!(events.len(), 1, "exactly one API event: OAuth only");
    let event = events.values().next().expect("one event");
    assert_eq!(
        text(at(props(event), "Path")),
        "/domains/registrations/pending"
    );
}
