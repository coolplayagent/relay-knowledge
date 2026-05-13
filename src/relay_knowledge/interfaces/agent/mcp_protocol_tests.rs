use std::{fmt::Debug, sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    http::{Request, StatusCode, header},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use super::{
    mcp_test_support::SlowSearchStore,
    mcp_tests::{
        call_mcp, call_mcp_with_session, initialize_params, raw_custom_response, raw_mcp_request,
        raw_mcp_request_without_protocol, raw_mcp_response, server_and_service_with_store,
        server_with_env, tool_names,
    },
};

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
    assert_eq!(
        initialize["result"]["capabilities"]["resources"]["listChanged"],
        false
    );
    assert_eq!(
        initialize["result"]["capabilities"]["prompts"]["listChanged"],
        false
    );
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
async fn resources_and_prompts_expose_agent_context_surfaces() {
    let server = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut router = server.router();

    let resources = call_mcp(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "resources", "method": "resources/list"}),
    )
    .await;
    let health = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "health-resource",
            "method": "resources/read",
            "params": {"uri": "relay://service/health"}
        }),
    )
    .await;
    let prompts = call_mcp(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "prompts", "method": "prompts/list"}),
    )
    .await;
    let prompt = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "prompt",
            "method": "prompts/get",
            "params": {
                "name": "relay.retrieve-context",
                "arguments": {
                    "query": "index freshness",
                    "source_scope": "docs"
                }
            }
        }),
    )
    .await;

    let resource_uris = resources["result"]["resources"]
        .as_array()
        .expect("resources")
        .iter()
        .map(|resource| resource["uri"].as_str().expect("uri"))
        .collect::<Vec<_>>();
    let health_text = health["result"]["contents"][0]["text"]
        .as_str()
        .expect("health text");
    let health_value: Value = serde_json::from_str(health_text).expect("health json");
    let graph_summary_denied = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "graph-summary-denied",
            "method": "resources/read",
            "params": {"uri": "relay://graph/summary"}
        }),
    )
    .await;
    let prompt_names = prompts["result"]["prompts"]
        .as_array()
        .expect("prompts")
        .iter()
        .map(|prompt| prompt["name"].as_str().expect("prompt name"))
        .collect::<Vec<_>>();

    assert!(resource_uris.contains(&"relay://service/health"));
    assert!(!resource_uris.contains(&"relay://graph/summary"));
    assert_eq!(graph_summary_denied["error"]["code"], -32000);
    assert!(
        graph_summary_denied["error"]["message"]
            .as_str()
            .expect("graph summary error")
            .contains("allow_unspecified_scope")
    );
    assert_eq!(health_value["healthy"], true);
    assert!(prompt_names.contains(&"relay.retrieve-context"));
    assert!(
        prompt["result"]["messages"][0]["content"]["text"]
            .as_str()
            .expect("prompt text")
            .contains("relay.retrieve_context")
    );
}

#[tokio::test]
async fn graph_summary_resource_requires_unspecified_scope_policy() {
    let server = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ("RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE", "true"),
    ])
    .await;
    let mut router = server.router();

    let resources = call_mcp(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "resources", "method": "resources/list"}),
    )
    .await;
    let graph_summary = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "graph-summary",
            "method": "resources/read",
            "params": {"uri": "relay://graph/summary"}
        }),
    )
    .await;
    let scoped_summary = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "scoped-graph-summary",
            "method": "resources/read",
            "params": {
                "uri": "relay://graph/summary",
                "source_scope": "docs"
            }
        }),
    )
    .await;

    let resource_uris = resources["result"]["resources"]
        .as_array()
        .expect("resources")
        .iter()
        .map(|resource| resource["uri"].as_str().expect("uri"))
        .collect::<Vec<_>>();
    let graph_summary_text = graph_summary["result"]["contents"][0]["text"]
        .as_str()
        .expect("graph summary text");
    let graph_summary_value: Value =
        serde_json::from_str(graph_summary_text).expect("graph summary json");

    assert!(resource_uris.contains(&"relay://graph/summary"));
    assert_eq!(graph_summary_value["graph"]["graph_version"], 0);
    assert_eq!(scoped_summary["error"]["code"], -32602);
    assert!(
        scoped_summary["error"]["message"]
            .as_str()
            .expect("scoped graph summary error")
            .contains("does not accept source_scope")
    );
}

