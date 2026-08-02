//! Direct contracts for JSON-RPC identity and response envelopes.

use serde_json::{Value, json};

use super::request_id_key;

#[test]
fn request_id_keys_preserve_json_rpc_id_type() {
    assert_ne!(
        request_id_key("session:a", &json!("1")),
        request_id_key("session:a", &json!(1))
    );
    assert_ne!(
        request_id_key("session:a", &json!("1")),
        request_id_key("session:b", &json!("1"))
    );
    assert_eq!(
        request_id_key("session:a", &json!("1")),
        Some("session:a|string:1".to_owned())
    );
    assert_eq!(
        request_id_key("session:a", &json!(1)),
        Some("session:a|number:1".to_owned())
    );
    assert_eq!(request_id_key("session:a", &json!(1.5)), None);
    assert_eq!(request_id_key("session:a", &Value::Null), None);
}
