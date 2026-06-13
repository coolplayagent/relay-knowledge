use serde_json::Value;

use super::{CancelParams, McpServer, request_id_key};

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
