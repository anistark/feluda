use serde_json::Value as JsonValue;

pub fn get_string(obj: &JsonValue, key: &str) -> Option<String> {
    obj.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

pub fn get_array(obj: &JsonValue, key: &str) -> Option<Vec<JsonValue>> {
    obj.get(key).and_then(|v| v.as_array()).cloned()
}

pub fn get_object(obj: &JsonValue, key: &str) -> Option<JsonValue> {
    obj.get(key).cloned()
}

pub fn has_key(obj: &JsonValue, key: &str) -> bool {
    obj.get(key).is_some()
}

pub fn is_valid_iso_datetime(datetime: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(datetime).is_ok()
        || chrono::NaiveDateTime::parse_from_str(datetime, "%Y-%m-%dT%H:%M:%SZ").is_ok()
}
