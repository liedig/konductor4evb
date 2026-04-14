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
