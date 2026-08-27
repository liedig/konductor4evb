use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

use crate::models::PerEnvResourceArns;

pub fn s(val: &str) -> AttributeValue {
    AttributeValue::S(val.to_string())
}

pub fn n(val: i64) -> AttributeValue {
    AttributeValue::N(val.to_string())
}

/// Hard ceiling on domains per deployment. Fixed by design: quota planning,
/// monitoring bounds, and blast radius all hang off this number.
pub const DOMAIN_COUNT_CAP: i64 = 1000;

/// Partition key of the domain-count counter item. Domain names are lowercase
/// alphanumerics and hyphens only, so this key can never collide with one.
pub const DOMAIN_COUNT_COUNTER_KEY: &str = "META#domain-count";

/// Transact-write item that atomically increments the domain-count counter,
/// conditioned below the cap. Pair it with a domain-row `Put` conditioned on
/// the name being absent so a new row and its slot commit or abort together.
/// The counter self-seeds from zero on the first registration of a fresh
/// deployment; pre-existing deployments must seed it (see deployment docs).
pub fn domain_count_increment(
    table: &str,
) -> Result<aws_sdk_dynamodb::types::Update, aws_sdk_dynamodb::error::BuildError> {
    aws_sdk_dynamodb::types::Update::builder()
        .table_name(table)
        .key("domainName", s(DOMAIN_COUNT_COUNTER_KEY))
        .update_expression(
            "SET #count = if_not_exists(#count, :zero) + :one, #itemType = :itemType",
        )
        .condition_expression("attribute_not_exists(domainName) OR #count < :cap")
        .expression_attribute_names("#count", "count")
        .expression_attribute_names("#itemType", "itemType")
        .expression_attribute_values(":zero", n(0))
        .expression_attribute_values(":one", n(1))
        .expression_attribute_values(":cap", n(DOMAIN_COUNT_CAP))
        .expression_attribute_values(":itemType", s("DomainCount"))
        .build()
}

pub fn get_s(item: &HashMap<String, AttributeValue>, key: &str) -> Option<String> {
    item.get(key).and_then(|v| v.as_s().ok()).cloned()
}

pub fn get_m<'a>(
    item: &'a HashMap<String, AttributeValue>,
    key: &str,
) -> Option<&'a HashMap<String, AttributeValue>> {
    item.get(key).and_then(|v| v.as_m().ok())
}

pub fn string_map_to_attr(map: &HashMap<String, String>) -> AttributeValue {
    let inner: HashMap<String, AttributeValue> =
        map.iter().map(|(k, v)| (k.clone(), s(v))).collect();
    AttributeValue::M(inner)
}

pub fn attr_to_string_map(
    item: &HashMap<String, AttributeValue>,
    key: &str,
) -> Option<HashMap<String, String>> {
    get_m(item, key).map(|m: &HashMap<String, AttributeValue>| {
        m.iter()
            .filter_map(|(k, v)| {
                let val: &String = v.as_s().ok()?;
                Some((k.clone(), val.clone()))
            })
            .collect()
    })
}

pub fn get_string_list(item: &HashMap<String, AttributeValue>, key: &str) -> Option<Vec<String>> {
    item.get(key).and_then(|v| v.as_l().ok()).map(|items| {
        items
            .iter()
            .filter_map(|av| av.as_s().ok().cloned())
            .collect()
    })
}

pub fn get_string_set(item: &HashMap<String, AttributeValue>, key: &str) -> Vec<String> {
    item.get(key)
        .and_then(|v| v.as_ss().ok())
        .cloned()
        .unwrap_or_default()
}

pub fn string_list_to_attr(list: &[String]) -> AttributeValue {
    AttributeValue::L(list.iter().map(|v| s(v)).collect())
}

fn per_env_arns_to_attr(arns: &PerEnvResourceArns) -> AttributeValue {
    let mut map: HashMap<String, AttributeValue> = HashMap::new();
    if let Some(v) = &arns.platform_event_bus_arn {
        map.insert("platformEventBusArn".to_string(), s(v));
    }
    if let Some(v) = &arns.app_event_bus_arn {
        map.insert("appEventBusArn".to_string(), s(v));
    }
    if let Some(v) = &arns.schema_registry_arn {
        map.insert("schemaRegistryArn".to_string(), s(v));
    }
    if let Some(v) = &arns.subscription_dlq_arn {
        map.insert("subscriptionDlqArn".to_string(), s(v));
    }
    if let Some(v) = &arns.integration_dlq_arn {
        map.insert("integrationDlqArn".to_string(), s(v));
    }
    if let Some(v) = &arns.dashboard_arn {
        map.insert("dashboardArn".to_string(), s(v));
    }
    AttributeValue::M(map)
}

