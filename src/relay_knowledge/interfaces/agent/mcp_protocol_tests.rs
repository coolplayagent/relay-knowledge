use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header},
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::mcp_test_support::SlowSearchStore;
use super::mcp_tests::{
    call_mcp, call_mcp_with_session, initialize_params, initialize_session, raw_custom_response,
    raw_mcp_request, raw_mcp_request_without_protocol, raw_mcp_response,
    server_and_service_with_store, server_with_env, tool_names,
};
use super::*;

#[tokio::test]
async fn initialize_and_tools_list_use_snake_case_names_without_refresh() {
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
    let names = tool_names(&tools);
    assert!(names.contains(&"relay_retrieve_context".to_owned()));
    assert!(names.contains(&"relay_code_query".to_owned()));
    assert!(names.contains(&"relay_software_query".to_owned()));
    assert!(names.contains(&"relay_code_impact".to_owned()));
    assert!(!names.contains(&"relay_refresh_indexes".to_owned()));
    assert!(
        names.iter().all(|name| name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')),
        "MCP tool names must avoid special characters"
    );
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
                "name": "relay_retrieve_context_prompt",
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
    assert!(prompt_names.contains(&"relay_retrieve_context_prompt"));
    assert!(
        prompt["result"]["messages"][0]["content"]["text"]
            .as_str()
            .expect("prompt text")
            .contains("relay_retrieve_context")
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
                "name": "relay_code_impact_prompt",
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
                "name": "relay_retrieve_context_prompt",
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
    assert!(metrics_text.contains("relay_knowledge_mcp_cold_start_total"));
    assert!(prompt_text.contains("relay_code_impact"));
    assert_eq!(unknown_resource["error"]["code"], -32602);
    assert_eq!(invalid_resource_params["error"]["code"], -32602);
    assert_eq!(missing_prompt_argument["error"]["code"], -32602);
    assert_eq!(unknown_prompt["error"]["code"], -32602);
}

#[tokio::test]
async fn metrics_exporter_shares_mcp_router_without_compat_routes() {
    let server = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut router = server.router();

    let removed_sse = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp/sse")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    let removed_message = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/message?sessionId=removed")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
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

    assert_eq!(removed_sse.status(), StatusCode::NOT_FOUND);
    assert_eq!(removed_message.status(), StatusCode::NOT_FOUND);
    assert_eq!(forbidden_metrics.0, StatusCode::FORBIDDEN);
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
async fn headerless_cancellation_notifications_cancel_occupied_mcp_qos_slot() {
    let (server, _service) = server_and_service_with_store(
        [
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
            ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "1"),
        ],
        Arc::new(SlowSearchStore),
    )
    .await;
    let router = server.clone().router();
    let session_id = {
        let mut setup_router = router.clone();
        initialize_session(&mut setup_router).await
    };
    let mut slow_router = router.clone();
    let slow_session_id = session_id.clone();
    let slow_request = tokio::spawn(async move {
        raw_mcp_request(
            &mut slow_router,
            json!({
                "jsonrpc": "2.0",
                "id": "slow",
                "method": "tools/call",
                "params": {
                    "name": "relay_retrieve_context",
                    "arguments": {
                        "query": "slow",
                        "source_scope": "docs"
                    }
                }
            }),
            [(MCP_SESSION_ID_HEADER, slow_session_id.as_str())],
        )
        .await
    });

    wait_for_web_in_flight(&server, 1).await;
    let mut cancel_router = router.clone();
    let cancelled = raw_mcp_request_at_endpoint(
        &mut cancel_router,
        "/mcp",
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {
                "requestId": "slow",
                "reason": "test"
            }
        }),
        Some(session_id.as_str()),
    )
    .await;
    let slow_response = slow_request.await.expect("slow request should finish");

    assert_eq!(cancelled.0, StatusCode::ACCEPTED);
    assert_eq!(slow_response.0, StatusCode::OK);
    assert_eq!(server.qos.diagnostics_snapshot().cancelled_total, 1);
    assert_eq!(server.qos.diagnostics_snapshot().rejected_total, 0);
}

