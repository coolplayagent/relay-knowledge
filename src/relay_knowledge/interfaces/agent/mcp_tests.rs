use std::sync::Arc;

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header},
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::{
    api::{IngestEvidence, IngestRequest},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

use super::mcp_test_support::SlowSearchStore;

#[tokio::test]
async fn initialize_and_tools_list_hide_refresh_when_policy_disables_it() {
    let server = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut router = server.router();

    let initialize = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "init",
            "method": "initialize",
            "params": initialize_params()
        }),
    )
    .await;
    let tools = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "tools",
            "method": "tools/list",
            "params": {}
        }),
    )
    .await;

    assert_eq!(
        initialize["result"]["protocolVersion"],
        MCP_PROTOCOL_VERSION
    );
    assert_eq!(initialize["result"]["capabilities"]["tools"], json!({}));
    assert!(tool_names(&tools).contains(&"relay.retrieve_context".to_owned()));
    assert!(tool_names(&tools).contains(&"relay.code_query".to_owned()));
    assert!(tool_names(&tools).contains(&"relay.code_impact".to_owned()));
    assert!(!tool_names(&tools).contains(&"relay.refresh_indexes".to_owned()));
}

#[tokio::test]
async fn session_lifecycle_requires_initialized_session_and_supports_ping() {
    let server = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut router = server.router();

    let missing_session = raw_mcp_request(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "tools", "method": "tools/list"}),
        [],
    )
    .await;
    let unknown_session = raw_mcp_request(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "tools", "method": "tools/list"}),
        [(MCP_SESSION_ID_HEADER, "rk-unknown")],
    )
    .await;
    let invalid_initialize_id = raw_mcp_request(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": {},
            "method": "initialize",
            "params": initialize_params()
        }),
        [],
    )
    .await;
    let invalid_initialize = raw_mcp_request(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "bad-init",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0.1.0"}
            }
        }),
        [],
    )
    .await;
    let (initialize_status, initialize_headers, _) = raw_mcp_response(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "init", "method": "initialize", "params": initialize_params()}),
        [],
    )
    .await;
    let session_id = initialize_headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("initialize should issue session")
        .to_owned();
    let before_initialized = raw_mcp_request(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "tools", "method": "tools/list"}),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    let initialized = raw_mcp_request(
        &mut router,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    let invalid_request_id = raw_mcp_request(
        &mut router,
        json!({"jsonrpc": "2.0", "id": {}, "method": "tools/list"}),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    let missing_protocol = raw_mcp_request_without_protocol(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "tools", "method": "tools/list"}),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    let ping = call_mcp_with_session(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "ping", "method": "ping"}),
        &session_id,
    )
    .await;

    assert_eq!(missing_session.0, StatusCode::BAD_REQUEST);
    assert_eq!(unknown_session.0, StatusCode::NOT_FOUND);
    assert_eq!(invalid_initialize_id.1["id"], Value::Null);
    assert_eq!(invalid_initialize_id.1["error"]["code"], -32600);
    assert_eq!(invalid_initialize.1["error"]["code"], -32602);
    assert_eq!(initialize_status, StatusCode::OK);
    assert_eq!(before_initialized.1["error"]["code"], -32002);
    assert_eq!(initialized.0, StatusCode::ACCEPTED);
    assert_eq!(invalid_request_id.1["id"], Value::Null);
    assert_eq!(invalid_request_id.1["error"]["code"], -32600);
    assert_eq!(missing_protocol.0, StatusCode::BAD_REQUEST);
    assert_eq!(ping["result"], json!({}));
}

