use serde_json::json;

use super::*;

#[test]
fn typed_case_fields_return_safe_defaults() {
    let value = json!({"items": [1, 2], "limit": 4});

    assert_eq!(array_field(&value, "items").len(), 2);
    assert!(array_field(&value, "missing").is_empty());
    assert_eq!(usize_field(&value, "limit", 1), 4);
    assert_eq!(usize_field(&value, "missing", 3), 3);
}
