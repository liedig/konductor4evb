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
            let name = format!("{logical_id}{suffix}Stage");
            let stage = at(at(&doc, "Resources"), &name);
            assert_eq!(text(at(stage, "Type")), "AWS::ApiGateway::Stage");
            assert_eq!(text(at(props(stage), "StageName")), env);
            assert_eq!(text(at(at(props(stage), "RestApiId"), "Ref")), logical_id);
            assert_eq!(
                text(at(at(props(stage), "DeploymentId"), "Ref")),
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
        [] as [&str; 0],
        "the delivery-only consumer needs no grant-engine policies"
    );
    actions.sort();
    assert_eq!(
        actions,
        [
            "dynamodb:GetItem",
            "events:PutEvents",
            "sqs:DeleteMessage",
            "sqs:GetQueueAttributes",
            "sqs:ReceiveMessage",
        ],
        "publish consumer: domain-state read, org-scoped PutEvents, queue access"
    );
}

#[test]
fn publish_admission_has_least_privilege_permissions() {
    let doc = core_template();
    let (mut actions, managed) = function_policy_shape(&doc, "PublishEventsAdmissionFunction");
    assert_eq!(
        managed,
        [
            "BindingsTableReadPolicy",
            "EntitlementsTableReadPolicy",
            "MetadataPointerReadPolicy",
        ],
        "authorization decisions read bindings, entitlements, and the metadata pointer through the reserved policies"
    );
    actions.sort();
    assert_eq!(
        actions,
        [
            "dynamodb:GetItem",
            "schemas:DescribeSchema",
            "sqs:SendMessage",
        ],
        "admission: domain-state read, schema read, publish-queue write"
    );
}

#[test]
fn publish_queue_writes_are_restricted_to_admission() {
    let doc = core_template();
    let policy = resource(&doc, "KonductorApiSqsPolicy");
    let statements = seq(at(at(props(policy), "PolicyDocument"), "Statement"));
    let resources: Vec<String> = statements
        .iter()
        .flat_map(|s| seq(at(s, "Resource")).iter().map(|r| format!("{r:?}")))
        .collect();
    assert!(
        !resources.iter().any(|r| r.contains("PublishQueue")),
        "API Gateway must no longer write the publish queue directly"
    );

    // IAM grants are necessary but not sufficient: a same-account principal
    // with broad sqs:SendMessage could inject an unvalidated command, so the
    // queue itself denies every writer except the admission role.
    let queue_policy = resource(&doc, "PublishQueueAdmissionOnlyPolicy");
    let qp_statements = seq(at(at(props(queue_policy), "PolicyDocument"), "Statement"));
    let deny = qp_statements
        .iter()
        .find(|s| text(at(s, "Sid")) == "DenyNonAdmissionSenders")
        .expect("queue policy must deny non-admission senders");
    assert_eq!(text(at(deny, "Effect")), "Deny");
    let actions: Vec<&str> = seq(at(deny, "Action")).iter().map(|a| text(a)).collect();
    assert!(
        actions.contains(&"sqs:SendMessage"),
        "sqs:SendMessage authorizes both send APIs (batch has no distinct IAM action), got {actions:?}"
    );
    assert!(
        !actions.contains(&"sqs:SendMessageBatch"),
        "sqs:SendMessageBatch is not a valid IAM action and fails deployment"
    );
    let exempt = format!(
        "{:?}",
        at(at(at(deny, "Condition"), "ArnNotLike"), "aws:PrincipalArn")
    );
    assert!(
        exempt.contains("PublishEventsAdmissionFunctionRole"),
        "the deny must exempt only the admission role, got {exempt}"
    );
}