#[tokio::test]
async fn retrieve_context_returns_canonical_structured_content() {
    let (server, service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![IngestEvidence {
                    id: Some("ev-mcp".to_owned()),
                    content: "MCP Streamable HTTP retrieves graph context".to_owned(),
                    entity_labels: vec!["MCP".to_owned()],
                }],
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");
    let mut router = server.router();

    let response = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "call-1",
            "method": "tools/call",
            "params": {
                "name": "relay.retrieve_context",
                "arguments": {
                    "query": "Streamable HTTP",
                    "source_scope": "docs",
                    "limit": 2,
                    "freshness": "wait-until-fresh"
                }
            }
        }),
    )
    .await;

    let structured = &response["result"]["structuredContent"];

    assert_eq!(response["result"]["isError"], false);
    assert_eq!(structured["metadata"]["graph_version"], 1);
    assert_eq!(structured["source_scope"], "docs");
    assert_eq!(structured["freshness"], "wait-until-fresh");
    assert_eq!(structured["retrieval_mode"], "hybrid");
    assert_eq!(structured["results"][0]["evidence_id"], "ev-mcp");
    assert_eq!(structured["runtime_identity"]["protocol"], "mcp");
}

#[tokio::test]
async fn retrieve_context_rejects_scope_and_limit_before_service_call() {
    let server = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ("RELAY_KNOWLEDGE_MCP_MAX_LIMIT", "1"),
    ])
    .await;
    let mut router = server.router();

    let missing_scope = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "missing-scope",
            "method": "tools/call",
            "params": {
                "name": "relay.retrieve_context",
                "arguments": {"query": "anything"}
            }
        }),
    )
    .await;
    let limit = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "limit",
            "method": "tools/call",
            "params": {
                "name": "relay.retrieve_context",
                "arguments": {"query": "anything", "source_scope": "docs", "limit": 2}
            }
        }),
    )
    .await;

    assert_eq!(
        missing_scope["result"]["structuredContent"]["error_kind"],
        "invalid_scope"
    );
    assert_eq!(
        limit["result"]["structuredContent"]["error_kind"],
        "limit_exceeded"
    );
}

#[tokio::test]
async fn rejects_disallowed_origin_and_protocol_version() {
    let server = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut router = server.router();

    let bad_origin = raw_mcp_request(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "x", "method": "initialize", "params": initialize_params()}),
        [("origin", "https://attacker.example")],
    )
    .await;
    let bad_version = raw_mcp_request(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "x", "method": "initialize", "params": initialize_params()}),
        [("mcp-protocol-version", "2024-11-05")],
    )
    .await;
    let ipv6_loopback = raw_mcp_request(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "ipv6", "method": "initialize", "params": initialize_params()}),
        [("origin", "http://[::1]")],
    )
    .await;

    assert_eq!(bad_origin.0, StatusCode::FORBIDDEN);
    assert_eq!(bad_version.0, StatusCode::BAD_REQUEST);
    assert_eq!(ipv6_loopback.0, StatusCode::OK);
}

#[tokio::test]
async fn qos_rejection_returns_tool_error_and_releases_permit() {
    let server = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "1"),
    ])
    .await;
    let policy = server.network.current().qos;
    let session_id = {
        let mut setup_router = server.clone().router();
        initialize_session(&mut setup_router).await
    };
    let _occupied = server
        .qos
        .admit_request(&policy)
        .expect("test should occupy budget");
    let mut router = server.clone().router();

    let rejected = call_mcp_with_session(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "qos",
            "method": "tools/call",
            "params": {
                "name": "relay.health",
                "arguments": {}
            }
        }),
        &session_id,
    )
    .await;

    assert_eq!(
        rejected["result"]["structuredContent"]["error_kind"],
        "qos_rejected"
    );
    assert_eq!(server.qos_snapshot().in_flight_requests, 1);
}