#[tokio::test]
async fn resources_and_prompts_cover_all_readonly_variants_and_errors() {
    let server = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut router = server.router();

    let service_status = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "service-status",
            "method": "resources/read",
            "params": {"uri": "relay://service/status"}
        }),
    )
    .await;
    let index_status = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "index-status",
            "method": "resources/read",
            "params": {"uri": "relay://indexes/status"}
        }),
    )
    .await;
    let metrics = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "metrics-resource",
            "method": "resources/read",
            "params": {"uri": "relay://metrics/prometheus"}
        }),
    )
    .await;
    let unknown_resource = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "unknown-resource",
            "method": "resources/read",
            "params": {"uri": "relay://unknown"}
        }),
    )
    .await;
    let invalid_resource_params = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "invalid-resource-params",
            "method": "resources/read",
            "params": {}
        }),
    )
    .await;
    let code_impact_prompt = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "code-impact-prompt",
            "method": "prompts/get",
            "params": {
                "name": "relay.code-impact",
                "arguments": {
                    "repository": "repo",
                    "base_ref": "main",
                    "head_ref": "feature"
                }
            }
        }),
    )
    .await;
    let missing_prompt_argument = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "missing-prompt-argument",
            "method": "prompts/get",
            "params": {
                "name": "relay.retrieve-context",
                "arguments": {"query": "   "}
            }
        }),
    )
    .await;
    let unknown_prompt = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "unknown-prompt",
            "method": "prompts/get",
            "params": {"name": "relay.unknown", "arguments": {}}
        }),
    )
    .await;

    let service_text = service_status["result"]["contents"][0]["text"]
        .as_str()
        .expect("service status text");
    let service_value: Value = serde_json::from_str(service_text).expect("service status json");
    let index_text = index_status["result"]["contents"][0]["text"]
        .as_str()
        .expect("index status text");
    let index_value: Value = serde_json::from_str(index_text).expect("index status json");
    let metrics_text = metrics["result"]["contents"][0]["text"]
        .as_str()
        .expect("metrics text");
    let prompt_text = code_impact_prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .expect("code impact prompt text");

    assert_eq!(service_value["service_name"], "relay-knowledge");
    assert!(index_value["indexes"].as_array().expect("indexes").len() >= 3);
    assert!(metrics_text.contains("relay_knowledge_graph_version"));
    assert!(prompt_text.contains("relay.code_impact"));
    assert_eq!(unknown_resource["error"]["code"], -32602);
    assert_eq!(invalid_resource_params["error"]["code"], -32602);
    assert_eq!(missing_prompt_argument["error"]["code"], -32602);
    assert_eq!(unknown_prompt["error"]["code"], -32602);
}

