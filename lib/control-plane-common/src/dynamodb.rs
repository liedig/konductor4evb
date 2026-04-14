use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

pub fn s(val: &str) -> AttributeValue {
    AttributeValue::S(val.to_string())
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
        key.insert("domainId".to_string(), s("abc-123"));
        key.insert("status".to_string(), s("ACTIVE"));

        let token = serialize_pagination_token(&key);
        let restored = deserialize_pagination_token(&token).unwrap();

        assert_eq!(restored.get("domainId").unwrap().as_s().unwrap(), "abc-123");
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
}