#[tokio::test]
async fn notifications_and_protocol_errors_use_http_contract() {
    let server = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut router = server.router();
    let session_id = initialize_session(&mut router).await;

    let missing_session = raw_mcp_request(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": "call-1"}
        }),
        [],
    )
    .await;
    let cancelled = raw_mcp_request(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": "call-1"}
        }),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    let duplicate_initialized = raw_mcp_request(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    let notification_with_id = raw_mcp_request(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "bad-notification",
            "method": "notifications/initialized",
            "params": {}
        }),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    let response_message = raw_mcp_request(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "server-request",
            "result": {"ack": true}
        }),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    let invalid_response_message = raw_mcp_request(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": null,
            "result": {"ack": true}
        }),
        [],
    )
    .await;
    let malformed = raw_custom_request(&mut router, "POST", "/mcp", "not-json", []).await;
    let invalid_id = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": {"nested": true},
            "method": "tools/list"
        }),
    )
    .await;
    let fractional_id = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": 1.5,
            "method": "tools/list"
        }),
    )
    .await;
    let malformed_tool_call = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "bad-tool-call",
            "method": "tools/call",
            "params": {"arguments": {}}
        }),
    )
    .await;
    let unknown = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "unknown",
            "method": "tools/call",
            "params": {"name": "relay.missing", "arguments": {}}
        }),
    )
    .await;

    assert_eq!(missing_session.0, StatusCode::BAD_REQUEST);
    assert_eq!(cancelled.0, StatusCode::ACCEPTED);
    assert_eq!(duplicate_initialized.0, StatusCode::ACCEPTED);
    assert_eq!(notification_with_id.1["error"]["code"], -32600);
    assert_eq!(response_message.0, StatusCode::ACCEPTED);
    assert_eq!(invalid_response_message.0, StatusCode::BAD_REQUEST);
    assert_eq!(malformed.1["error"]["code"], -32700);
    assert_eq!(invalid_id["error"]["code"], -32600);
    assert_eq!(fractional_id["error"]["code"], -32600);
    assert_eq!(malformed_tool_call["error"]["code"], -32602);
    assert_eq!(unknown["error"]["code"], -32602);
}

