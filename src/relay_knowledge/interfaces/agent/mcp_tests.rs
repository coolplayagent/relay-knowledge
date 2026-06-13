use std::sync::Arc;

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header},
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::{
    api::{AuditQueryApiRequest, IngestEvidence, IngestRequest},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    env::{EnvironmentConfig, PlatformKind},
    interfaces::agent::AgentAuditStatus,
    storage::SqliteGraphStore,
};

use super::mcp_test_support::SlowSearchStore;

#[path = "mcp_tests/support.rs"]
mod support;
pub(super) use support::{
    call_mcp, call_mcp_with_session, initialize_params, initialize_session, raw_custom_request,
    raw_custom_request_with_defaults, raw_custom_response, raw_mcp_request,
    raw_mcp_request_without_protocol, raw_mcp_response, server_and_service,
    server_and_service_with_store, server_with_env, tool_call, tool_names,
};

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
                    source_path: None,
                    span: None,
                    confidence: None,
                    status: None,
                    content: "MCP Streamable HTTP retrieves graph context".to_owned(),
                    entity_labels: vec!["MCP".to_owned()],
                    extraction: None,
                }],
                relations: Vec::new(),
                claims: Vec::new(),
                events: Vec::new(),
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
                "name": "relay_retrieve_context",
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
                "name": "relay_retrieve_context",
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
                "name": "relay_retrieve_context",
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
async fn runtime_scope_verification_failure_denies_by_policy() {
    let store = Arc::new(SlowSearchStore);
    let (server, _service) = server_and_service_with_store([], store).await;
    let mut router = server.router();

    let response = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "unverified-runtime-scope",
            "method": "tools/call",
            "params": {
                "name": "relay_retrieve_context",
                "arguments": {
                    "query": "anything",
                    "source_scope": "repo",
                    "limit": 2
                }
            }
        }),
    )
    .await;

    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error_kind"],
        "permission_denied"
    );
    assert!(
        response["result"]["structuredContent"]["message"]
            .as_str()
            .expect("error message")
            .contains("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES=repo")
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
                "name": "relay_health",
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
async fn nested_web_mcp_requests_do_not_consume_queue_budget() {
    let (server, _service) = server_and_service_with_store(
        [
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
            ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "10"),
            ("RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH", "1"),
        ],
        Arc::new(SlowSearchStore),
    )
    .await;
    let policy = server.network.current().qos;
    let router = crate::net::http::router_with_qos_request_admission(
        server.clone().router(),
        server.qos.clone(),
        policy,
    );
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
    wait_for_mcp_in_flight(&server, 1).await;
    let mut second_router = router.clone();
    let response = call_mcp_with_session(
        &mut second_router,
        json!({
            "jsonrpc": "2.0",
            "id": "list-tools",
            "method": "tools/list"
        }),
        &session_id,
    )
    .await;
    let slow_response = slow_request.await.expect("slow request task should join");

    assert!(response["result"]["tools"].is_array());
    assert_eq!(slow_response.0, StatusCode::OK);
    assert_eq!(server.qos.diagnostics_snapshot().rejected_total, 0);
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
async fn resources_prompts_and_delete_session_are_supported() {
    let (server, service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![IngestEvidence {
                    id: Some("ev-resource".to_owned()),
                    source_path: Some("docs/guide.md".to_owned()),
                    span: None,
                    confidence: None,
                    status: None,
                    content: "MCP resources expose metadata".to_owned(),
                    entity_labels: Vec::new(),
                    extraction: None,
                }],
                relations: Vec::new(),
                claims: Vec::new(),
                events: Vec::new(),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");
    let mut router = server.router();
    let session_id = initialize_session(&mut router).await;

    let resources = call_mcp_with_session(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "resources", "method": "resources/list"}),
        &session_id,
    )
    .await;
    let service_status = call_mcp_with_session(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "service-status",
            "method": "resources/read",
            "params": {"uri": "relay://service/status"}
        }),
        &session_id,
    )
    .await;
    let index_status = call_mcp_with_session(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "index-status",
            "method": "resources/read",
            "params": {"uri": "relay://indexes/status"}
        }),
        &session_id,
    )
    .await;
    let prompts = call_mcp_with_session(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "prompts", "method": "prompts/list"}),
        &session_id,
    )
    .await;
    let prompt = call_mcp_with_session(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "prompt",
            "method": "prompts/get",
            "params": {
                "name": "relay_retrieve_context_prompt",
                "arguments": {"query": "metadata", "source_scope": "docs"}
            }
        }),
        &session_id,
    )
    .await;
    let delete = raw_custom_request(
        &mut router,
        "DELETE",
        "/mcp",
        "",
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    let after_delete = raw_mcp_request(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "ping", "method": "ping"}),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;

    assert_eq!(
        resources["result"]["resources"][0]["uri"],
        "relay://service/status"
    );
    assert!(
        service_status["result"]["contents"][0]["text"]
            .as_str()
            .expect("resource text")
            .contains("agent_protocols")
    );
    assert!(
        index_status["result"]["contents"][0]["text"]
            .as_str()
            .expect("index status text")
            .contains("indexes")
    );
    assert_eq!(
        prompts["result"]["prompts"][0]["name"],
        "relay_retrieve_context_prompt"
    );
    assert!(
        prompt["result"]["messages"][0]["content"]["text"]
            .as_str()
            .expect("prompt text")
            .contains("evidence")
    );
    assert_eq!(delete.0, StatusCode::ACCEPTED);
    assert_eq!(after_delete.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_session_enforces_origin_allowlist() {
    let server = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        (
            "RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS",
            "https://trusted.example",
        ),
    ])
    .await;
    let mut router = server.router();
    let (init_status, init_headers, init_response) = raw_mcp_response(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "init",
            "method": "initialize",
            "params": initialize_params()
        }),
        [("origin", "https://trusted.example")],
    )
    .await;
    assert_eq!(init_status, StatusCode::OK);
    assert_eq!(
        init_response["result"]["protocolVersion"],
        MCP_PROTOCOL_VERSION
    );
    let session_id = init_headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("initialize should issue a session")
        .to_owned();
    let initialized = raw_mcp_request(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
        [
            (MCP_SESSION_ID_HEADER, session_id.as_str()),
            ("origin", "https://trusted.example"),
        ],
    )
    .await;
    let rejected_delete = raw_custom_request(
        &mut router,
        "DELETE",
        "/mcp",
        "",
        [
            (MCP_SESSION_ID_HEADER, session_id.as_str()),
            ("origin", "https://attacker.example"),
        ],
    )
    .await;
    let still_active = raw_mcp_request(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "ping", "method": "ping"}),
        [
            (MCP_SESSION_ID_HEADER, session_id.as_str()),
            ("origin", "https://trusted.example"),
        ],
    )
    .await;

    assert_eq!(initialized.0, StatusCode::ACCEPTED);
    assert_eq!(rejected_delete.0, StatusCode::FORBIDDEN);
    assert_eq!(still_active.0, StatusCode::OK);
}

