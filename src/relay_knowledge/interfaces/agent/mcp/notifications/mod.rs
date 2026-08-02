//! Handles MCP notifications that affect active request lifecycle.

#[cfg(test)]
mod mod_tests;

use serde::Deserialize;
use serde_json::Value;

use super::{McpServer, json_rpc::request_id_key};

#[derive(Debug, Deserialize)]
struct CancelParams {
    #[serde(rename = "requestId")]
    request_id: Value,
}

pub(super) fn handle_notification(
    server: &McpServer,
    method: &str,
    params: Value,
    namespace: &str,
) -> bool {
    let Some(request_id) = (method == "notifications/cancelled")
        .then(|| serde_json::from_value::<CancelParams>(params).ok())
        .flatten()
        .and_then(|cancel| request_id_key(namespace, &cancel.request_id))
    else {
        return false;
    };
    let cancelled = server.cancellations.cancel(&request_id);
    if cancelled {
        server.qos.record_cancelled();
    }
    cancelled
}