#[tokio::test]
async fn http_headers_and_body_budget_are_enforced() {
    let server = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ("RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES", "32"),
    ])
    .await;
    let mut router = server.router();

    let bad_content_type = raw_custom_request(
        &mut router,
        "POST",
        "/mcp",
        "{}",
        [("content-type", "text/plain")],
    )
    .await;
    let bad_accept = raw_custom_request(
        &mut router,
        "POST",
        "/mcp",
        "{}",
        [("accept", "text/plain")],
    )
    .await;
    let partial_accept = raw_custom_request(
        &mut router,
        "POST",
        "/mcp",
        "{}",
        [("accept", "application/json")],
    )
    .await;
    let zero_quality_accept = raw_custom_request(
        &mut router,
        "POST",
        "/mcp",
        "{}",
        [("accept", "application/json;q=1, text/event-stream;q=0")],
    )
    .await;
    let wildcard_refused_sse = raw_custom_request(
        &mut router,
        "POST",
        "/mcp",
        "{}",
        [(
            "accept",
            "*/*;q=1, text/event-stream;q=0, application/json;q=1",
        )],
    )
    .await;
    let uppercase_media_types = raw_custom_request(
        &mut router,
        "POST",
        "/mcp",
        "{}",
        [
            ("content-type", "Application/JSON"),
            ("accept", "Application/Json, Text/Event-Stream"),
        ],
    )
    .await;
    let missing_accept =
        raw_custom_request_with_defaults(&mut router, "POST", "/mcp", "{}", [], true, false).await;
    let too_large = raw_custom_request(
        &mut router,
        "POST",
        "/mcp",
        r#"{"jsonrpc":"2.0","id":"large","method":"tools/list","params":{"padding":"xxxxxxxx"}}"#,
        [],
    )
    .await;
    let get = raw_custom_request(&mut router, "GET", "/mcp", "", []).await;

    assert_eq!(bad_content_type.0, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(bad_accept.0, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(partial_accept.0, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(zero_quality_accept.0, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(wildcard_refused_sse.0, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(uppercase_media_types.0, StatusCode::OK);
    assert_eq!(missing_accept.0, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(too_large.0, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(get.0, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn tool_timeout_returns_json_rpc_tool_error() {
    let store = Arc::new(SlowSearchStore);
    let (server, _service) = server_and_service_with_store(
        [
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
            ("RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS", "10"),
        ],
        store,
    )
    .await;
    let mut router = server.router();

    let response = tool_call(
        &mut router,
        "slow-search",
        "relay.retrieve_context",
        json!({"query": "slow", "source_scope": "docs"}),
    )
    .await;

    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error_kind"],
        "timeout"
    );
}

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

#[test]
fn cancellation_registration_releases_entries_on_drop_only_for_own_token() {
    let registry = CancellationRegistry::default();
    let (_first_receiver, first_registration) = registry.register("string:call".to_owned());
    let (_second_receiver, _second_registration) = registry.register("string:call".to_owned());

    drop(first_registration);

    assert_eq!(registry.active_len(), 1);
}

#[tokio::test]
async fn serve_until_shutdown_rejects_disabled_or_remote_bind_before_listening() {
    let disabled = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut disabled_agent = disabled.agent.clone();
    disabled_agent.mcp_streamable_http_enabled = false;
    let disabled_server = McpServer::new(
        disabled.service.clone(),
        disabled.network.clone(),
        disabled_agent,
    );

    let remote = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ("RELAY_KNOWLEDGE_HTTP_BIND", "0.0.0.0:8791"),
    ])
    .await;
    let alternate_loopback = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ("RELAY_KNOWLEDGE_HTTP_BIND", "127.0.0.2:8791"),
    ])
    .await;

    let disabled_error = disabled_server
        .serve_until_shutdown(async {})
        .await
        .expect_err("disabled server should fail");
    let remote_error = remote
        .serve_until_shutdown(async {})
        .await
        .expect_err("remote bind should fail before listen");
    let loopback_allowed = ensure_remote_bind_allowed(
        &alternate_loopback.network.current().http,
        &alternate_loopback.agent.access_policy,
    );

    assert!(matches!(disabled_error, McpServeError::Disabled));
    assert!(matches!(remote_error, McpServeError::RemoteBindDisabled));
    assert!(loopback_allowed.is_ok());
}

async fn server_with_env<const N: usize>(pairs: [(&str, &str); N]) -> McpServer {
    server_and_service(pairs).await.0
}

async fn server_and_service<const N: usize>(
    pairs: [(&str, &str); N],
) -> (McpServer, RelayKnowledgeService) {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    server_and_service_with_store(pairs, store).await
}

async fn server_and_service_with_store<const N: usize>(
    pairs: [(&str, &str); N],
    store: Arc<dyn crate::storage::KnowledgeStore>,
) -> (McpServer, RelayKnowledgeService) {
    let mut base = vec![
        ("HOME", "/home/alice"),
        ("TMPDIR", "/tmp"),
        ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ("RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED", "true"),
    ];
    base.extend(pairs);
    let environment =
        EnvironmentConfig::from_pairs(PlatformKind::Unix, base).expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let service = RelayKnowledgeService::with_store(runtime.clone(), store);
    let server = McpServer::new(
        service.clone(),
        runtime.network.clone(),
        runtime.agent.clone(),
    );

    (server, service)
}

async fn call_mcp(router: &mut Router, payload: Value) -> Value {
    if payload.get("method").and_then(Value::as_str) == Some("initialize") {
        let (status, value) = raw_mcp_request(router, payload, []).await;
        assert_eq!(status, StatusCode::OK);
        return value;
    }

    let session_id = initialize_session(router).await;
    call_mcp_with_session(router, payload, &session_id).await
}

async fn call_mcp_with_session(router: &mut Router, payload: Value, session_id: &str) -> Value {
    let (status, value) =
        raw_mcp_request(router, payload, [(MCP_SESSION_ID_HEADER, session_id)]).await;
    assert_eq!(status, StatusCode::OK);
    value
}

async fn tool_call(router: &mut Router, id: &str, name: &str, arguments: Value) -> Value {
    call_mcp(
        router,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments
            }
        }),
    )
    .await
}

async fn raw_mcp_request<const N: usize>(
    router: &mut Router,
    payload: Value,
    headers: [(&str, &str); N],
) -> (StatusCode, Value) {
    raw_custom_request(router, "POST", "/mcp", &payload.to_string(), headers).await
}

async fn raw_mcp_request_without_protocol<const N: usize>(
    router: &mut Router,
    payload: Value,
    headers: [(&str, &str); N],
) -> (StatusCode, Value) {
    let (status, _headers, value) = raw_custom_response_with_protocol_default(
        router,
        "POST",
        "/mcp",
        &payload.to_string(),
        headers,
        HeaderDefaults {
            content_type: true,
            accept: true,
            protocol_version: false,
        },
    )
    .await;
    (status, value)
}

