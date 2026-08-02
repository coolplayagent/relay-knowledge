use serde_json::json;

use super::CancelParams;
use crate::interfaces::agent::mcp::json_rpc::request_id_key;

#[test]
fn cancellation_params_preserve_typed_request_identity() {
    let string_params: CancelParams =
        serde_json::from_value(json!({"requestId": "call"})).expect("string request id");
    let numeric_params: CancelParams =
        serde_json::from_value(json!({"requestId": 7})).expect("numeric request id");

    assert_eq!(
        request_id_key("session:rk", &string_params.request_id).as_deref(),
        Some("session:rk|string:call")
    );
    assert_eq!(
        request_id_key("session:rk", &numeric_params.request_id).as_deref(),
        Some("session:rk|number:7")
    );
}

#[test]
fn cancellation_params_reject_missing_request_identity() {
    assert!(serde_json::from_value::<CancelParams>(json!({"reason": "late"})).is_err());
}