#[tokio::test]
async fn late_cancellation_notifications_do_not_increment_cancelled_total() {
    let server = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut router = server.clone().router();
    let session_id = initialize_session(&mut router).await;

    let cancelled = raw_mcp_request(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {
                "requestId": "missing",
                "reason": "late"
            }
        }),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;

    assert_eq!(cancelled.0, StatusCode::ACCEPTED);
    assert_eq!(server.qos.diagnostics_snapshot().cancelled_total, 0);
}

#[tokio::test]
async fn cancellation_priority_requires_initialized_session() {
    let (server, _service) = server_and_service_with_store(
        [
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
            ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "1"),
        ],
        Arc::new(SlowSearchStore),
    )
    .await;
    let router = server.clone().router();
    let session_id = {
        let mut setup_router = router.clone();
        initialize_session(&mut setup_router).await
    };
    let mut slow_router = router.clone();
    let slow_session_id = session_id.clone();
    let slow_request = tokio::spawn(async move {
        raw_mcp_request(
            &mut slow_router,
            json!({
                "jsonrpc": "2.0",
                "id": "slow",
                "method": "tools/call",
                "params": {
                    "name": "relay_retrieve_context",
                    "arguments": {
                        "query": "slow",
                        "source_scope": "docs"
                    }
                }
            }),
            [(MCP_SESSION_ID_HEADER, slow_session_id.as_str())],
        )
        .await
    });

    wait_for_web_in_flight(&server, 1).await;
    let mut cancel_router = router.clone();
    let rejected = raw_mcp_request_at_endpoint(
        &mut cancel_router,
        "/mcp",
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {
                "requestId": "slow",
                "reason": "test"
            }
        }),
        Some("rk-unknown"),
    )
    .await;
    let mut cancel_router = router.clone();
    let _ = raw_mcp_request_at_endpoint(
        &mut cancel_router,
        "/mcp",
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": "slow", "reason": "test"}
        }),
        Some(session_id.as_str()),
    )
    .await;
    let slow_response = slow_request.await.expect("slow request should finish");

    assert_eq!(rejected.0, StatusCode::NOT_FOUND);
    assert_eq!(slow_response.0, StatusCode::OK);
    assert_eq!(server.qos.diagnostics_snapshot().cancelled_total, 1);
    assert_eq!(server.qos.diagnostics_snapshot().rejected_total, 0);
}

#[tokio::test]
async fn service_style_mcp_qos_rejection_stays_json_rpc() {
    let (server, service) = server_and_service_with_store(
        [
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
            ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "1"),
        ],
        Arc::new(SlowSearchStore),
    )
    .await;
    let network = server.network.current();
    let web_router =
        crate::interfaces::web::router(service.clone(), network.http.max_request_body_bytes);
    let router = crate::net::http::router_with_qos_request_admission(
        web_router,
        server.qos.clone(),
        network.qos,
    )
    .merge(server.clone().router());
    let session_id = {
        let mut setup_router = router.clone();
        initialize_session(&mut setup_router).await
    };
    let mut slow_router = router.clone();
    let slow_session_id = session_id.clone();
    let slow_request = tokio::spawn(async move {
        raw_mcp_request(
            &mut slow_router,
            json!({
                "jsonrpc": "2.0",
                "id": "slow",
                "method": "tools/call",
                "params": {
                    "name": "relay_retrieve_context",
                    "arguments": {"query": "slow", "source_scope": "docs"}
                }
            }),
            [(MCP_SESSION_ID_HEADER, slow_session_id.as_str())],
        )
        .await
    });

    wait_for_web_in_flight(&server, 1).await;
    let mut rejected_router = router.clone();
    let rejected = raw_mcp_request(
        &mut rejected_router,
        json!({
            "jsonrpc": "2.0",
            "id": "qos",
            "method": "tools/call",
            "params": {"name": "relay_health", "arguments": {}}
        }),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    let mut cancel_router = router.clone();
    let _ = raw_mcp_request_at_endpoint(
        &mut cancel_router,
        "/mcp",
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": "slow", "reason": "test"}
        }),
        Some(session_id.as_str()),
    )
    .await;
    let _ = slow_request.await.expect("slow request should finish");

    assert_eq!(rejected.0, StatusCode::OK);
    assert_eq!(rejected.1["result"]["isError"], true);
    assert_eq!(
        rejected.1["result"]["structuredContent"]["error_kind"],
        "qos_rejected"
    );
}