#[tokio::test]
async fn delete_session_uses_qos_admission_and_releases_permit() {
    let server = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "1"),
    ])
    .await;
    let session_id = {
        let mut setup_router = server.clone().router();
        initialize_session(&mut setup_router).await
    };
    let policy = server.network.current().qos;
    let occupied = server
        .qos
        .admit_request(&policy)
        .expect("test should occupy budget");
    let mut router = server.clone().router();

    let rejected_delete = raw_custom_request(
        &mut router,
        "DELETE",
        "/mcp",
        "",
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    drop(occupied);
    let still_active = raw_mcp_request(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "ping", "method": "ping"}),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    let accepted_delete = raw_custom_request(
        &mut router,
        "DELETE",
        "/mcp",
        "",
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;

    assert_eq!(rejected_delete.0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(still_active.0, StatusCode::OK);
    assert_eq!(accepted_delete.0, StatusCode::ACCEPTED);
    assert_eq!(server.qos_snapshot().in_flight_requests, 0);
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
        "relay_retrieve_context",
        json!({"query": "slow", "source_scope": "docs"}),
    )
    .await;

    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error_kind"],
        "timeout"
    );
}

#[tokio::test]
async fn resources_and_metrics_respect_runtime_timeout() {
    let store = Arc::new(SlowSearchStore);
    let (server, _service) = server_and_service_with_store(
        [
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
            ("RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS", "10"),
        ],
        store,
    )
    .await;
    let mut router = server.clone().router();

    let resource = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "slow-health-resource",
            "method": "resources/read",
            "params": {"uri": "relay://service/health"}
        }),
    )
    .await;
    let metrics = raw_custom_request(&mut router, "GET", "/mcp/metrics", "", []).await;

    assert_eq!(resource["error"]["code"], -32000);
    assert!(
        resource["error"]["message"]
            .as_str()
            .expect("message")
            .contains("max_runtime_ms")
    );
    assert_eq!(metrics.0, StatusCode::REQUEST_TIMEOUT);
    assert_eq!(server.qos_snapshot().in_flight_requests, 0);

    let audit = server.audit_snapshot();
    let resource_audit = audit
        .iter()
        .find(|event| event.operation == "resources/read")
        .expect("resources/read audit event");
    assert_eq!(resource_audit.status, AgentAuditStatus::Failed);
    assert_eq!(resource_audit.error_kind.as_deref(), Some("timeout"));
}