#[tokio::test]
async fn legacy_sse_endpoint_and_metrics_exporter_share_mcp_router() {
    let server = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut router = server.router();

    let forbidden_sse = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp/sse")
                .header(header::ORIGIN, "https://attacker.example")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    let sse_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp/sse")
                .header(header::ACCEPT, "text/event-stream")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    let sse_status = sse_response.status();
    let sse_headers = sse_response.headers().clone();
    let session_id = sse_headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("legacy sse should issue session")
        .to_owned();
    let mut sse_stream = sse_response.into_body().into_data_stream();
    let endpoint_event = next_sse_event(&mut sse_stream).await;
    let stream_closed = tokio::time::timeout(Duration::from_millis(20), sse_stream.next()).await;

    let initialize = raw_custom_response(
        &mut router,
        "POST",
        &format!("/mcp/message?sessionId={session_id}"),
        &json!({
            "jsonrpc": "2.0",
            "id": "legacy-init",
            "method": "initialize",
            "params": initialize_params()
        })
        .to_string(),
        [("accept", "application/json")],
    )
    .await;
    let initialize_event = next_sse_event(&mut sse_stream).await;
    let initialized = raw_custom_response(
        &mut router,
        "POST",
        &format!("/mcp/message?sessionId={session_id}"),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        [("accept", "application/json")],
    )
    .await;
    let legacy_ping = raw_custom_response(
        &mut router,
        "POST",
        &format!("/mcp/message?sessionId={session_id}"),
        r#"{"jsonrpc":"2.0","id":"legacy-ping","method":"ping"}"#,
        [("accept", "application/json")],
    )
    .await;
    let ping_event = next_sse_event(&mut sse_stream).await;
    let bad_protocol = raw_custom_response(
        &mut router,
        "POST",
        &format!("/mcp/message?sessionId={session_id}"),
        r#"{"jsonrpc":"2.0","id":"bad-protocol","method":"ping"}"#,
        [
            ("accept", "application/json"),
            (MCP_PROTOCOL_VERSION_HEADER, "2024-11-05"),
        ],
    )
    .await;
    let bad_protocol_event = next_sse_event(&mut sse_stream).await;
    let (metrics_status, metrics_headers, metrics_body) =
        raw_custom_response(&mut router, "GET", "/mcp/metrics", "", []).await;
    let forbidden_metrics = raw_custom_response(
        &mut router,
        "GET",
        "/mcp/metrics",
        "",
        [(header::ORIGIN.as_str(), "https://attacker.example")],
    )
    .await;

    assert_eq!(forbidden_sse.status(), StatusCode::FORBIDDEN);
    assert_eq!(forbidden_metrics.0, StatusCode::FORBIDDEN);
    assert_eq!(sse_status, StatusCode::OK);
    assert!(endpoint_event.contains("event: endpoint"));
    assert!(endpoint_event.contains(&format!("/mcp/message?sessionId={session_id}")));
    assert!(stream_closed.is_err());
    assert_eq!(initialize.0, StatusCode::ACCEPTED);
    assert_eq!(initialize.2, Value::Null);
    assert!(initialize_event.contains("event: message"));
    assert!(initialize_event.contains(r#""id":"legacy-init""#));
    assert!(initialize_event.contains(MCP_PROTOCOL_VERSION));
    assert_eq!(initialized.0, StatusCode::ACCEPTED);
    assert_eq!(legacy_ping.0, StatusCode::ACCEPTED);
    assert_eq!(legacy_ping.2, Value::Null);
    assert!(ping_event.contains("event: message"));
    assert!(ping_event.contains(r#""id":"legacy-ping""#));
    assert!(ping_event.contains(r#""result":{}"#));
    assert_eq!(bad_protocol.0, StatusCode::ACCEPTED);
    assert_eq!(bad_protocol.2, Value::Null);
    assert!(bad_protocol_event.contains("event: message"));
    assert!(!bad_protocol_event.contains("event: error"));
    assert!(bad_protocol_event.contains(r#""error""#));
    assert!(bad_protocol_event.contains(r#""http_status":400"#));
    assert_eq!(metrics_status, StatusCode::OK);
    assert_eq!(
        metrics_headers
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/plain; version=0.0.4")
    );
    assert_eq!(metrics_body, Value::Null);
}

#[tokio::test]
async fn mcp_get_endpoints_reject_missing_origin_when_allowlist_is_configured() {
    let server = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        (
            "RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS",
            "https://trusted.example",
        ),
    ])
    .await;
    let mut router = server.router();

    let missing_origin_sse = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp/sse")
                .header(header::ACCEPT, "text/event-stream")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    let missing_origin_metrics =
        raw_custom_response(&mut router, "GET", "/mcp/metrics", "", []).await;
    let allowed_origin_metrics = raw_custom_response(
        &mut router,
        "GET",
        "/mcp/metrics",
        "",
        [(header::ORIGIN.as_str(), "https://trusted.example")],
    )
    .await;

    assert_eq!(missing_origin_sse.status(), StatusCode::FORBIDDEN);
    assert_eq!(missing_origin_metrics.0, StatusCode::FORBIDDEN);
    assert_eq!(allowed_origin_metrics.0, StatusCode::OK);
}

#[tokio::test]
async fn legacy_message_rejects_before_tool_execution_when_sse_stream_is_closed() {
    let server = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut router = server.clone().router();
    let sse_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp/sse")
                .header(header::ACCEPT, "text/event-stream")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    let session_id = sse_response
        .headers()
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("legacy sse should issue session")
        .to_owned();
    let mut sse_stream = sse_response.into_body().into_data_stream();
    let _endpoint_event = next_sse_event(&mut sse_stream).await;

    let initialize = raw_custom_response(
        &mut router,
        "POST",
        &format!("/mcp/message?sessionId={session_id}"),
        &json!({
            "jsonrpc": "2.0",
            "id": "legacy-init",
            "method": "initialize",
            "params": initialize_params()
        })
        .to_string(),
        [("accept", "application/json")],
    )
    .await;
    let _initialize_event = next_sse_event(&mut sse_stream).await;
    let initialized = raw_custom_response(
        &mut router,
        "POST",
        &format!("/mcp/message?sessionId={session_id}"),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        [("accept", "application/json")],
    )
    .await;
    drop(sse_stream);

    let refresh_after_close = raw_custom_response(
        &mut router,
        "POST",
        &format!("/mcp/message?sessionId={session_id}"),
        &json!({
            "jsonrpc": "2.0",
            "id": "closed-refresh",
            "method": "tools/call",
            "params": {
                "name": "relay.refresh_indexes",
                "arguments": {"kinds": []}
            }
        })
        .to_string(),
        [("accept", "application/json")],
    )
    .await;

    assert_eq!(initialize.0, StatusCode::ACCEPTED);
    assert_eq!(initialized.0, StatusCode::ACCEPTED);
    assert_eq!(refresh_after_close.0, StatusCode::NOT_FOUND);
    assert!(
        !server
            .audit_snapshot()
            .iter()
            .any(|event| event.operation == "relay.refresh_indexes"),
        "legacy /message must reject unavailable SSE delivery before tool execution"
    );
}

#[tokio::test]
async fn legacy_message_acknowledges_before_slow_mcp_execution_completes() {
    let (server, _service) = server_and_service_with_store(
        [("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")],
        Arc::new(SlowSearchStore),
    )
    .await;
    let mut router = server.router();
    let sse_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp/sse")
                .header(header::ACCEPT, "text/event-stream")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    let session_id = sse_response
        .headers()
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("legacy sse should issue session")
        .to_owned();
    let mut sse_stream = sse_response.into_body().into_data_stream();
    let _endpoint_event = next_sse_event(&mut sse_stream).await;
    let initialize = raw_custom_response(
        &mut router,
        "POST",
        &format!("/mcp/message?sessionId={session_id}"),
        &json!({
            "jsonrpc": "2.0",
            "id": "legacy-init",
            "method": "initialize",
            "params": initialize_params()
        })
        .to_string(),
        [("accept", "application/json")],
    )
    .await;
    let _initialize_event = next_sse_event(&mut sse_stream).await;
    let initialized = raw_custom_response(
        &mut router,
        "POST",
        &format!("/mcp/message?sessionId={session_id}"),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        [("accept", "application/json")],
    )
    .await;

    let inspect_request = Request::builder()
        .method("POST")
        .uri(format!("/mcp/message?sessionId={session_id}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json")
        .header(MCP_PROTOCOL_VERSION_HEADER, MCP_PROTOCOL_VERSION)
        .body(Body::from(
            json!({
                "jsonrpc": "2.0",
                "id": "slow-inspect",
                "method": "tools/call",
                "params": {
                    "name": "relay.inspect_graph",
                    "arguments": {"source_scope": "docs"}
                }
            })
            .to_string(),
        ))
        .expect("request should build");
    let inspect_task = tokio::spawn(router.clone().oneshot(inspect_request));
    tokio::time::sleep(Duration::from_millis(20)).await;
    drop(sse_stream);
    let inspect_response = inspect_task
        .await
        .expect("request task should join")
        .expect("router should respond");

    assert_eq!(initialize.0, StatusCode::ACCEPTED);
    assert_eq!(initialized.0, StatusCode::ACCEPTED);
    assert_eq!(inspect_response.status(), StatusCode::ACCEPTED);
}

async fn next_sse_event<S, E>(stream: &mut S) -> String
where
    S: futures_util::Stream<Item = Result<Bytes, E>> + Unpin,
    E: Debug,
{
    let chunk = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("SSE event should arrive")
        .expect("SSE stream should stay open")
        .expect("SSE chunk should be readable");

    String::from_utf8(chunk.to_vec()).expect("SSE event should be UTF-8")
}