#[test]
fn schema_admin_has_least_privilege_permissions() {
    let doc = core_template();
    let (mut actions, managed) = function_policy_shape(&doc, "SchemaAdminFunction");
    assert_eq!(
        managed,
        ["BindingsTableReadPolicy", "EntitlementsTableReadPolicy"],
        "grant-engine decisions read bindings and entitlements through the reserved policies"
    );
    actions.sort();
    assert_eq!(
        actions,
        [
            "dynamodb:GetItem",
            "dynamodb:PutItem",
            "events:PutEvents",
            "schemas:CreateSchema",
            "schemas:DescribeSchema",
            "schemas:UpdateSchema"
        ],
        "schema handlers: domain-state read, append-only schema writes, the compatibility gate's latest-version read (gate-schema-promotion), the SchemaVersionPublished promotion signal (follow-schema-promotion), and version-content recording"
    );
    let function = resource(&doc, "SchemaAdminFunction");
    let policies = seq(at(props(function), "Policies"));
    let scoped = policies
        .iter()
        .filter_map(|policy| policy.get("Statement"))
        .flat_map(seq)
        .any(|s| {
            seq(at(s, "Action"))
                .iter()
                .any(|a| text(a) == "dynamodb:PutItem")
                && format!("{:?}", at(s, "Resource")).contains("SchemaVersionsTable")
        });
    assert!(
        scoped,
        "the PutItem grant must be scoped to the versions table only"
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
    for logical_id in [
        "PublishEventsFunction",
        "PublishEventsAdmissionFunction",
        "SchemaAdminFunction",
    ] {
        let (actions, _) = function_policy_shape(&doc, logical_id);
        for action in &forbidden {
            // Schema-admin records version content into the versions table:
            // PutItem there is its single sanctioned write (scoped above).
            if logical_id == "SchemaAdminFunction" && *action == "dynamodb:PutItem" {
                continue;
            }
            assert!(
                !actions.iter().any(|a| a == action),
                "{logical_id} must not hold {action}"
            );
        }
    }
}

#[test]
fn schema_promotions_has_least_privilege_permissions() {
    let doc = core_template();
    let (mut actions, managed) = function_policy_shape(&doc, "SchemaPromotionsFunction");
    assert_eq!(
        managed,
        ["BindingsTableReadPolicy", "EntitlementsTableReadPolicy"],
        "grant-engine decisions read bindings and entitlements through the reserved policies"
    );
    actions.sort();
    assert_eq!(
        actions,
        [
            "dynamodb:GetItem",
            "dynamodb:GetItem",
            "dynamodb:PutItem",
            "schemas:DescribeSchema"
        ],
        "promotion requests: domain-state read, head-item get/put, and the source registry's latest-version read; workflow execution rides the stream"
    );
}

#[test]
fn schema_promotion_steps_has_least_privilege_permissions() {
    let doc = core_template();
    let (mut actions, managed) = function_policy_shape(&doc, "SchemaPromotionStepsFunction");
    assert_eq!(managed, Vec::<String>::new(), "no managed policies");
    actions.sort();
    assert_eq!(
        actions,
        [
            // promotions head + REPIN items: get, conditional put, update
            "dynamodb:GetItem",
            // versions content records
            "dynamodb:GetItem",
            // metadata pointer reads for upgrade snapshot selectors
            "dynamodb:GetItem",
            "dynamodb:PutItem",
            "dynamodb:PutItem",
            // event-streams entry enumeration
            "dynamodb:Query",
            "dynamodb:UpdateItem",
            "schemas:CreateSchema",
            "schemas:DescribeSchema",
            "schemas:ListSchemaVersions",
            "schemas:UpdateSchema",
        ],
        "steps: head/REPIN writes, version content read+write, entry query, registry reads and target writes (CreateSchema lands v1 on first arrival)"
    );
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

#[test]
fn publish_routes_admit_through_the_admission_lambda() {
    for doc in [oauth_contract(), iam_contract()] {
        let op = at(at(at(&doc, "paths"), "/publish"), "post");
        assert_eq!(
            text(at(op, "x-amazon-apigateway-request-validator")),
            "validate-body"
        );
        let integration = at(op, "x-amazon-apigateway-integration");
        assert_eq!(text(at(integration, "type")), "aws_proxy");
        assert!(
            text(at(at(integration, "uri"), "Fn::Sub"))
                .contains("${PublishEventsAdmissionFunction.Arn}"),
            "publish must integrate with the admission Lambda"
        );
        assert!(
            integration.get("requestTemplates").is_none(),
            "admission owns envelope construction; no SQS mapping template"
        );
        assert!(
            integration.get("credentials").is_none(),
            "no API Gateway SQS role on the publish route"
        );

        let responses = mapping(at(op, "responses"));
        for status in ["202", "400", "403", "404", "422", "503"] {
            assert!(
                responses.contains_key(status),
                "publish must document a {status} response"
            );
        }
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

    let oauth_doc = oauth_contract();
    let oauth_op = at(at(at(&oauth_doc, "paths"), "/publish"), "post");
    assert!(
        oauth_op.get("x-amazon-apigateway-auth").is_none(),
        "OAuth publish relies on the default REQUEST authorizer, not IAM"
    );
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
fn schema_write_routes_document_validation_failures() {
    for doc in [oauth_contract(), iam_contract()] {
        for path in ["/schemas", "/schemas/versions"] {
            let op = at(at(at(&doc, "paths"), path), "post");
            let responses = mapping(at(op, "responses"));
            assert!(
                responses.contains_key("400"),
                "POST {path} must document the 400 for unsupported schemaType and malformed draft-4 content"
            );
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
fn metadata_administration_routes_are_oauth_only() {
    let oauth = operations(&oauth_contract());
    let iam = operations(&iam_contract());

    let mut oauth_ops: Vec<(&str, &str)> = oauth
        .iter()
        .filter(|op| op.path.starts_with("/metadata"))
        .map(|op| (op.method.as_str(), op.path.as_str()))
        .collect();
    oauth_ops.sort();
    assert_eq!(
        oauth_ops,
        [
            ("get", "/metadata/activations"),
            ("get", "/metadata/environments"),
            ("get", "/metadata/revisions"),
            ("get", "/metadata/revisions/{revision}"),
            ("post", "/metadata/activations"),
            ("post", "/metadata/preview"),
            ("post", "/metadata/readiness"),
            ("post", "/metadata/revisions"),
        ],
        "OAuth surface must expose exactly the eight metadata administration operations"
    );

    assert!(
        iam.iter().all(|op| !op.path.starts_with("/metadata")),
        "IAM surface must not expose metadata administration"
    );
}

#[test]
fn metadata_admin_function_is_wired_to_the_oauth_surface_only() {
    let doc = core_template();
    let func = resource(&doc, "MetadataAdminFunction");
    let events = mapping(at(props(func), "Events"));
    assert_eq!(
        events.len(),
        8,
        "metadata admin must have exactly the eight route events"
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
fn composed_contract_routes_are_on_both_surfaces() {
    for doc in [oauth_contract(), iam_contract()] {
        let contract = at(
            at(at(&doc, "paths"), "/schemas/{schemaName}/contract"),
            "get",
        );
        assert_eq!(text(at(contract, "operationId")), "GetSchemaContract");
        let params = seq(at(contract, "parameters"));
        let mut names: Vec<&str> = params.iter().map(|p| text(at(p, "name"))).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "consumerDomain",
                "domain",
                "environment",
                "metadataRevision",
                "schemaName",
                "schemaVersion"
            ],
            "the contract read authorizes like detail and carries all-or-nothing historical selectors"
        );
        let mut required: Vec<&str> = params
            .iter()
            .filter(|p| boolean(at(p, "required")))
            .map(|p| text(at(p, "name")))
            .collect();
        required.sort_unstable();
        assert_eq!(
            required,
            ["domain", "environment", "schemaName"],
            "historical selectors are optional; their absence selects the current pair"
        );
        let responses = mapping(at(contract, "responses"));
        for code in ["200", "400", "403", "404", "503"] {
            assert!(
                responses.contains_key(code),
                "the contract read must document {code}"
            );
        }

        let preview = at(at(at(&doc, "paths"), "/schemas/contract-preview"), "post");
        assert_eq!(text(at(preview, "operationId")), "PreviewSchemaContract");
        let body_ref = at(
            at(
                at(
                    at(at(preview, "requestBody"), "content"),
                    "application/json",
                ),
                "schema",
            ),
            "$ref",
        );
        assert_eq!(
            text(body_ref),
            "#/components/schemas/ContractPreviewRequest"
        );
        let responses = mapping(at(preview, "responses"));
        for code in ["200", "400", "403", "404", "503"] {
            assert!(
                responses.contains_key(code),
                "the contract preview must document {code}"
            );
        }
    }
}

#[test]
fn contract_routes_are_lambda_proxy_integrations_on_both_surfaces() {
    for doc in [oauth_contract(), iam_contract()] {
        for (path, method, operation) in [
            ("/schemas/{schemaName}/contract", "get", "GetSchemaContract"),
            ("/schemas/contract-preview", "post", "PreviewSchemaContract"),
        ] {
            let op = at(at(at(&doc, "paths"), path), method);
            let integration = at(op, "x-amazon-apigateway-integration");
            assert_eq!(text(at(integration, "type")), "aws_proxy");
            assert!(
                text(at(at(integration, "uri"), "Fn::Sub"))
                    .contains("${DescribeSchemaFunction.Arn}"),
                "{operation} must integrate with the describe-schema function"
            );
        }
    }
}

#[test]
fn contract_routes_are_wired_to_both_apis_in_sam() {
    let doc = core_template();
    let events = mapping(at(
        props(resource(&doc, "DescribeSchemaFunction")),
        "Events",
    ));
    for (name, path, method, rest_api) in [
        (
            "ContractOAuthApi",
            "/schemas/{schemaName}/contract",
            "GET",
            "KonductorOAuthApi",
        ),
        (
            "ContractIamApi",
            "/schemas/{schemaName}/contract",
            "GET",
            "KonductorIamApi",
        ),
        (
            "PreviewOAuthApi",
            "/schemas/contract-preview",
            "POST",
            "KonductorOAuthApi",
        ),
        (
            "PreviewIamApi",
            "/schemas/contract-preview",
            "POST",
            "KonductorIamApi",
        ),
    ] {
        let event = events
            .get(name)
            .unwrap_or_else(|| panic!("DescribeSchemaFunction missing {name} event"));
        let properties = at(event, "Properties");
        assert_eq!(text(at(properties, "Path")), path);
        assert_eq!(text(at(properties, "Method")), method);
        assert_eq!(text(at(properties, "RestApiId")), rest_api);
    }
}

#[test]
fn contract_preview_request_is_closed_and_carries_no_metadata_fields() {
    for doc in [oauth_contract(), iam_contract()] {
        let schemas = at(at(&doc, "components"), "schemas");
        let request = at(schemas, "ContractPreviewRequest");
        assert!(
            !boolean(at(request, "additionalProperties")),
            "ContractPreviewRequest must reject unknown (platform-owned) fields"
        );
        let mut required: Vec<&str> = seq(at(request, "required")).iter().map(text).collect();
        required.sort_unstable();
        assert_eq!(
            required,
            ["content", "domain", "environment", "schemaName"],
            "schemaVersion is optional and pins the composed stamp"
        );
        assert!(
            !mapping(at(request, "properties")).contains_key("metadata"),
            "the platform owns metadata; a preview body cannot stamp it"
        );

        let preview_response = at(schemas, "ContractPreviewResponse");
        assert!(
            mapping(at(preview_response, "properties")).contains_key("registered"),
            "the preview response must be distinguishable from a registered contract"
        );
    }
}

#[test]
fn composed_contract_schema_exposes_the_document_for_offline_use() {
    for doc in [oauth_contract(), iam_contract()] {
        let contract = at(at(at(&doc, "components"), "schemas"), "ComposedContract");
        let properties = mapping(at(contract, "properties"));
        for key in [
            "businessContent",
            "metadataSchema",
            "generators",
            "composerFormat",
            "composedDocument",
        ] {
            assert!(
                properties.contains_key(key),
                "ComposedContract must expose {key} for offline validation"
            );
        }
    }
}

#[test]
fn catalog_summary_reports_active_metadata_revisions() {
    for doc in [oauth_contract(), iam_contract()] {
        let summary = at(at(at(&doc, "components"), "schemas"), "SchemaSummary");
        let revisions = at(at(summary, "properties"), "metadataRevisions");
        assert_eq!(
            text(at(at(revisions, "additionalProperties"), "type")),
            "integer",
            "metadataRevisions maps environment names to active revisions"
        );
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
fn activation_materializes_each_environment_via_the_shared_function() {
    let doc = core_template();
    let workflow = resource(&doc, "ActivateSubscriptionWorkflow");
    let substitutions = mapping(at(props(workflow), "DefinitionSubstitutions"));
    for key in [
        "MaterializeEnvFunctionArn",
        "EventStreamsTable",
        "KonductorEventBus",
    ] {
        assert!(
            substitutions.contains_key(key),
            "missing substitution {key}"
        );
    }

    // Per-env materialization (resolve latest version, fail-closed coverage
    // re-check, rule upsert, consumer-bus target, zero-events alarm, per-env
    // record merge) lives in one function shared by the activation workflow
    // and the promotion extender (follow-schema-promotion).
    let (actions, managed) = function_policy_shape(&doc, "MaterializeSubscriptionEnvFunction");
    assert!(
        managed
            .iter()
            .any(|p| p.contains("EntitlementsTableReadPolicy")),
        "materialization re-checks entitlement coverage"
    );
    for required in [
        "schemas:DescribeSchema",
        "events:DescribeRule",
        "events:PutRule",
        "events:PutTargets",
        "iam:PassRole",
        "cloudwatch:PutMetricAlarm",
        "dynamodb:UpdateItem",
    ] {
        assert!(
            actions.contains(&required.to_string()),
            "missing {required}"
        );
    }

    // The workflow role no longer creates rules: creation moved into the
    // function with per-env independence; the role keeps teardown
    // (remove-subscription), invocation, and record/status writes.
    let policy = resource(&doc, "SubscriptionWorkflowPolicy");
    let statements = seq(at(at(props(policy), "PolicyDocument"), "Statement"));
    let workflow_actions: Vec<String> = statements
        .iter()
        .flat_map(|s| {
            seq(at(s, "Action"))
                .iter()
                .map(text)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        workflow_actions.contains(&"lambda:InvokeFunction".to_string()),
        "the workflow invokes the materialize function"
    );
    for moved in [
        "schemas:DescribeSchema",
        "events:PutRule",
        "events:PutTargets",
        "cloudwatch:PutMetricAlarm",
        "iam:PassRole",
    ] {
        assert!(
            !workflow_actions.contains(&moved.to_string()),
            "{moved} belongs to the materialize function, not the workflow role"
        );
    }
}

#[test]
fn activation_workflow_maps_materialize_without_compensation() {
    let asl = repo_yaml("event-streams/state-machines/activate-subscription.asl.yaml");
    let states = mapping(at(&asl, "States"));
    let map_state = states
        .iter()
        .find(|(name, _)| name.as_str() == Some("MaterializeEnvironments"))
        .map(|(_, state)| state)
        .expect("MaterializeEnvironments state");
    let item_states = mapping(at(at(map_state, "ItemProcessor"), "States"));
    let invoke = item_states
        .iter()
        .find(|(name, _)| name.as_str() == Some("MaterializeEnv"))
        .map(|(_, state)| state)
        .expect("MaterializeEnv state");
    assert_eq!(
        text(at(invoke, "Resource")),
        "arn:aws:states:::lambda:invoke"
    );
    // lambda:invoke under JSONata yields the payload already parsed; $parse
    // on it throws T0410 and falsely fails the map iteration (e2e, 2026-09-08).
    assert_eq!(
        text(at(invoke, "Output")),
        "{% $states.result.Payload %}",
        "the invoke output must not re-$parse the payload"
    );

    // Per-env independence: no cross-environment compensation anywhere.
    for (name, _) in states.iter().chain(item_states.iter()) {
        let name = name.as_str().expect("state name");
        assert!(
            !name.contains("Rollback")
                && !name.starts_with("Delete")
                && !name.starts_with("Remove"),
            "no cross-environment compensation state {name}"
        );
    }

    // The post-map aggregation touches only status fields; per-env entries
    // are merged by the function, never replaced wholesale.
    let active = states
        .iter()
        .find(|(name, _)| name.as_str() == Some("UpdateStreamActive"))
        .map(|(_, state)| state)
        .expect("UpdateStreamActive state");
    let update_expression = text(at(at(active, "Arguments"), "UpdateExpression"));
    assert!(
        update_expression.contains("REMOVE errorDetail"),
        "activation clears a stale top-level errorDetail"
    );
    assert!(
        !update_expression.contains("environments"),
        "the whole environments map must never be replaced by the aggregation"
    );
    states
        .iter()
        .find(|(name, _)| name.as_str() == Some("UpdateStreamFailed"))
        .expect("UpdateStreamFailed state (typo guard + per-env failures)");
}

#[test]
fn promotion_extender_is_wired_to_schema_publication() {
    let doc = core_template();

    // Least privilege: census reads plus the lifecycle event; rule, alarm,
    // and registry permissions stay on the shared materialize function,
    // which the extender invokes (follow-schema-promotion design §5).
    let (mut actions, managed) = function_policy_shape(&doc, "SubscriptionExtenderFunction");
    assert_eq!(managed, [] as [&str; 0]);
    actions.sort();
    assert_eq!(
        actions,
        [
            "dynamodb:GetItem",
            "dynamodb:Query",
            "events:PutEvents",
            "lambda:InvokeFunction",
            "states:StartExecution",
        ],
        "extender: stream census, consumer account lookup, materialize invoke, lifecycle event, authoring re-pin trigger"
    );

    let func = resource(&doc, "SubscriptionExtenderFunction");
    let env = mapping(at(at(props(func), "Environment"), "Variables"));
    for key in [
        "EVENT_STREAMS_TABLE",
        "DOMAINS_TABLE",
        "MATERIALIZE_FUNCTION_ARN",
        "KONDUKTOR_EVENT_BUS",
    ] {
        assert!(env.contains_key(key), "missing extender env var {key}");
    }

    let rule = resource(&doc, "SchemaVersionPublishedRule");
    assert_eq!(text(at(props(rule), "EventBusName")), "KonductorEventBus");
    let pattern = at(props(rule), "EventPattern");
    assert_eq!(
        text(seq(at(pattern, "source")).first().unwrap()),
        "konductor.schema-management"
    );
    assert_eq!(
        text(seq(at(pattern, "detail-type")).first().unwrap()),
        "SchemaVersionPublished"
    );
    let target = seq(at(props(rule), "Targets"))
        .first()
        .expect("rule target");
    assert!(
        format!("{:?}", at(target, "Arn")).contains("SubscriptionExtenderFunction"),
        "the promotion signal must invoke the extender"
    );
    assert!(
        format!("{:?}", at(at(target, "DeadLetterConfig"), "Arn")).contains("Dlq"),
        "extension is idempotent, so a failed event dead-letters for replay"
    );

    let permission = resource(&doc, "SchemaVersionPublishedRulePermission");
    assert_eq!(
        text(at(props(permission), "Principal")),
        "events.amazonaws.com"
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

// ---- System events (platform -> domain notifications) ----

#[test]
fn event_streams_table_streams_new_and_old_images() {
    let doc = core_template();
    let table = resource(&doc, "EventStreamsTable");
    assert_eq!(
        text(at(
            at(props(table), "StreamSpecification"),
            "StreamViewType"
        )),
        "NEW_AND_OLD_IMAGES",
        "the projector diffs old/new upgradeEvents snapshots"
    );
}

#[test]
fn system_event_projector_mapping_is_bounded_and_partial_batch_aware() {
    let doc = core_template();
    let mapping_res = resource(&doc, "SystemEventProjectorEventSourceMapping");
    assert_eq!(
        text(at(mapping_res, "Type")),
        "AWS::Lambda::EventSourceMapping"
    );
    let p = props(mapping_res);
    assert_eq!(text(at(p, "StartingPosition")), "TRIM_HORIZON");
    assert_eq!(int(at(p, "BatchSize")), 10);
    assert_eq!(int(at(p, "MaximumRetryAttempts")), 3);
    assert_eq!(int(at(p, "MaximumRecordAgeInSeconds")), 3600);
    assert!(boolean(at(p, "BisectBatchOnFunctionError")));
    assert_eq!(text(at(p, "EventSourceArn")), "EventStreamsTable.StreamArn");
    let responses: Vec<&str> = seq(at(p, "FunctionResponseTypes"))
        .iter()
        .map(text)
        .collect();
    assert_eq!(responses, ["ReportBatchItemFailures"]);
    assert_eq!(
        text(at(
            at(at(p, "DestinationConfig"), "OnFailure"),
            "Destination"
        )),
        "SystemEventProjectorFailureBucket.Arn",
        "permanently undeliverable payloads land in the S3 failure bucket"
    );
}

#[test]
fn system_event_archive_retains_ninety_days() {
    let doc = core_template();
    let archive = resource(&doc, "SystemEventBusArchive");
    assert_eq!(int(at(props(archive), "RetentionDays")), 90);
    assert_eq!(text(at(props(archive), "SourceArn")), "SystemEventBus.Arn");
}

#[test]
fn system_event_projector_registers_schemas_before_consuming() {
    let doc = core_template();
    let mapping_res = resource(&doc, "SystemEventProjectorEventSourceMapping");
    assert_eq!(
        text(at(mapping_res, "DependsOn")),
        "SystemSchemaBootstrap",
        "contracts land in the registry before projection begins"
    );
}

#[test]
fn system_event_resources_do_not_depend_on_the_ses_condition() {
    let doc = core_template();
    for logical_id in [
        "SystemEventBus",
        "SystemEventBusArchive",
        "SystemSchemaRegistry",
        "SystemSchemaBootstrapFunction",
        "SystemSchemaBootstrap",
        "SystemEventProjectorFunction",
        "SystemEventProjectorEventSourceMapping",
        "SystemEventProjectorFailureBucket",
        "SystemEventRouterFunction",
        "SystemEventRoutingRule",
        "SystemTopicManagerFunction",
        "SystemEventTargetDlq",
        "DomainOwnerEmailChangeRule",
        "DomainOwnerEmailChangeRulePermission",
    ] {
        let res = resource(&doc, logical_id);
        assert!(
            mapping(res).get("Condition").is_none(),
            "{logical_id} must deploy independently of NotificationsEnabled"
        );
    }
}
#[test]
fn owner_email_change_rule_routes_to_the_topic_manager_unconditioned() {
    let doc = core_template();
    let rule = resource(&doc, "DomainOwnerEmailChangeRule");
    assert_eq!(text(at(props(rule), "EventBusName")), "KonductorEventBus");
    let pattern = at(props(rule), "EventPattern");
    assert_eq!(seq(at(pattern, "detail-type")), &["UpdateDomainSucceeded"]);
    let changed = at(at(pattern, "detail"), "changedFields");
    assert_eq!(seq(changed), &["ownerEmail"]);
    let target = &seq(at(props(rule), "Targets"))[0];
    assert_eq!(text(at(target, "Arn")), "SystemTopicManagerFunction.Arn");
    assert_eq!(
        text(at(at(target, "DeadLetterConfig"), "Arn")),
        "EventBridgeRuleFailureDlq.Arn"
    );
    let transformer = at(target, "InputTransformer");
    let template = text(at(transformer, "InputTemplate"));
    assert!(template.contains("reconcileOwnerEmail"), "{template}");
    assert!(template.contains("<previousOwnerEmail>"), "{template}");

    let permission = resource(&doc, "DomainOwnerEmailChangeRulePermission");
    assert_eq!(
        text(at(props(permission), "SourceArn")),
        "DomainOwnerEmailChangeRule.Arn"
    );
}
#[test]
fn system_events_topic_arn_is_describe_only_and_oauth_iam_identical() {
    for contract in [oauth_contract(), iam_contract()] {
        let schemas = at(at(&contract, "components"), "schemas");
        let detail = at(schemas, "DomainDetail");
        let field = at(at(detail, "properties"), "systemEventsTopicArn");
        assert_eq!(text(at(field, "type")), "string");
        let required: Vec<&str> = detail
            .get("required")
            .map(|r| seq(r).iter().map(text).collect())
            .unwrap_or_default();
        assert!(
            !required.contains(&"systemEventsTopicArn"),
            "recorded only after provisioning or backfill"
        );
        for schema in [
            "DomainSummary",
            "RegisterDomainRequest",
            "UpdateDomainRequest",
        ] {
            assert!(
                mapping(at(at(schemas, schema), "properties"))
                    .get("systemEventsTopicArn")
                    .is_none(),
                "{schema} must not carry or accept the platform-owned topic ARN"
            );
        }
    }
}
#[test]
fn system_events_marker_is_on_summary_and_detail_in_both_contracts() {
    for contract in [oauth_contract(), iam_contract()] {
        let schemas = at(at(&contract, "components"), "schemas");
        for schema in ["DomainSummary", "DomainDetail"] {
            let props = at(at(schemas, schema), "properties");
            for field in ["systemEventsLastEventAt", "systemEventsLastEventType"] {
                assert_eq!(
                    text(at(at(props, field), "type")),
                    "string",
                    "{schema}.{field}"
                );
            }
            let required: Vec<&str> = at(schemas, schema)
                .get("required")
                .map(|r| seq(r).iter().map(text).collect())
                .unwrap_or_default();
            assert!(
                !required.contains(&"systemEventsLastEventAt"),
                "{schema}: marker absent until the first delivered event"
            );
        }
        for schema in ["RegisterDomainRequest", "UpdateDomainRequest"] {
            let props = mapping(at(at(schemas, schema), "properties"));
            assert!(
                props.get("systemEventsLastEventAt").is_none()
                    && props.get("systemEventsLastEventType").is_none(),
                "{schema} must not accept the platform-owned marker"
            );
        }
    }
}
#[test]
fn system_event_email_rules_are_conditioned_and_scoped() {
    let doc = core_template();
    let cases: [(&str, &str, &str, &[&str]); 2] = [
        (
            "SystemEventConsumerEmailRule",
            "SystemEventBus",
            "konductor.system",
            &["SubscriptionUpgradeStarted", "SubscriptionUpgraded"],
        ),
        (
            "SchemaPromotionFailedEmailRule",
            "KonductorEventBus",
            "konductor.schema-management",
            &["SchemaPromotionFailed"],
        ),
    ];
    for (logical_id, bus, source, detail_types) in cases {
        let rule = resource(&doc, logical_id);
        assert_eq!(
            mapping(rule).get("Condition").map(text),
            Some("NotificationsEnabled"),
            "{logical_id} deploys only when notifications are enabled"
        );
        let pattern = at(props(rule), "EventPattern");
        assert_eq!(
            text(at(pattern, "source").get(0).unwrap_or(pattern)),
            source,
            "{logical_id}"
        );
        let declared: Vec<&str> = seq(at(pattern, "detail-type")).iter().map(text).collect();
        assert_eq!(declared, detail_types, "{logical_id}");
        let target = &seq(at(props(rule), "Targets"))[0];
        assert_eq!(
            text(at(target, "Arn")),
            "SystemEventNotifierFunction.Arn",
            "{logical_id}"
        );
        assert_eq!(
            text(at(at(target, "DeadLetterConfig"), "Arn")),
            "SystemEventTargetDlq.Arn",
            "{logical_id}"
        );
        let permission_id = format!("{logical_id}Permission");
        assert_eq!(
            mapping(resource(&doc, &permission_id))
                .get("Condition")
                .map(text),
            Some("NotificationsEnabled"),
            "{permission_id}"
        );
    }
    let consumer = at(
        props(resource(&doc, "SystemEventConsumerEmailRule")),
        "EventPattern",
    );
    let trigger = seq(at(at(at(consumer, "detail"), "data"), "trigger"));
    assert_eq!(
        trigger.iter().map(text).collect::<Vec<_>>(),
        vec!["PROMOTION"],
        "authoring re-pins never produce consumer emails"
    );
}

#[test]
fn system_event_failure_bucket_is_private_encrypted_and_expires() {
    let doc = core_template();
    let bucket = resource(&doc, "SystemEventProjectorFailureBucket");
    let p = props(bucket);
    let block = at(p, "PublicAccessBlockConfiguration");
    for key in [
        "BlockPublicAcls",
        "BlockPublicPolicy",
        "IgnorePublicAcls",
        "RestrictPublicBuckets",
    ] {
        assert!(boolean(at(block, key)));
    }
    let encryption = seq(at(
        at(p, "BucketEncryption"),
        "ServerSideEncryptionConfiguration",
    ));
    assert_eq!(encryption.len(), 1);
    let rules = seq(at(at(p, "LifecycleConfiguration"), "Rules"));
    assert_eq!(rules.len(), 1);
    assert_eq!(int(at(&rules[0], "ExpirationInDays")), 90);
}

#[test]
fn system_event_routing_uses_one_rule_regardless_of_domain_count() {
    let doc = core_template();
    let resources = mapping(at(&doc, "Resources"));
    let rules: Vec<&str> = resources
        .keys()
        .filter_map(|k| k.as_str())
        .filter(|id| {
            let res = resource(&doc, id);
            text(at(res, "Type")) == "AWS::Events::Rule"
                && props(res).get("EventBusName").and_then(|v| v.as_str()) == Some("SystemEventBus")
        })
        .collect();
    assert_eq!(
        rules,
        ["SystemEventRoutingRule", "SystemEventConsumerEmailRule"],
        "constant rules only; domains never add rules"
    );
    assert_eq!(
        mapping(resource(&doc, "SystemEventConsumerEmailRule"))
            .get("Condition")
            .map(text),
        Some("NotificationsEnabled"),
        "the email rule is optional; routing is not"
    );
    let rule = resource(&doc, "SystemEventRoutingRule");
    let targets = seq(at(props(rule), "Targets"));
    assert_eq!(targets.len(), 1);
    assert_eq!(
        text(at(at(&targets[0], "DeadLetterConfig"), "Arn")),
        "SystemEventTargetDlq.Arn"
    );
    let dlq = resource(&doc, "SystemEventTargetDlq");
    assert_eq!(
        int(at(props(dlq), "MessageRetentionPeriod")),
        1209600,
        "14-day SQS maximum retention"
    );
}

#[test]
fn system_event_permissions_stay_scoped() {
    let doc = core_template();
    let resources = mapping(at(&doc, "Resources"));
    // The system bus accepts publishes only from the projector: no
    // EventBusPolicy may widen it, and the router role must not carry
    // events:PutEvents.
    for (id, res) in resources {
        let id = id.as_str().expect("resource id");
        assert_ne!(
            text(at(res, "Type")),
            "AWS::Events::EventBusPolicy",
            "{id}: the system bus must not gain a resource policy"
        );
    }
    let router = resource(&doc, "SystemEventRouterFunction");
    let router_rendered = serde_yaml::to_string(router).unwrap();
    assert!(!router_rendered.contains("events:PutEvents"));
    assert!(router_rendered.contains("sns:Publish"));
    assert!(router_rendered.contains("-system-events"));
    // No stack-managed topic policy may target the system topics; subscribe
    // restrictions live in the helper-set policy, not the stack. (The
    // pre-existing SES notifications topic policy is unrelated.)
    for (id, res) in resources {
        let id = id.as_str().expect("resource id");
        if text(at(res, "Type")) == "AWS::SNS::TopicPolicy" {
            assert!(
                !id.contains("SystemEvent"),
                "{id}: system topic policies are managed by the topic manager, not the stack"
            );
        }
    }
}

#[test]
fn system_event_emission_defaults_to_disabled() {
    let doc = core_template();
    let params = mapping(at(&doc, "Parameters"));
    let param = params
        .get("SystemEventEmissionEnabled")
        .expect("gate parameter");
    assert_eq!(text(at(param, "Default")), "false");
}
