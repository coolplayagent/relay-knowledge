use axum::http::StatusCode;
use serde_json::{Value, json};

use super::mcp_tests::{
    call_mcp, call_mcp_with_session, initialize_params, raw_mcp_request,
    raw_mcp_request_without_protocol, raw_mcp_response, server_with_env, tool_names,
};
use super::*;

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