async fn initialize_session(router: &mut Router) -> String {
    let (status, headers, response) = raw_mcp_response(
        router,
        json!({
            "jsonrpc": "2.0",
            "id": "init",
            "method": "initialize",
            "params": initialize_params()
        }),
        [],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    let session_id = headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("initialize should issue a session")
        .to_owned();
    let initialized = raw_mcp_request(
        router,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    assert_eq!(initialized.0, StatusCode::ACCEPTED);
    session_id
}

fn initialize_params() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {
            "name": "relay-knowledge-test",
            "version": "0.1.0"
        }
    })
}

async fn raw_mcp_response<const N: usize>(
    router: &mut Router,
    payload: Value,
    headers: [(&str, &str); N],
) -> (StatusCode, HeaderMap, Value) {
    raw_custom_response(router, "POST", "/mcp", &payload.to_string(), headers).await
}

async fn raw_custom_request<const N: usize>(
    router: &mut Router,
    method: &str,
    uri: &str,
    body: &str,
    headers: [(&str, &str); N],
) -> (StatusCode, Value) {
    raw_custom_request_with_defaults(router, method, uri, body, headers, true, true).await
}

async fn raw_custom_response<const N: usize>(
    router: &mut Router,
    method: &str,
    uri: &str,
    body: &str,
    headers: [(&str, &str); N],
) -> (StatusCode, HeaderMap, Value) {
    raw_custom_response_with_defaults(router, method, uri, body, headers, true, true).await
}

async fn raw_custom_request_with_defaults<const N: usize>(
    router: &mut Router,
    method: &str,
    uri: &str,
    body: &str,
    headers: [(&str, &str); N],
    default_content_type: bool,
    default_accept: bool,
) -> (StatusCode, Value) {
    let (status, _headers, value) = raw_custom_response_with_protocol_default(
        router,
        method,
        uri,
        body,
        headers,
        HeaderDefaults {
            content_type: default_content_type,
            accept: default_accept,
            protocol_version: true,
        },
    )
    .await;
    (status, value)
}

async fn raw_custom_response_with_defaults<const N: usize>(
    router: &mut Router,
    method: &str,
    uri: &str,
    body: &str,
    headers: [(&str, &str); N],
    default_content_type: bool,
    default_accept: bool,
) -> (StatusCode, HeaderMap, Value) {
    raw_custom_response_with_protocol_default(
        router,
        method,
        uri,
        body,
        headers,
        HeaderDefaults {
            content_type: default_content_type,
            accept: default_accept,
            protocol_version: true,
        },
    )
    .await
}

#[derive(Clone, Copy)]
struct HeaderDefaults {
    content_type: bool,
    accept: bool,
    protocol_version: bool,
}

async fn raw_custom_response_with_protocol_default<const N: usize>(
    router: &mut Router,
    method: &str,
    uri: &str,
    body: &str,
    headers: [(&str, &str); N],
    defaults: HeaderDefaults,
) -> (StatusCode, HeaderMap, Value) {
    let has_content_type = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"));
    let has_accept = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("accept"));
    let has_protocol_version = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(MCP_PROTOCOL_VERSION_HEADER));
    let mut builder = Request::builder().method(method).uri(uri);
    if defaults.content_type && !has_content_type {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if defaults.accept && !has_accept {
        builder = builder.header(header::ACCEPT, "application/json, text/event-stream");
    }
    if defaults.protocol_version && !has_protocol_version {
        builder = builder.header(MCP_PROTOCOL_VERSION_HEADER, MCP_PROTOCOL_VERSION);
    }
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    let response = router
        .clone()
        .oneshot(
            builder
                .body(Body::from(body.to_owned()))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or(Value::Null)
    };

    (status, headers, value)
}

fn tool_names(response: &Value) -> Vec<String> {
    response["result"]["tools"]
        .as_array()
        .expect("tools should be an array")
        .iter()
        .map(|tool| {
            tool["name"]
                .as_str()
                .expect("tool should have a name")
                .to_owned()
        })
        .collect()
}
