use serde_json::{Map, Value};

pub fn object_field<'a>(value: &'a Value, name: &str) -> Option<&'a Map<String, Value>> {
    value.get(name).and_then(Value::as_object)
}

pub fn array_field<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub fn string_field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

pub fn string_or<'a>(value: &'a Value, name: &str, default: &'a str) -> &'a str {
    string_field(value, name).unwrap_or(default)
}

pub fn number_or(value: &Value, name: &str, default: u64) -> u64 {
    value.get(name).and_then(Value::as_u64).unwrap_or(default)
}

pub fn string_vec(value: &Value, name: &str) -> Vec<String> {
    array_field(value, name)
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod fields_tests;
