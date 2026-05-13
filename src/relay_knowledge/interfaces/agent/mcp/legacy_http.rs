use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex},
};

use axum::{
    body::{Body, Bytes, to_bytes},
    extract::{Query, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::stream;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::{
    MCP_PROTOCOL_VERSION, MCP_PROTOCOL_VERSION_HEADER, MCP_SESSION_ID_HEADER, McpServer,
    admit_mcp_request, endpoint_child, handle_mcp_post, http_contract::validate_origin,
};

const LEGACY_SSE_QUEUE_DEPTH: usize = 32;
const LEGACY_MESSAGE_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Default)]
pub(super) struct LegacySseRegistry {
    active: Arc<Mutex<HashMap<String, mpsc::Sender<Bytes>>>>,
}

pub(super) struct LegacySseRegistration {
    registry: LegacySseRegistry,
    session_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct LegacyMessageQuery {
    #[serde(rename = "sessionId")]
    session_id: String,
}

pub(super) fn sse_endpoint(endpoint: &str) -> String {
    endpoint_child(endpoint, "sse")
}

pub(super) fn message_endpoint(endpoint: &str) -> String {
    endpoint_child(endpoint, "message")
}

pub(super) async fn handle_legacy_sse_get(
    State(server): State<McpServer>,
    headers: HeaderMap,
) -> Response {
    if let Err(status) = validate_origin(&server, &headers) {
        return status.into_response();
    }
    let permit = match admit_mcp_request(&server) {
        Ok(permit) => permit,
        Err(_) => return StatusCode::TOO_MANY_REQUESTS.into_response(),
    };
    let session_id = match server.sessions.create_session() {
        Ok(session_id) => session_id,
        Err(error) => {
            drop(permit);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain")],
                error.to_string(),
            )
                .into_response();
        }
    };
    let endpoint = format!(
        "{}?sessionId={}",
        message_endpoint(&server.agent.mcp_endpoint),
        session_id
    );
    let (receiver, registration) = server.legacy_sse.register(session_id.clone());
    let stream = legacy_sse_stream(sse_frame("endpoint", &endpoint), receiver, registration);

    let mut response = (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/event-stream"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        Body::from_stream(stream),
    )
        .into_response();
    response.headers_mut().insert(
        MCP_SESSION_ID_HEADER,
        HeaderValue::from_str(&session_id).expect("generated MCP session id is a valid header"),
    );
    drop(permit);
    response
}

pub(super) async fn handle_legacy_message_post(
    State(server): State<McpServer>,
    Query(query): Query<LegacyMessageQuery>,
    mut headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Ok(session_id) = HeaderValue::from_str(&query.session_id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    headers.insert(MCP_SESSION_ID_HEADER, session_id);
    insert_default_header(
        &mut headers,
        header::CONTENT_TYPE.as_str(),
        "application/json",
    );
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    insert_default_header(
        &mut headers,
        MCP_PROTOCOL_VERSION_HEADER,
        MCP_PROTOCOL_VERSION,
    );

    let delivery = match server.legacy_sse.reserve(&query.session_id) {
        Ok(delivery) => delivery,
        Err(LegacySsePublishError::NoOpenStream) => return StatusCode::NOT_FOUND.into_response(),
        Err(LegacySsePublishError::QueueFull) => {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
    };

    tokio::spawn(publish_legacy_mcp_response(server, headers, body, delivery));

    StatusCode::ACCEPTED.into_response()
}

async fn publish_legacy_mcp_response(
    server: McpServer,
    headers: HeaderMap,
    body: Bytes,
    delivery: LegacySseDelivery,
) {
    let response = handle_mcp_post(State(server), headers, body).await;
    let status = response.status();
    let body = match to_bytes(response.into_body(), LEGACY_MESSAGE_RESPONSE_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            let message = legacy_json_rpc_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "legacy MCP response exceeded the SSE compatibility limit",
            );
            let _ = delivery.send(sse_frame("message", &message));
            return;
        }
    };
    if body.is_empty() {
        if !status.is_success() {
            let message = legacy_json_rpc_error(status, "legacy MCP request failed");
            let _ = delivery.send(sse_frame("message", &message));
        }
        return;
    }
    let message = legacy_message_body(status, &body);
    let message = message.as_ref();
    let _ = delivery.send(sse_frame("message", message));
}

