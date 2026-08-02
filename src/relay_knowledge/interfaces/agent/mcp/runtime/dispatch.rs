use std::time::Instant;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    interfaces::agent::{AgentAdapterError, AgentAdapterErrorKind},
    net::qos::{QosPermit, RejectReason},
};

use super::super::{
    audit_bridge::{record_mcp_qos_rejection, record_mcp_tool_audit},
    http_contract::{validate_http_headers, validate_origin, validate_protocol_version_header},
    json_rpc::{
        initialize_result, invalid_request_id_response, is_json_rpc_id, is_valid_json_rpc_response,
        json_rpc_error, json_rpc_success, json_rpc_success_with_session, request_id_key,
        response_message_session_response, session_create_error, session_lookup_error_response,
        uninitialized_session_response, validate_initialize_params,
    },
    metrics, notifications, prompts, resources,
    state::SessionLookupError,
    tool_contract::tool_error_result,
    tool_registry::is_known_tool,
};
use super::{
    server::McpServer,
    tool_runtime::{ToolCallParams, elapsed_millis, run_cancellable_tool_call},
};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: Option<String>,
    #[serde(default)]
    params: Value,
}

pub(super) async fn handle_mcp_post(
    State(server): State<McpServer>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(status) = validate_http_headers(&server, &headers) {
        return status.into_response();
    }
    if body.len() as u64 > server.network.current().http.max_request_body_bytes {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    let payload = match serde_json::from_slice::<Value>(&body) {
        Ok(payload) => payload,
        Err(error) => {
            return json_rpc_error(Value::Null, -32700, format!("parse error: {error}"));
        }
    };
    if payload.is_array() {
        return json_rpc_error(Value::Null, -32600, "batch requests are not supported");
    }
    let request = match serde_json::from_value::<JsonRpcRequest>(payload.clone()) {
        Ok(request) => request,
        Err(error) => {
            return json_rpc_error(Value::Null, -32600, format!("invalid request: {error}"));
        }
    };
    let id = request.id.clone().unwrap_or(Value::Null);
    if request.jsonrpc.as_deref() != Some("2.0") {
        return json_rpc_error(id, -32600, "jsonrpc must be 2.0");
    }
    let Some(method) = request.method.as_deref() else {
        if is_valid_json_rpc_response(&payload) {
            if let Err(status) = validate_protocol_version_header(&headers, true) {
                return status.into_response();
            }
            return response_message_session_response(&server, &headers);
        }
        if payload
            .as_object()
            .is_some_and(|object| object.contains_key("result") || object.contains_key("error"))
        {
            return StatusCode::BAD_REQUEST.into_response();
        }
        return json_rpc_error(id, -32600, "method is required");
    };

    if method == "initialize" {
        let Some(id) = request.id else {
            return json_rpc_error(Value::Null, -32600, "requests must include an id");
        };
        if !is_json_rpc_id(&id) {
            return invalid_request_id_response();
        }
        if let Err(message) = validate_initialize_params(request.params) {
            return json_rpc_error(id, -32602, message);
        }
        let Ok(permit) = admit_mcp_request(&server) else {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        };
        let session_id = match server.sessions.require_session(&headers) {
            Ok(session) => session.session_id().to_owned(),
            Err(SessionLookupError::Missing) => match server.sessions.create_session() {
                Ok(session_id) => session_id,
                Err(error) => {
                    drop(permit);
                    return session_create_error(id, error);
                }
            },
            Err(error) => {
                drop(permit);
                return session_lookup_error_response(error);
            }
        };
        drop(permit);
        return json_rpc_success_with_session(id, initialize_result(), &session_id);
    }

    if let Err(status) = validate_protocol_version_header(&headers, true) {
        return status.into_response();
    }

    let session = match server.sessions.require_session(&headers) {
        Ok(session) => session,
        Err(error) => return session_lookup_error_response(error),
    };

    if method == "notifications/initialized" {
        if request.id.is_some() {
            return json_rpc_error(id, -32600, "notifications must not include an id");
        }
        let Ok(permit) = admit_mcp_request(&server) else {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        };
        if let Err(error) = server.sessions.mark_initialized(session.session_id()) {
            drop(permit);
            return session_lookup_error_response(error);
        }
        drop(permit);
        return StatusCode::ACCEPTED.into_response();
    }

    if !session.initialized {
        return uninitialized_session_response(request.id);
    }

    let namespace = session.namespace();
    if method.starts_with("notifications/") {
        if request.id.is_some() {
            return json_rpc_error(id, -32600, "notifications must not include an id");
        }
        if method == "notifications/cancelled" {
            notifications::handle_notification(&server, method, request.params, &namespace);
            return StatusCode::ACCEPTED.into_response();
        }
        let Ok(permit) = admit_mcp_request(&server) else {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        };
        notifications::handle_notification(&server, method, request.params, &namespace);
        drop(permit);
        return StatusCode::ACCEPTED.into_response();
    }

    let Some(id) = request.id else {
        return json_rpc_error(Value::Null, -32600, "requests must include an id");
    };
    let Some(request_id) = request_id_key(&namespace, &id) else {
        return invalid_request_id_response();
    };
    let permit = match admit_mcp_request(&server) {
        Ok(permit) => permit,
        Err(reason) => {
            let error =
                AgentAdapterError::new(AgentAdapterErrorKind::QosRejected, qos_message(reason));
            record_mcp_qos_rejection(&server, method, &id, error.kind.as_str());
            server.metrics.record_rejection("mcp", error.kind.as_str());
            return if method == "tools/call" {
                json_rpc_success(id, tool_error_result(error))
            } else {
                json_rpc_error(id, -32000, error.to_string())
            };
        }
    };

    let started = Instant::now();
    let mut pending_tool_audit = None;
    let result = match method {
        "ping" => json!({}),
        "tools/list" => metrics::tools_list_result(&server, &session),
        "resources/list" => resources::list_resources(&server),
        "resources/read" => {
            match resources::read_resource_with_timeout(&server, request.params, &request_id).await
            {
                Ok(result) => result,
                Err(error) => return json_rpc_error(id, error.code, error.message),
            }
        }
        "prompts/list" => prompts::list_prompts(),
        "prompts/get" => match prompts::get_prompt(&server, request.params, &request_id).await {
            Ok(result) => result,
            Err(error) => return json_rpc_error(id, error.code, error.message),
        },
        "tools/call" => {
            let params = match serde_json::from_value::<ToolCallParams>(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return json_rpc_error(
                        id,
                        -32602,
                        format!("invalid tools/call params: {error}"),
                    );
                }
            };
            if !is_known_tool(&params.name) {
                return json_rpc_error(id, -32602, "unknown tool name");
            }
            let outcome = run_cancellable_tool_call(&server, params, request_id).await;
            pending_tool_audit = Some((
                outcome.operation,
                outcome.request_id,
                outcome.result.clone(),
                outcome.duration_ms,
            ));
            outcome.result
        }
        _ => return json_rpc_error(id, -32601, "method not found"),
    };

    drop(permit);
    if let Some((operation, request_id, result, duration_ms)) = pending_tool_audit {
        record_mcp_tool_audit(&server, &operation, &request_id, &result, duration_ms).await;
    } else if !matches!(method, "resources/read" | "prompts/get") {
        server
            .metrics
            .record_request("mcp", method, "completed", elapsed_millis(started), false);
    }
    json_rpc_success(id, result)
}

