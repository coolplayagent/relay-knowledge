use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::project::PROJECT_NAME;

use super::{
    InitializeParams, MCP_PROTOCOL_VERSION, MCP_SESSION_ID_HEADER, McpServer,
    state::{SessionCreateError, SessionLookupError},
};

pub(super) fn is_valid_json_rpc_response(payload: &Value) -> bool {
    let Some(object) = payload.as_object() else {
        return false;
    };
    let has_result = object.contains_key("result");
    let has_error = object.contains_key("error");
    if has_result == has_error {
        return false;
    }

    object.get("id").is_some_and(is_json_rpc_id)
}

pub(super) fn validate_initialize_params(params: Value) -> Result<(), String> {
    let params = serde_json::from_value::<InitializeParams>(params)
        .map_err(|error| format!("invalid initialize params: {error}"))?;
    if params.protocol_version != MCP_PROTOCOL_VERSION {
        return Err(format!(
            "unsupported MCP protocol version '{}'",
            params.protocol_version
        ));
    }
    if !params.capabilities.is_object() {
        return Err("initialize capabilities must be an object".to_owned());
    }
    if params.client_info.name.trim().is_empty() || params.client_info.version.trim().is_empty() {
        return Err("initialize clientInfo requires name and version".to_owned());
    }

    Ok(())
}

pub(super) fn response_message_session_response(
    server: &McpServer,
    headers: &HeaderMap,
) -> Response {
    match server.sessions.require_session(headers) {
        Ok(session) if session.initialized => StatusCode::ACCEPTED.into_response(),
        Ok(_) => StatusCode::BAD_REQUEST.into_response(),
        Err(error) => session_lookup_error_response(error),
    }
}

pub(super) fn session_lookup_error_response(error: SessionLookupError) -> Response {
    match error {
        SessionLookupError::Missing | SessionLookupError::InvalidHeader => {
            StatusCode::BAD_REQUEST.into_response()
        }
        SessionLookupError::Unknown => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(super) fn uninitialized_session_response(id: Option<Value>) -> Response {
    let Some(id) = id else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if is_json_rpc_id(&id) {
        json_rpc_error(id, -32002, "MCP session is not initialized")
    } else {
        invalid_request_id_response()
    }
}

pub(super) fn invalid_request_id_response() -> Response {
    json_rpc_error(Value::Null, -32600, "request id must be a string or number")
}

pub(super) fn session_create_error(id: Value, error: SessionCreateError) -> Response {
    json_rpc_error(id, -32603, format!("failed to create MCP session: {error}"))
}

pub(super) fn initialize_result() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {
            "tools": {},
            "resources": {"listChanged": false},
            "prompts": {"listChanged": false}
        },
        "serverInfo": {
            "name": PROJECT_NAME,
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "MCP tool schemas are static and storage is opened lazily on the first storage-backed tool call. For repository exploration, prefer relay_code_query or relay_code_repository_set_query and follow the explore_budget returned in structuredContent; budget tiers are 0-499 files: 1 call/15000 chars/5 files, 500-4999: 2/30000/10, 5000-14999: 3/45000/15, 15000+: 5/75000/25. Free-text queries are capped at 10000 characters and path filters at 4096 characters."
    })
}

pub(super) fn json_rpc_success(id: Value, result: Value) -> Response {
    json_response(
        StatusCode::OK,
        json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    )
}

pub(super) fn json_rpc_success_with_session(
    id: Value,
    result: Value,
    session_id: &str,
) -> Response {
    let mut response = json_rpc_success(id, result);
    response.headers_mut().insert(
        MCP_SESSION_ID_HEADER,
        HeaderValue::from_str(session_id).expect("generated MCP session id is a valid header"),
    );
    response
}

pub(super) fn json_rpc_error(id: Value, code: i64, message: impl Into<String>) -> Response {
    json_response(
        StatusCode::OK,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message.into()
            }
        }),
    )
}

fn json_response(status: StatusCode, value: Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
        .into_response()
}

pub(super) fn request_id_key(namespace: &str, value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(format!("{namespace}|string:{value}")),
        Value::Number(value) if value.is_i64() || value.is_u64() => {
            Some(format!("{namespace}|number:{value}"))
        }
        _ => None,
    }
}

pub(super) fn is_json_rpc_id(value: &Value) -> bool {
    match value {
        Value::String(_) => true,
        Value::Number(number) => number.is_i64() || number.is_u64(),
        _ => false,
    }
}

#[cfg(test)]
#[path = "json_rpc_tests.rs"]
mod tests;
