//! Template checks for the metadata configuration slice of
//! `konductor-core-stack.yaml`: the configuration table, the admin and
//! pointer-read managed policies, and the deployment-time bootstrap
//! custom resource.

mod support;

use support::{at, boolean, core_template, int, mapping, props, resource, seq, text};

/// The `Action` list of the single statement of a policy document.
fn policy_actions(policy: &serde_yaml::Value) -> Vec<String> {
    let statements = seq(at(at(policy, "PolicyDocument"), "Statement"));
    assert_eq!(statements.len(), 1, "one statement per metadata policy");
    seq(at(&statements[0], "Action"))
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
    let actions = seq(at(&statements[0], "Action"));
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

#[test]
fn metadata_admin_function_reads_writes_through_the_admin_policy_only() {
    let doc = core_template();
    let function = resource(&doc, "MetadataAdminFunction");
    assert_eq!(text(at(function, "Type")), "AWS::Serverless::Function");

    let props = props(function);
    assert_eq!(
        text(at(props, "CodeUri")),
        "./event-streams/functions/metadata-admin"
    );
    assert_eq!(text(at(props, "Handler")), "bootstrap");

    let env = at(at(props, "Environment"), "Variables");
    assert_eq!(
        text(at(env, "METADATA_TABLE")),
        "MetadataConfigurationTable"
    );
    assert_eq!(text(at(env, "ORGANIZATION")), "Organization");
    assert!(
        mapping(env).contains_key("ENVIRONMENTS"),
        "preview/readiness validate environments against the configured list"
    );

    // The function attaches the metadata admin managed policy; the policy
    // resource stays attachable, never function-inline.
    let policies = seq(at(props, "Policies"));
    assert_eq!(
        policies.iter().map(text).collect::<Vec<_>>(),
        ["MetadataAdminPolicy"]
    );
}

#[test]
fn publish_admission_reads_metadata_through_the_pointer_read_policy() {
    let doc = core_template();
    let function = props(resource(&doc, "PublishEventsAdmissionFunction"));

    let env = at(at(function, "Environment"), "Variables");
    assert_eq!(
        text(at(env, "METADATA_TABLE")),
        "MetadataConfigurationTable",
        "admission resolves the environment pointer from the metadata table"
    );

    let policies = seq(at(function, "Policies"));
    // Managed-policy references are `!Ref Name` scalars; inline statement
    // documents are mappings and stay out of the reference check.
    let referenced: Vec<String> = policies
        .iter()
        .filter(|p| support::untag(p).as_str().is_some())
        .map(|p| text(p).to_string())
        .collect();
    assert!(
        referenced.contains(&"MetadataPointerReadPolicy".to_string()),
        "admission attaches the GetItem-only pointer-read policy, not the admin policy"
    );
    assert!(
        !referenced.contains(&"MetadataAdminPolicy".to_string()),
        "admission must not hold mutation permissions"
    );
}

#[test]
fn metadata_stream_consumer_wires_markers_and_event_publication() {
    let doc = core_template();
    let function = resource(&doc, "MetadataStreamConsumerFunction");
    assert_eq!(text(at(function, "Type")), "AWS::Serverless::Function");

    let props = props(function);
    assert_eq!(
        text(at(props, "CodeUri")),
        "./control-plane/functions/metadata-stream-consumer"
    );
    assert_eq!(text(at(props, "Handler")), "bootstrap");

    let env = at(at(props, "Environment"), "Variables");
    assert_eq!(
        text(at(env, "METADATA_TABLE")),
        "MetadataConfigurationTable"
    );
    assert_eq!(text(at(env, "KONDUKTOR_EVENT_BUS")), "KonductorEventBus");

    let policies = seq(at(props, "Policies"));
    assert_eq!(policies.len(), 1);
    let statements = seq(at(&policies[0], "Statement"));
    assert_eq!(statements.len(), 2);

    let marker_actions = seq(at(&statements[0], "Action"));
    assert_eq!(
        marker_actions.iter().map(text).collect::<Vec<_>>(),
        ["dynamodb:PutItem", "dynamodb:DeleteItem"]
    );
    assert!(is_table_arn(at(&statements[0], "Resource")));

    assert_eq!(
        seq(at(&statements[1], "Action"))
            .iter()
            .map(text)
            .collect::<Vec<_>>(),
        ["events:PutEvents"]
    );
    assert_eq!(
        text(at(&statements[1], "Resource")),
        "KonductorEventBus.Arn",
        "lifecycle events publish to the control-plane bus"
    );
}

#[test]
fn metadata_stream_consumer_esm_is_bounded_with_bisect_and_failure_capture() {
    let doc = core_template();
    let events = at(
        props(resource(&doc, "MetadataStreamConsumerFunction")),
        "Events",
    );
    let esm = at(mapping(events).values().next().unwrap(), "Properties");

    assert_eq!(
        text(at(esm, "Stream")),
        "MetadataConfigurationTable.StreamArn"
    );
    assert_eq!(text(at(esm, "StartingPosition")), "TRIM_HORIZON");
    assert!(boolean(at(esm, "BisectBatchOnFunctionError")));
    assert_eq!(
        int(at(esm, "MaximumRetryAttempts")),
        3,
        "bounded retry before failure capture"
    );

    let on_failure = at(at(esm, "DestinationConfig"), "OnFailure");
    assert_eq!(text(at(on_failure, "Type")), "SQS");
    assert_eq!(
        text(at(on_failure, "Destination")),
        "LambdaFailureDlq.Arn",
        "records that exhaust retries land in the shared failure DLQ"
    );
}

#[test]
fn metadata_stream_consumer_has_an_iterator_age_alarm() {
    let doc = core_template();
    let alarm = props(resource(&doc, "MetadataStreamConsumerIteratorAgeAlarm"));
    assert_eq!(text(at(alarm, "MetricName")), "IteratorAge");
    assert_eq!(text(at(alarm, "Namespace")), "AWS/Lambda");

    let dimensions = seq(at(alarm, "Dimensions"));
    assert_eq!(dimensions.len(), 1);
    assert_eq!(
        text(at(&dimensions[0], "Value")),
        "MetadataStreamConsumerFunction"
    );

    assert_eq!(int(at(alarm, "Threshold")), 60000);
    assert_eq!(
        text(at(alarm, "ComparisonOperator")),
        "GreaterThanThreshold"
    );
}
