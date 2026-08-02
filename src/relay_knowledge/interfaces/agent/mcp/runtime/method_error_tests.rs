//! Direct MCP method error mapping tests.

use crate::{
    api::ApiError,
    interfaces::agent::{AgentAdapterError, AgentAdapterErrorKind},
};

use super::McpMethodError;

#[test]
fn api_errors_preserve_protocol_kind_and_message() {
    let error = McpMethodError::api(ApiError::storage_unavailable("database busy"));

    assert_eq!(error.code, -32000);
    assert_eq!(error.kind, "storage_unavailable");
    assert_eq!(error.message, "database busy");
}

#[test]
fn adapter_errors_preserve_adapter_kind_and_message() {
    let error = McpMethodError::adapter(AgentAdapterError::new(
        AgentAdapterErrorKind::Cancelled,
        "request cancelled",
    ));

    assert_eq!(error.code, -32000);
    assert_eq!(error.kind, "cancelled");
    assert_eq!(error.message, "request cancelled");
}