#[tokio::test]
async fn method_level_reads_are_recorded_in_durable_audit() {
    let (server, service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut router = server.router();

    let tools = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "tools-list-metrics",
            "method": "tools/list",
            "params": {}
        }),
    )
    .await;
    let resource = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "resource-audit",
            "method": "resources/read",
            "params": {"uri": "relay://service/status"}
        }),
    )
    .await;
    let response = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "prompt-audit",
            "method": "prompts/get",
            "params": {
                "name": "relay_retrieve_context_prompt",
                "arguments": {"query": "audit", "source_scope": "docs"}
            }
        }),
    )
    .await;

    assert!(tools["result"]["tools"].as_array().expect("tools").len() > 1);
    assert!(resource["result"]["contents"][0]["text"].is_string());
    assert_eq!(response["result"]["description"], "Retrieve Graph Context");

    let resource_audit = service
        .query_audit(
            AuditQueryApiRequest {
                operation: Some("resources/read".to_owned()),
                limit: 1,
            },
            RequestContext::with_ids(
                InterfaceKind::Cli,
                "req-resource-audit-query",
                "trace-resource-audit-query",
            ),
        )
        .await
        .expect("durable resource audit should query");
    let durable = service
        .query_audit(
            AuditQueryApiRequest {
                operation: Some("prompts/get".to_owned()),
                limit: 1,
            },
            RequestContext::with_ids(
                InterfaceKind::Cli,
                "req-prompt-audit-query",
                "trace-prompt-audit-query",
            ),
        )
        .await
        .expect("durable prompt audit should query");
    let resource_event = resource_audit
        .events
        .first()
        .expect("durable resource audit");
    assert_eq!(resource_event.operation, "resources/read");
    assert_eq!(resource_event.status, crate::domain::AuditStatus::Completed);
    assert!(
        resource_event
            .request_id
            .ends_with("|string:resource-audit")
    );

    let event = durable.events.first().expect("durable prompt audit");
    assert_eq!(event.operation, "prompts/get");
    assert_eq!(event.status, crate::domain::AuditStatus::Completed);
    assert!(event.request_id.ends_with("|string:prompt-audit"));

    let metrics = service.observability().status().agent_protocol;
    assert_eq!(metrics.requests_total, 3);
    assert_eq!(metrics.rejections_total, 0);
}

async fn wait_for_mcp_in_flight(server: &McpServer, expected: usize) {
    for _ in 0..50 {
        if server.qos_snapshot().in_flight_requests == expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    panic!("MCP QoS in-flight count did not reach {expected}");
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
