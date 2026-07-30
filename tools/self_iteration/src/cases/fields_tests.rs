use super::*;

#[test]
fn typed_fields_accept_only_the_requested_json_shapes() {
    let value = serde_json::json!({
        "object": {"enabled": true},
        "array": ["one", 2, "three"],
        "string": "value",
        "number": 7,
        "wrong_object": [],
        "wrong_array": {},
        "wrong_string": false,
        "wrong_number": -1
    });

    assert_eq!(
        object_field(&value, "object").and_then(|object| object.get("enabled")),
        Some(&serde_json::json!(true))
    );
    assert!(object_field(&value, "wrong_object").is_none());
    assert_eq!(array_field(&value, "array").len(), 3);
    assert!(array_field(&value, "wrong_array").is_empty());
    assert_eq!(string_field(&value, "string"), Some("value"));
    assert!(string_field(&value, "wrong_string").is_none());
    assert_eq!(number_or(&value, "number", 3), 7);
    assert_eq!(number_or(&value, "wrong_number", 3), 3);
}

#[test]
fn field_defaults_and_string_vectors_are_deterministic() {
    let value = serde_json::json!({
        "items": ["one", 2, "three"],
        "present": "configured"
    });

    assert_eq!(string_or(&value, "present", "default"), "configured");
    assert_eq!(string_or(&value, "missing", "default"), "default");
    assert_eq!(number_or(&value, "missing", 9), 9);
    assert_eq!(string_vec(&value, "items"), ["one", "three"]);
    assert!(string_vec(&value, "missing").is_empty());
}