fn attr_to_per_env_arns(inner: &HashMap<String, AttributeValue>) -> PerEnvResourceArns {
    PerEnvResourceArns {
        platform_event_bus_arn: get_s(inner, "platformEventBusArn"),
        app_event_bus_arn: get_s(inner, "appEventBusArn"),
        schema_registry_arn: get_s(inner, "schemaRegistryArn"),
        subscription_dlq_arn: get_s(inner, "subscriptionDlqArn"),
        integration_dlq_arn: get_s(inner, "integrationDlqArn"),
        dashboard_arn: get_s(inner, "dashboardArn"),
    }
}

pub fn resource_arns_map_to_attr(map: &HashMap<String, PerEnvResourceArns>) -> AttributeValue {
    let outer: HashMap<String, AttributeValue> = map
        .iter()
        .map(|(env, arns)| (env.clone(), per_env_arns_to_attr(arns)))
        .collect();
    AttributeValue::M(outer)
}

pub fn attr_to_resource_arns_map(
    item: &HashMap<String, AttributeValue>,
    key: &str,
) -> Option<HashMap<String, PerEnvResourceArns>> {
    get_m(item, key).map(|m| {
        m.iter()
            .filter_map(|(env, av)| {
                let inner = av.as_m().ok()?;
                Some((env.clone(), attr_to_per_env_arns(inner)))
            })
            .collect()
    })
}

pub fn serialize_pagination_token(key: &HashMap<String, AttributeValue>) -> String {
    let token_map: HashMap<String, String> = key
        .iter()
        .filter_map(|(k, v)| {
            let val: &String = v.as_s().ok()?;
            Some((k.clone(), val.clone()))
        })
        .collect();
    serde_json::to_string(&token_map).unwrap_or_default()
}