#[tokio::test]
async fn cancellation_priority_uses_configured_mcp_endpoint() {
    let (server, _service) = server_and_service_with_store(
        [
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
            ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "1"),
            ("RELAY_KNOWLEDGE_MCP_ENDPOINT", "/relay-mcp"),
        ],
        Arc::new(SlowSearchStore),
    )
    .await;
    let router = server.clone().router();
    let session_id = {
        let mut setup_router = router.clone();
        initialize_session_at_endpoint(&mut setup_router, "/relay-mcp").await
    };
    let mut slow_router = router.clone();
    let slow_session_id = session_id.clone();
    let slow_request = tokio::spawn(async move {
        raw_mcp_request_at_endpoint(
            &mut slow_router,
            "/relay-mcp",
            json!({
                "jsonrpc": "2.0",
                "id": "slow",
                "method": "tools/call",
                "params": {
                    "name": "relay_retrieve_context",
                    "arguments": {
                        "query": "slow",
                        "source_scope": "docs"
                    }
                }
            }),
            Some(slow_session_id.as_str()),
        )
        .await
    });

    wait_for_web_in_flight(&server, 1).await;
    let mut cancel_router = router.clone();
    let cancelled = raw_mcp_request_at_endpoint(
        &mut cancel_router,
        "/relay-mcp",
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {
                "requestId": "slow",
                "reason": "test"
            }
        }),
        Some(session_id.as_str()),
    )
    .await;
    let slow_response = slow_request.await.expect("slow request should finish");

    assert_eq!(cancelled.0, StatusCode::ACCEPTED);
    assert_eq!(slow_response.0, StatusCode::OK);
    assert_eq!(server.qos.diagnostics_snapshot().cancelled_total, 1);
    assert_eq!(server.qos.diagnostics_snapshot().rejected_total, 0);
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

    assert_eq!(missing_origin_metrics.0, StatusCode::FORBIDDEN);
    assert_eq!(allowed_origin_metrics.0, StatusCode::OK);
}

async fn wait_for_web_in_flight(server: &McpServer, expected: usize) {
    for _ in 0..50 {
        if server.qos_snapshot().in_flight_requests == expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    panic!("Web QoS in-flight count did not reach {expected}");
}

async fn initialize_session_at_endpoint(router: &mut Router, endpoint: &str) -> String {
    let (status, headers, response) = raw_mcp_response_at_endpoint(
        router,
        endpoint,
        json!({
            "jsonrpc": "2.0",
            "id": "init",
            "method": "initialize",
            "params": initialize_params()
        }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    let session_id = headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("initialize should issue a session")
        .to_owned();
    let initialized = raw_mcp_request_at_endpoint(
        router,
        endpoint,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
        Some(session_id.as_str()),
    )
    .await;
    assert_eq!(initialized.0, StatusCode::ACCEPTED);
    session_id
}

async fn raw_mcp_request_at_endpoint(
    router: &mut Router,
    endpoint: &str,
    payload: Value,
    session_id: Option<&str>,
) -> (StatusCode, Value) {
    let (status, _headers, value) =
        raw_mcp_response_at_endpoint(router, endpoint, payload, session_id).await;
    (status, value)
}

async fn raw_mcp_response_at_endpoint(
    router: &mut Router,
    endpoint: &str,
    payload: Value,
    session_id: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let body = payload.to_string();
    let mut builder = Request::builder()
        .method("POST")
        .uri(endpoint)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(MCP_PROTOCOL_VERSION_HEADER, MCP_PROTOCOL_VERSION);
    if let Some(session_id) = session_id {
        builder = builder.header(MCP_SESSION_ID_HEADER, session_id);
    }
    let response = router
        .clone()
        .oneshot(
            builder
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    if body.is_empty() {
        return (status, headers, Value::Null);
    }

    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, headers, value)
}
