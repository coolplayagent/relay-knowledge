//! Direct cancellable tool-runtime contract tests.

use serde_json::json;

use super::ToolCallParams;

#[test]
fn tool_call_params_default_missing_arguments_to_null() {
    let params = serde_json::from_value::<ToolCallParams>(json!({"name": "relay_health"}))
        .expect("tool call params should decode");

    assert_eq!(params.name, "relay_health");
    assert!(params.arguments.is_null());
}
