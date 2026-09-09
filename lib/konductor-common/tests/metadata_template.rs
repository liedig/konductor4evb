//! Template checks for the metadata configuration slice of
//! `konductor-core-stack.yaml`: the configuration table, the admin and
//! pointer-read managed policies, and the deployment-time bootstrap
//! custom resource.

mod support;

use support::{at, boolean, core_template, mapping, props, resource, seq, text};

/// The `Action` list of the single statement of a policy document.
fn policy_actions(policy: &serde_yaml::Value) -> Vec<String> {
    let statements = seq(at(at(policy, "PolicyDocument"), "Statement"));
    assert_eq!(statements.len(), 1, "one statement per metadata policy");
    seq(&at(&statements[0], "Action"))
        .iter()
        .map(text)
        .map(String::from)
        .collect()
}

/// True when the value is `!GetAtt MetadataConfigurationTable.Arn` (the
/// short form parses as a single dotted string).
fn is_table_arn(v: &serde_yaml::Value) -> bool {
    text(v) == "MetadataConfigurationTable.Arn"
}

#[test]
fn metadata_table_is_hardened_with_sse_pitr_and_on_demand_billing() {
    let doc = core_template();
    let table = resource(&doc, "MetadataConfigurationTable");
    assert_eq!(text(at(table, "Type")), "AWS::DynamoDB::Table");

    let props = props(table);
    assert_eq!(
        text(at(props, "TableName")),
        "konductor-metadata-configuration"
    );
    assert_eq!(text(at(props, "BillingMode")), "PAY_PER_REQUEST");

    let sse = at(props, "SSESpecification");
    assert!(boolean(at(sse, "SSEEnabled")));
    assert_eq!(text(at(sse, "SSEType")), "KMS");

    let pitr = at(props, "PointInTimeRecoverySpecification");
    assert!(boolean(at(pitr, "PointInTimeRecoveryEnabled")));
}

#[test]
fn metadata_table_stream_view_is_new_image() {
    let doc = core_template();
    let stream = at(
        props(resource(&doc, "MetadataConfigurationTable")),
        "StreamSpecification",
    );
    assert_eq!(
        mapping(stream).len(),
        1,
        "stream spec carries only the view type"
    );
    assert_eq!(text(at(stream, "StreamViewType")), "NEW_IMAGE");
}

#[test]
fn metadata_table_ttl_covers_only_stream_markers() {
    let doc = core_template();
    let ttl = at(
        props(resource(&doc, "MetadataConfigurationTable")),
        "TimeToLiveSpecification",
    );
    assert!(boolean(at(ttl, "Enabled")));
    assert_eq!(text(at(ttl, "AttributeName")), "expiresAt");
}

#[test]
fn metadata_table_key_schema_is_pk_hash_sk_range() {
    let doc = core_template();
    let props = props(resource(&doc, "MetadataConfigurationTable"));

    let attrs = seq(at(props, "AttributeDefinitions"));
    assert_eq!(attrs.len(), 2);
    assert_eq!(text(at(&attrs[0], "AttributeName")), "PK");
    assert_eq!(text(at(&attrs[0], "AttributeType")), "S");
    assert_eq!(text(at(&attrs[1], "AttributeName")), "SK");
    assert_eq!(text(at(&attrs[1], "AttributeType")), "S");

    let keys = seq(at(props, "KeySchema"));
    assert_eq!(keys.len(), 2);
    assert_eq!(text(at(&keys[0], "AttributeName")), "PK");
    assert_eq!(text(at(&keys[0], "KeyType")), "HASH");
    assert_eq!(text(at(&keys[1], "AttributeName")), "SK");
    assert_eq!(text(at(&keys[1], "KeyType")), "RANGE");
}

#[test]
fn metadata_admin_policy_grants_read_query_and_transactions_on_the_table() {
    let doc = core_template();
    let policy = props(resource(&doc, "MetadataAdminPolicy"));

    let mut actions = policy_actions(policy);
    actions.sort();
    assert_eq!(
        actions,
        [
            "dynamodb:GetItem",
            "dynamodb:Query",
            "dynamodb:TransactWriteItems"
        ]
    );

    let statements = seq(at(at(policy, "PolicyDocument"), "Statement"));
    assert!(is_table_arn(at(&statements[0], "Resource")));
}

#[test]
fn metadata_pointer_read_policy_is_getitem_only_on_the_table() {
    let doc = core_template();
    let policy = props(resource(&doc, "MetadataPointerReadPolicy"));

    let actions = policy_actions(policy);
    assert_eq!(actions, ["dynamodb:GetItem"]);

    let statements = seq(at(at(policy, "PolicyDocument"), "Statement"));
    assert!(is_table_arn(at(&statements[0], "Resource")));
}

#[test]
fn metadata_bootstrap_function_puts_to_the_table_only() {
    let doc = core_template();
    let function = resource(&doc, "MetadataBootstrapFunction");
    assert_eq!(text(at(function, "Type")), "AWS::Serverless::Function");

    let props = props(function);
    assert_eq!(
        text(at(props, "CodeUri")),
        "./event-streams/functions/metadata-bootstrap"
    );
    assert_eq!(text(at(props, "Handler")), "bootstrap");

    let env = at(at(props, "Environment"), "Variables");
    assert_eq!(
        text(at(env, "METADATA_TABLE")),
        "MetadataConfigurationTable"
    );
    assert!(
        mapping(env).contains_key("ENVIRONMENTS"),
        "the bootstrap receives the platform environment list"
    );

    let policies = seq(at(props, "Policies"));
    assert_eq!(policies.len(), 1);
    let statements = seq(at(&policies[0], "Statement"));
    assert_eq!(statements.len(), 1);
    let actions = seq(&at(&statements[0], "Action"));
    assert_eq!(
        actions.iter().map(text).collect::<Vec<_>>(),
        ["dynamodb:PutItem"]
    );
    assert!(is_table_arn(at(&statements[0], "Resource")));
}

#[test]
fn metadata_bootstrap_custom_resource_targets_the_function() {
    let doc = core_template();
    let custom = resource(&doc, "MetadataBootstrap");
    assert_eq!(
        text(at(custom, "Type")),
        "AWS::CloudFormation::CustomResource"
    );

    let token = at(props(custom), "ServiceToken");
    assert_eq!(
        text(token),
        "MetadataBootstrapFunction.Arn",
        "ServiceToken must be the bootstrap function ARN"
    );
}

#[test]
fn stack_exposes_metadata_table_and_pointer_policy_outputs() {
    let doc = core_template();
    let outputs = at(&doc, "Outputs");
    assert_eq!(
        text(at(at(outputs, "MetadataConfigurationTableName"), "Value")),
        "MetadataConfigurationTable"
    );
    assert_eq!(
        text(at(at(outputs, "MetadataPointerReadPolicyArn"), "Value")),
        "MetadataPointerReadPolicy"
    );
}