pub(super) async fn handle_mcp_delete(
    State(server): State<McpServer>,
    headers: HeaderMap,
) -> Response {
    if let Err(status) = validate_origin(&server, &headers) {
        return status.into_response();
    }
    if let Err(status) = validate_protocol_version_header(&headers, true) {
        return status.into_response();
    }
    let permit = match admit_mcp_request(&server) {
        Ok(permit) => permit,
        Err(_) => {
            server.metrics.record_rejection("mcp", "qos_rejected");
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
    };
    match server.sessions.terminate_session(&headers) {
        Ok(()) => {
            drop(permit);
            StatusCode::ACCEPTED.into_response()
        }
        Err(error) => {
            drop(permit);
            session_lookup_error_response(error)
        }
    }
}

pub(in crate::interfaces::agent::mcp) fn admit_mcp_request(
    server: &McpServer,
) -> Result<QosPermit, RejectReason> {
    let policy = server.network.current().qos;
    if crate::net::http::qos_request_context_active() {
        return Ok(QosPermit::already_admitted(server.qos.clone()));
    }
    server.qos.admit_queued_request(&policy)
}

fn qos_message(reason: RejectReason) -> &'static str {
    match reason {
        RejectReason::ConnectionBudgetExceeded => "connection budget exhausted",
        RejectReason::RequestBudgetExceeded => "request budget exhausted",
        RejectReason::QueueBudgetExceeded => "queue budget exhausted",
    }
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
