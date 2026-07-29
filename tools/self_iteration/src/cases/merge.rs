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

#[cfg(test)]
#[path = "merge_tests.rs"]
mod merge_tests;