fn legacy_message_body(status: StatusCode, body: &[u8]) -> String {
    if status.is_success() {
        return String::from_utf8_lossy(body).into_owned();
    }

    if let Ok(value) = serde_json::from_slice::<Value>(body)
        && value.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
        && value.get("error").is_some()
    {
        return value.to_string();
    }

    let details = String::from_utf8_lossy(body);
    legacy_json_rpc_error(status, details.trim())
}

fn legacy_json_rpc_error(status: StatusCode, message: &str) -> String {
    let message = if message.is_empty() {
        "legacy MCP request failed"
    } else {
        message
    };
    json!({
        "jsonrpc": "2.0",
        "id": Value::Null,
        "error": {
            "code": -32000,
            "message": message,
            "data": {"http_status": status.as_u16()}
        }
    })
    .to_string()
}

impl LegacySseRegistry {
    fn register(&self, session_id: String) -> (mpsc::Receiver<Bytes>, LegacySseRegistration) {
        let (sender, receiver) = mpsc::channel(LEGACY_SSE_QUEUE_DEPTH);
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(session_id.clone(), sender);

        (
            receiver,
            LegacySseRegistration {
                registry: self.clone(),
                session_id,
            },
        )
    }

    fn reserve(&self, session_id: &str) -> Result<LegacySseDelivery, LegacySsePublishError> {
        let Some(sender) = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(session_id)
            .cloned()
        else {
            return Err(LegacySsePublishError::NoOpenStream);
        };

        match sender.try_reserve_owned() {
            Ok(permit) => Ok(LegacySseDelivery {
                registry: self.clone(),
                session_id: session_id.to_owned(),
                permit,
            }),
            Err(mpsc::error::TrySendError::Full(_)) => Err(LegacySsePublishError::QueueFull),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.unregister(session_id);
                Err(LegacySsePublishError::NoOpenStream)
            }
        }
    }

    fn unregister(&self, session_id: &str) {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(session_id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacySsePublishError {
    NoOpenStream,
    QueueFull,
}

struct LegacySseDelivery {
    registry: LegacySseRegistry,
    session_id: String,
    permit: mpsc::OwnedPermit<Bytes>,
}

impl LegacySseDelivery {
    fn send(self, frame: Bytes) -> Result<(), LegacySsePublishError> {
        let sender = self.permit.send(frame);
        if sender.is_closed() {
            self.registry.unregister(&self.session_id);
            return Err(LegacySsePublishError::NoOpenStream);
        }

        Ok(())
    }
}

impl Drop for LegacySseRegistration {
    fn drop(&mut self) {
        self.registry.unregister(&self.session_id);
    }
}

struct LegacySseStreamState {
    initial: Option<Bytes>,
    receiver: mpsc::Receiver<Bytes>,
    _registration: LegacySseRegistration,
}

fn legacy_sse_stream(
    initial: Bytes,
    receiver: mpsc::Receiver<Bytes>,
    registration: LegacySseRegistration,
) -> impl futures_util::Stream<Item = Result<Bytes, Infallible>> + Send + 'static {
    stream::unfold(
        LegacySseStreamState {
            initial: Some(initial),
            receiver,
            _registration: registration,
        },
        |mut state| async move {
            if let Some(frame) = state.initial.take() {
                return Some((Ok(frame), state));
            }
            state.receiver.recv().await.map(|frame| (Ok(frame), state))
        },
    )
}

fn sse_frame(event: &str, data: &str) -> Bytes {
    let mut frame = format!("event: {event}\n");
    if data.is_empty() {
        frame.push_str("data: \n");
    } else {
        for line in data.lines() {
            frame.push_str("data: ");
            frame.push_str(line);
            frame.push('\n');
        }
    }
    frame.push('\n');

    Bytes::from(frame)
}

fn insert_default_header(headers: &mut HeaderMap, name: &'static str, value: &str) {
    let name = HeaderName::from_static(name);
    if !headers.contains_key(&name) {
        headers.insert(
            name,
            HeaderValue::from_str(value).expect("static compatibility header value is valid"),
        );
    }
}
