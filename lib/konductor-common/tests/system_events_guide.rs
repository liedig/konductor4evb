//! The consumer guide's embedded examples stay honest: JSON blocks parse, the
//! queue policy is source-topic scoped, and the filter-attribute table matches
//! the attributes the router actually sets.

use konductor_common::system_events::{
    ContractSelector, ContractSelectors, SystemEvent, SystemEventAudience, SystemEventData,
    SystemEventMetadata, SystemEventType, SystemResource, SystemResourceType, UpgradeTrigger,
};

fn guide() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/guides/system-events.md"
    );
    std::fs::read_to_string(path).expect("guide")
}

fn json_blocks(doc: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = doc;
    while let Some(start) = rest.find("```json") {
        let after = &rest[start + 7..];
        let end = after.find("```").expect("closing fence");
        blocks.push(after[..end].trim().to_string());
        rest = &after[end + 3..];
    }
    blocks
}

fn sample_event(terminal: bool) -> SystemEvent {
    let selector = |version| ContractSelector {
        producer_domain: "orders".to_string(),
        schema_name: "acme-orders@OrderNotification".to_string(),
        environment: "prod".to_string(),
        schema_version: version,
        metadata_revision: 0,
    };
    let (event_type, outcome, resulting) = if terminal {
        (
            SystemEventType::SubscriptionUpgraded,
            Some(konductor_common::system_events::UpgradeOutcome::Completed),
            Some(selector(4)),
        )
    } else {
        (SystemEventType::SubscriptionUpgradeStarted, None, None)
    };
    SystemEvent::new(
        event_type,
        "acme-konductor-system-bus".to_string(),
        SystemEventMetadata {
            schema_version: 1,
            idempotency_key: "system-event/v1/abc".to_string(),
            published_at: "2026-07-30T10:30:00.123Z".parse().unwrap(),
            correlation_id: "promotion-42".to_string(),
            resource_revision: 5,
        },
        SystemEventData {
            audience: SystemEventAudience::Consumer,
            domain: "checkout".to_string(),
            environment: Some("prod".to_string()),
            resource: SystemResource {
                resource_type: SystemResourceType::Subscription,
                id: "checkout.orders.OrderNotification".to_string(),
            },
            producer_domain: "orders".to_string(),
            schema_name: "acme-orders@OrderNotification".to_string(),
            trigger: UpgradeTrigger::Promotion,
            from_version: 3,
            requested_version: 4,
            resulting_version: resulting.as_ref().map(|s| s.schema_version),
            intermediate_versions: None,
            enabled: true,
            subscription_status: konductor_common::streams::StreamStatus::Active,
            expected_repin_at: None,
            contracts: ContractSelectors {
                from: selector(3),
                requested: selector(4),
                resulting,
            },
            outcome,
            reason: None,
        },
    )
    .expect("valid fixture")
}

#[test]
fn guide_json_examples_parse() {
    let blocks = json_blocks(&guide());
    assert!(
        blocks.len() >= 2,
        "queue policy and filter examples present"
    );
    for block in blocks {
        serde_json::from_str::<serde_json::Value>(&block).expect("json example parses");
    }
}

#[test]
fn guide_queue_policy_is_source_topic_scoped() {
    let blocks = json_blocks(&guide());
    let policy: serde_json::Value = serde_json::from_str(&blocks[0]).unwrap();
    let statement = &policy["Statement"][0];
    assert_eq!(statement["Action"], "sqs:SendMessage");
    assert_eq!(statement["Principal"]["Service"], "sns.amazonaws.com");
    let source = statement["Condition"]["ArnEquals"]["aws:SourceArn"]
        .as_str()
        .unwrap();
    assert!(
        source.ends_with("-system-events"),
        "queue policy must scope sns:SendMessage to the system topic"
    );
}

#[test]
fn guide_filter_attributes_match_the_router_attributes() {
    let guide = guide();
    let router_keys: std::collections::BTreeSet<String> =
        sample_event(true).sns_attributes().into_keys().collect();
    let documented: std::collections::BTreeSet<String> = [
        "eventType",
        "schemaVersion",
        "audience",
        "domain",
        "environment",
        "resourceType",
        "trigger",
        "outcome",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(
        router_keys, documented,
        "guide table and router attributes must agree"
    );
    for key in &documented {
        assert!(
            guide.contains(&format!("| `{key}` |")),
            "guide documents {key}"
        );
    }
    // Start events omit `outcome`; the guide warns that outcome filters never
    // match them.
    assert!(!sample_event(false).sns_attributes().contains_key("outcome"));
    assert!(guide.contains("never match"));
}

#[test]
fn guide_describes_at_least_once_and_no_archive_replay() {
    let guide = guide();
    assert!(guide.contains("at-least-once"));
    assert!(guide.contains("idempotencyKey"));
    assert!(guide.contains("audit-only"));
    assert!(guide.contains("RawMessageDelivery"));
    assert!(guide.contains("GET /subscriptions"));
}