pub fn deserialize_pagination_token(token: &str) -> Option<HashMap<String, AttributeValue>> {
    let map: HashMap<String, String> = serde_json::from_str(token).ok()?;
    Some(map.into_iter().map(|(k, v)| (k, s(&v))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s_creates_string_attribute() {
        let attr = s("hello");
        assert_eq!(attr.as_s().unwrap(), "hello");
    }

    #[test]
    fn n_creates_number_attribute() {
        let attr = n(42);
        assert_eq!(attr.as_n().unwrap(), "42");
    }

    #[test]
    fn counter_key_is_outside_the_domain_name_keyspace() {
        for c in DOMAIN_COUNT_COUNTER_KEY.chars() {
            assert!(
                c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || c == '-'
                    || c.is_ascii_uppercase()
                    || c == '#',
                "unexpected character in counter key"
            );
        }
        assert!(
            DOMAIN_COUNT_COUNTER_KEY
                .chars()
                .any(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')),
            "counter key must be unrepresentable as a domain name"
        );
    }

    #[test]
    fn counter_increment_is_conditional_below_the_cap() {
        let update = domain_count_increment("domains").unwrap();

        assert_eq!(update.table_name(), "domains");
        assert_eq!(
            update.key().get("domainName").unwrap().as_s().unwrap(),
            DOMAIN_COUNT_COUNTER_KEY
        );
        assert_eq!(
            update.condition_expression().unwrap(),
            "attribute_not_exists(domainName) OR #count < :cap"
        );
        assert!(update.update_expression().contains("if_not_exists"));

        let values = update.expression_attribute_values().unwrap();
        assert_eq!(values.get(":cap").unwrap().as_n().unwrap(), "1000");
        assert_eq!(values.get(":one").unwrap().as_n().unwrap(), "1");
        assert_eq!(values.get(":zero").unwrap().as_n().unwrap(), "0");

        let names = update.expression_attribute_names().unwrap();
        assert_eq!(names.get("#count").unwrap().as_str(), "count");
    }

    #[test]
    fn get_s_extracts_string_from_item() {
        let mut item = HashMap::new();
        item.insert("name".to_string(), s("billing"));
        assert_eq!(get_s(&item, "name"), Some("billing".to_string()));
    }

    #[test]
    fn get_s_returns_none_for_missing_key() {
        let item = HashMap::new();
        assert_eq!(get_s(&item, "name"), None);
    }

    #[test]
    fn get_m_extracts_map_from_item() {
        let mut inner = HashMap::new();
        inner.insert("key".to_string(), s("value"));
        let mut item = HashMap::new();
        item.insert("tags".to_string(), AttributeValue::M(inner));
        let result = get_m(&item, "tags");
        assert!(result.is_some());
        assert_eq!(result.unwrap().get("key").unwrap().as_s().unwrap(), "value");
    }

    #[test]
    fn get_m_returns_none_for_missing_key() {
        let item = HashMap::new();
        assert!(get_m(&item, "tags").is_none());
    }

    #[test]
    fn string_map_round_trips_through_attribute_value() {
        let mut map = HashMap::new();
        map.insert("env".to_string(), "prod".to_string());
        map.insert("team".to_string(), "platform".to_string());

        let attr = string_map_to_attr(&map);
        let mut item = HashMap::new();
        item.insert("tags".to_string(), attr);

        let result = attr_to_string_map(&item, "tags").unwrap();
        assert_eq!(result.get("env").unwrap(), "prod");
        assert_eq!(result.get("team").unwrap(), "platform");
    }

    #[test]
    fn attr_to_string_map_returns_none_for_missing_key() {
        let item = HashMap::new();
        assert!(attr_to_string_map(&item, "tags").is_none());
    }

    #[test]
    fn pagination_token_round_trips() {
        let mut key = HashMap::new();
        key.insert("domainName".to_string(), s("billing"));
        key.insert("status".to_string(), s("ACTIVE"));

        let token = serialize_pagination_token(&key);
        let restored = deserialize_pagination_token(&token).unwrap();

        assert_eq!(
            restored.get("domainName").unwrap().as_s().unwrap(),
            "billing"
        );
        assert_eq!(restored.get("status").unwrap().as_s().unwrap(), "ACTIVE");
    }

    #[test]
    fn deserialize_pagination_token_returns_none_for_empty_string() {
        assert!(deserialize_pagination_token("").is_none());
    }

    #[test]
    fn deserialize_pagination_token_returns_none_for_malformed_json() {
        assert!(deserialize_pagination_token("not json {").is_none());
    }

    #[test]
    fn string_list_round_trips_through_attribute_value() {
        let list = vec!["dev".to_string(), "staging".to_string(), "prod".to_string()];
        let attr = string_list_to_attr(&list);
        let mut item = HashMap::new();
        item.insert("environments".to_string(), attr);

        let restored = get_string_list(&item, "environments").unwrap();
        assert_eq!(restored, list);
    }

    #[test]
    fn get_string_list_returns_none_for_missing_key() {
        let item = HashMap::new();
        assert!(get_string_list(&item, "environments").is_none());
    }

    #[test]
    fn get_string_set_extracts_ss_from_item() {
        let mut item = HashMap::new();
        item.insert(
            "secondaryOwnerEmails".to_string(),
            AttributeValue::Ss(vec!["a@x.com".into(), "b@x.com".into()]),
        );
        let result = get_string_set(&item, "secondaryOwnerEmails");
        assert_eq!(result, vec!["a@x.com", "b@x.com"]);
    }

    #[test]
    fn get_string_set_returns_empty_for_missing_key() {
        let item = HashMap::new();
        assert!(get_string_set(&item, "secondaryOwnerEmails").is_empty());
    }

    #[test]
    fn resource_arns_map_round_trips_through_attribute_value() {
        let mut input: HashMap<String, PerEnvResourceArns> = HashMap::new();
        input.insert(
            "dev".to_string(),
            PerEnvResourceArns {
                platform_event_bus_arn: Some("arn:dev:platform".to_string()),
                app_event_bus_arn: Some("arn:dev:app".to_string()),
                schema_registry_arn: Some("arn:dev:schema".to_string()),
                subscription_dlq_arn: Some("arn:dev:subdlq".to_string()),
                integration_dlq_arn: Some("arn:dev:intdlq".to_string()),
                dashboard_arn: Some("arn:dev:dash".to_string()),
            },
        );
        input.insert(
            "prod".to_string(),
            PerEnvResourceArns {
                platform_event_bus_arn: Some("arn:prod:platform".to_string()),
                app_event_bus_arn: None,
                schema_registry_arn: None,
                subscription_dlq_arn: None,
                integration_dlq_arn: None,
                dashboard_arn: None,
            },
        );

        let attr = resource_arns_map_to_attr(&input);
        let mut item = HashMap::new();
        item.insert("resourceArns".to_string(), attr);

        let restored = attr_to_resource_arns_map(&item, "resourceArns").unwrap();
        assert_eq!(restored.get("dev"), input.get("dev"));
        assert_eq!(restored.get("prod"), input.get("prod"));
        assert_eq!(restored.len(), 2);
    }

    #[test]
    fn attr_to_resource_arns_map_returns_none_for_missing_key() {
        let item = HashMap::new();
        assert!(attr_to_resource_arns_map(&item, "resourceArns").is_none());
    }
}
