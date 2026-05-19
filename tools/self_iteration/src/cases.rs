use std::{collections::BTreeMap, fs, path::Path};

use serde_json::{Map, Value};

pub fn load_cases(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let mut config = serde_json::from_str::<Value>(&text)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    let include_files = config
        .as_object_mut()
        .and_then(|object| object.remove("include_files"))
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    for include_file in include_files {
        let relative = include_file
            .as_str()
            .ok_or_else(|| format!("invalid include file entry in {}", path.display()))?;
        let parent = path.parent().unwrap_or(Path::new("."));
        let included = load_cases(&parent.join(relative))?;
        merge_case_config(&mut config, included)?;
    }
    Ok(config)
}

fn merge_case_config(target: &mut Value, included: Value) -> Result<(), String> {
    match (target, included) {
        (Value::Object(target), Value::Object(included)) => merge_objects(target, included),
        _ => Err("case config roots must be objects".to_owned()),
    }
}

fn merge_objects(
    target: &mut Map<String, Value>,
    included: Map<String, Value>,
) -> Result<(), String> {
    for (key, value) in included {
        match (target.get_mut(&key), value) {
            (Some(Value::Array(target_items)), Value::Array(mut included_items)) => {
                target_items.append(&mut included_items);
            }
            (Some(Value::Object(target_object)), Value::Object(included_object)) => {
                merge_objects(target_object, included_object)?;
            }
            (Some(existing), value) => *existing = value,
            (None, value) => {
                target.insert(key, value);
            }
        }
    }
    Ok(())
}

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

pub fn objects_by_repository(cases: &[Value]) -> BTreeMap<String, Vec<Value>> {
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for case in cases {
        if let Some(repository) = string_field(case, "repository") {
            grouped
                .entry(repository.to_owned())
                .or_default()
                .push(case.clone());
        }
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_appends_arrays_and_merges_maps() {
        let mut target = serde_json::json!({
            "query_cases": [{"id": "one"}],
            "repositories": {"a": {"path": "/a"}}
        });
        let included = serde_json::json!({
            "query_cases": [{"id": "two"}],
            "repositories": {"b": {"path": "/b"}}
        });

        merge_case_config(&mut target, included).expect("merge should succeed");

        assert_eq!(array_field(&target, "query_cases").len(), 2);
        assert!(
            object_field(&target, "repositories")
                .expect("repositories should exist")
                .contains_key("b")
        );
    }
}
