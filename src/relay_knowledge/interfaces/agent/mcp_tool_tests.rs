use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header},
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::{
    api::{AuditQueryApiRequest, CodeRepositoryRegisterRequest, IngestEvidence, IngestRequest},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeRepositorySelector, CodeRepositorySetAddMemberRequest,
        CodeRepositorySetCreateRequest, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    interfaces::agent::AgentAuditStatus,
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn code_query_tool_returns_indexed_repository_hits_and_audit() {
    let repo = FixtureRepo::create("mcp-code-query");
    repo.write(
        "src/lib.rs",
        r#"
/// Returns the retry budget.
pub fn retry_policy() -> u32 {
    3
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let (server, service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "fixture")]).await;
    register_and_index_fixture(&service, &repo, "fixture", "HEAD", CodeIndexMode::Full).await;
    let mut router = server.clone().router();

    let response = tool_call(
        &mut router,
        "code-query",
        "relay_code_query",
        json!({
            "repository": "fixture",
            "query": "retry_policy",
            "kind": "definition",
            "limit": 5,
            "freshness": "wait-until-fresh"
        }),
    )
    .await;

    let structured = &response["result"]["structuredContent"];
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(structured["request"]["repository"]["repository"], "fixture");
    assert_eq!(structured["results"][0]["path"], "src/lib.rs");

    let audit = server.audit_snapshot();
    let event = audit.last().expect("tool call should write audit event");
    assert_eq!(event.operation, "relay_code_query");
    assert_eq!(event.source_scope.as_deref(), Some("fixture"));
    assert_eq!(event.result_count, Some(1));
    assert_eq!(event.status, AgentAuditStatus::Completed);
}

#[tokio::test]
async fn mcp_tools_auto_authorize_registered_repository_alias_at_runtime() {
    let repo = FixtureRepo::create("mcp-runtime-scope");
    repo.write(
        "src/lib.rs",
        r#"
pub fn runtime_scope_policy() -> bool {
    true
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let (server, service) = server_and_service([]).await;
    register_and_index_fixture(&service, &repo, "fixture", "HEAD", CodeIndexMode::Full).await;
    service
        .ingest(
            IngestRequest {
                source_scope: "fixture".to_owned(),
                evidence: vec![IngestEvidence {
                    id: Some("ev-runtime-scope".to_owned()),
                    source_path: None,
                    span: None,
                    confidence: None,
                    status: None,
                    content: "Registered repository aliases can be promoted into MCP runtime scope policy."
                        .to_owned(),
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
    let mut router = server.clone().router();

    let code_query = tool_call(
        &mut router,
        "runtime-code-query",
        "relay_code_query",
        json!({
            "repository": "fixture",
            "query": "runtime_scope_policy",
            "kind": "definition",
            "limit": 5,
            "freshness": "wait-until-fresh"
        }),
    )
    .await;
    let retrieve = tool_call(
        &mut router,
        "runtime-retrieve",
        "relay_retrieve_context",
        json!({
            "query": "runtime scope policy",
            "source_scope": "fixture",
            "limit": 3,
            "freshness": "wait-until-fresh"
        }),
    )
    .await;

    assert_eq!(code_query["result"]["isError"], false);
    assert_eq!(
        code_query["result"]["structuredContent"]["request"]["repository"]["repository"],
        "fixture"
    );
    assert_eq!(retrieve["result"]["isError"], false);
    assert_eq!(
        retrieve["result"]["structuredContent"]["source_scope"],
        "fixture"
    );

    let audit = server.audit_snapshot();
    assert!(
        audit
            .iter()
            .any(|event| event.operation == "relay_code_query"
                && event.source_scope.as_deref() == Some("fixture"))
    );
    assert!(
        audit
            .iter()
            .any(|event| event.operation == "relay_retrieve_context"
                && event.source_scope.as_deref() == Some("fixture"))
    );
}

#[tokio::test]
async fn repository_set_tool_requires_set_or_member_scope_authorization() {
    let repo = FixtureRepo::create("mcp-repo-set-authorization");
    repo.write(
        "src/lib.rs",
        r#"
pub fn workspace_symbol() -> u32 {
    1
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let (denied_server, denied_service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "other")]).await;
    create_fixture_repository_set(&denied_service, &repo).await;
    let mut denied_router = denied_server.router();

    let denied = tool_call(
        &mut denied_router,
        "repo-set-denied",
        "relay_code_repository_set_query",
        json!({
            "repository_set": "workspace",
            "query": "workspace_symbol",
            "kind": "definition",
            "limit": 5
        }),
    )
    .await;

    assert_eq!(denied["result"]["isError"], true);
    assert_eq!(
        denied["result"]["structuredContent"]["error_kind"],
        "permission_denied"
    );

    let (allowed_server, allowed_service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "fixture")]).await;
    create_fixture_repository_set(&allowed_service, &repo).await;
    let mut allowed_router = allowed_server.clone().router();

    let allowed = tool_call(
        &mut allowed_router,
        "repo-set-allowed",
        "relay_code_repository_set_query",
        json!({
            "repository_set": "workspace",
            "query": "workspace_symbol",
            "kind": "definition",
            "limit": 5
        }),
    )
    .await;

    assert_eq!(allowed["result"]["isError"], false);
    assert_eq!(
        allowed["result"]["structuredContent"]["results"][0]["hit"]["path"],
        "src/lib.rs"
    );
    let audit = allowed_server.audit_snapshot();
    let event = audit.last().expect("repo-set query should audit");
    assert_eq!(event.operation, "relay_code_repository_set_query");
    assert_eq!(event.source_scope.as_deref(), Some("workspace"));

    let restricted = FixtureRepo::create("mcp-repo-set-restricted");
    restricted.write(
        "src/lib.rs",
        r#"
pub fn restricted_symbol() -> u32 {
    2
}
"#,
    );
    restricted.git(["add", "."]);
    restricted.git(["commit", "-m", "initial"]);
    register_and_index_fixture(
        &allowed_service,
        &restricted,
        "restricted",
        "HEAD",
        CodeIndexMode::Full,
    )
    .await;
    allowed_service
        .add_code_repository_set_member(
            CodeRepositorySetAddMemberRequest::new(
                "workspace",
                "restricted",
                "HEAD",
                Vec::new(),
                Vec::new(),
                0,
            )
            .expect("member request should validate"),
            RequestContext::with_ids(
                InterfaceKind::Cli,
                "req-set-restricted",
                "trace-set-restricted",
            ),
        )
        .await
        .expect("restricted member should add");

    let revalidated = tool_call(
        &mut allowed_router,
        "repo-set-revalidated",
        "relay_code_repository_set_query",
        json!({
            "repository_set": "workspace",
            "query": "workspace_symbol",
            "kind": "definition",
            "limit": 5
        }),
    )
    .await;

    assert_eq!(revalidated["result"]["isError"], true);
    assert_eq!(
        revalidated["result"]["structuredContent"]["error_kind"],
        "permission_denied"
    );
}

#[tokio::test]
async fn repository_set_tool_rejects_repository_alias_collision() {
    let colliding = FixtureRepo::create("mcp-repo-set-collision");
    colliding.write(
        "src/lib.rs",
        r#"
pub fn colliding_repository_symbol() -> u32 {
    1
}
"#,
    );
    colliding.git(["add", "."]);
    colliding.git(["commit", "-m", "initial"]);
    let restricted = FixtureRepo::create("mcp-repo-set-collision-member");
    restricted.write(
        "src/lib.rs",
        r#"
pub fn restricted_member_symbol() -> u32 {
    2
}
"#,
    );
    restricted.git(["add", "."]);
    restricted.git(["commit", "-m", "initial"]);

    let (server, service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "workspace")]).await;
    register_and_index_fixture(
        &service,
        &colliding,
        "workspace",
        "HEAD",
        CodeIndexMode::Full,
    )
    .await;
    register_and_index_fixture(
        &service,
        &restricted,
        "restricted",
        "HEAD",
        CodeIndexMode::Full,
    )
    .await;
    service
        .create_code_repository_set(
            CodeRepositorySetCreateRequest::new("workspace", None, None)
                .expect("set request should validate"),
            RequestContext::with_ids(InterfaceKind::Cli, "req-set", "trace-set"),
        )
        .await
        .expect("repository set should create");
    service
        .add_code_repository_set_member(
            CodeRepositorySetAddMemberRequest::new(
                "workspace",
                "restricted",
                "HEAD",
                Vec::new(),
                Vec::new(),
                0,
            )
            .expect("member request should validate"),
            RequestContext::with_ids(InterfaceKind::Cli, "req-set-member", "trace-set-member"),
        )
        .await
        .expect("repository set member should add");
    let mut router = server.router();

    let response = tool_call(
        &mut router,
        "repo-set-collision",
        "relay_code_repository_set_query",
        json!({
            "repository_set": "workspace",
            "query": "restricted_member_symbol",
            "kind": "definition",
            "limit": 5
        }),
    )
    .await;

    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error_kind"],
        "permission_denied"
    );
}

#[tokio::test]
async fn code_impact_tool_returns_diff_impact_and_audit() {
    let repo = FixtureRepo::create("mcp-code-impact");
    repo.write(
        "src/lib.rs",
        r#"
pub fn retry_policy() -> u32 {
    3
}
"#,
    );
    repo.write(
        "src/main.rs",
        r#"
use fixture::retry_policy;

fn run_worker() {
    retry_policy();
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let base_ref = repo.git_text(["rev-parse", "HEAD"]);
    let (server, service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "fixture")]).await;
    register_and_index_fixture(&service, &repo, "fixture", "HEAD", CodeIndexMode::Full).await;
    repo.write(
        "src/lib.rs",
        r#"
pub fn retry_policy() -> u32 {
    5
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update retry policy"]);
    let head_ref = repo.git_text(["rev-parse", "HEAD"]);
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: CodeRepositorySelector::new(
                    "fixture",
                    head_ref.clone(),
                    Vec::new(),
                    Vec::new(),
                )
                .expect("selector should validate"),
                mode: CodeIndexMode::incremental(base_ref.clone(), head_ref.clone())
                    .expect("incremental refs should validate"),
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-update", "trace-update"),
        )
        .await
        .expect("repository should update");
    let mut router = server.clone().router();

    let response = tool_call(
        &mut router,
        "code-impact",
        "relay_code_impact",
        json!({
            "repository": "fixture",
            "base_ref": base_ref,
            "head_ref": head_ref,
            "limit": 10
        }),
    )
    .await;

    let structured = &response["result"]["structuredContent"];
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        structured["path_groups"]["in_scope_changed_paths"][0],
        "src/lib.rs"
    );
    assert!(
        !structured["results"]
            .as_array()
            .expect("results")
            .is_empty()
    );

    let audit = server.audit_snapshot();
    let event = audit.last().expect("tool call should write audit event");
    assert_eq!(event.operation, "relay_code_impact");
    assert_eq!(event.source_scope.as_deref(), Some("fixture"));
    assert_eq!(event.status, AgentAuditStatus::Completed);
}

#[tokio::test]
async fn diagnostic_tools_return_structured_content() {
    let (server, service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![IngestEvidence {
                    id: Some("ev-diagnostics".to_owned()),
                    source_path: None,
                    span: None,
                    confidence: None,
                    status: None,
                    content: "Diagnostic tools share application service state".to_owned(),
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

    let inspect = tool_call(
        &mut router,
        "inspect",
        "relay_inspect_graph",
        json!({"source_scope": "docs"}),
    )
    .await;
    let health = tool_call(&mut router, "health", "relay_health", json!({})).await;
    let service_status = tool_call(&mut router, "service", "relay_service_status", json!({})).await;
    let index_status = tool_call(&mut router, "index", "relay_index_status", json!({})).await;

    assert_eq!(
        inspect["result"]["structuredContent"]["graph"]["evidence_count"],
        1
    );
    assert_eq!(health["result"]["structuredContent"]["healthy"], true);
    assert_eq!(
        service_status["result"]["structuredContent"]["agent_protocols"]["mcp_streamable_http_enabled"],
        true
    );
    assert_eq!(
        index_status["result"]["structuredContent"]["indexes"]
            .as_array()
            .expect("indexes")
            .len(),
        3
    );
}

#[tokio::test]
async fn mcp_never_lists_or_runs_index_refresh_tools() {
    let server = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ("RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH", "true"),
    ])
    .await;
    let mut router = server.router();
    let listed = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "tools",
            "method": "tools/list"
        }),
    )
    .await;
    let old_name = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "refresh-old",
            "method": "tools/call",
            "params": {
                "name": "relay.refresh_indexes",
                "arguments": {"kinds": ["bm25"]}
            }
        }),
    )
    .await;
    let snake_name = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "refresh-snake",
            "method": "tools/call",
            "params": {
                "name": "relay_refresh_indexes",
                "arguments": {"kinds": ["bm25"]}
            }
        }),
    )
    .await;

    let names = tool_names(&listed);
    assert!(!names.contains(&"relay.refresh_indexes".to_owned()));
    assert!(!names.contains(&"relay_refresh_indexes".to_owned()));
    assert_eq!(old_name["error"]["code"], -32602);
    assert_eq!(snake_name["error"]["code"], -32602);
}

#[tokio::test]
async fn persistent_audit_sink_writes_jsonl_events_when_enabled() {
    let runtime_root = FixtureRepo::create("mcp-audit-runtime");
    let root = runtime_root.path.display().to_string();
    let server = server_with_env([
        ("RELAY_KNOWLEDGE_HOME", root.as_str()),
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ("RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED", "true"),
        ("RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH", "4"),
    ])
    .await;
    let mut router = server.router();

    let response = tool_call(&mut router, "audit-health", "relay_health", json!({})).await;
    let audit_path = runtime_root.path.join("logs").join("agent-audit.jsonl");
    let audit = wait_for_audit_line(&audit_path, "relay_health").await;
    let event: Value = serde_json::from_str(&audit).expect("audit event should be json");

    assert_eq!(response["result"]["isError"], false);
    assert_eq!(event["operation"], "relay_health");
    assert_eq!(event["status"], "completed");
    assert_eq!(event["qos_decision"], "admitted");
}

#[tokio::test]
async fn durable_mcp_audit_records_result_graph_version() {
    let (server, service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![IngestEvidence {
                    id: Some("ev-audit-version".to_owned()),
                    source_path: None,
                    span: None,
                    confidence: None,
                    status: None,
                    content: "Audit graph version should follow tool results".to_owned(),
                    entity_labels: Vec::new(),
                    extraction: None,
                }],
                relations: Vec::new(),
                claims: Vec::new(),
                events: Vec::new(),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-audit-ingest", "trace-audit-ingest"),
        )
        .await
        .expect("ingest should succeed");
    let mut router = server.router();

    let response = tool_call(
        &mut router,
        "audit-health-version",
        "relay_health",
        json!({}),
    )
    .await;
    assert_eq!(response["result"]["isError"], false);

    let audit = service
        .query_audit(
            AuditQueryApiRequest {
                operation: Some("relay_health".to_owned()),
                limit: 1,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-audit-query", "trace-audit-query"),
        )
        .await
        .expect("durable audit should query");

    let event = audit.events.first().expect("durable audit event");
    assert_eq!(event.graph_version, 1);
}

async fn register_and_index_fixture(
    service: &RelayKnowledgeService,
    repo: &FixtureRepo,
    alias: &str,
    ref_selector: &str,
    mode: CodeIndexMode,
) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: alias.to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-register", "trace-register"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: CodeRepositorySelector::new(
                    alias,
                    ref_selector,
                    Vec::new(),
                    Vec::new(),
                )
                .expect("selector should validate"),
                mode,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-index", "trace-index"),
        )
        .await
        .expect("repository should index");
}

async fn create_fixture_repository_set(service: &RelayKnowledgeService, repo: &FixtureRepo) {
    register_and_index_fixture(service, repo, "fixture", "HEAD", CodeIndexMode::Full).await;
    service
        .create_code_repository_set(
            CodeRepositorySetCreateRequest::new("workspace", None, None)
                .expect("set request should validate"),
            RequestContext::with_ids(InterfaceKind::Cli, "req-set", "trace-set"),
        )
        .await
        .expect("repository set should create");
    service
        .add_code_repository_set_member(
            CodeRepositorySetAddMemberRequest::new(
                "workspace",
                "fixture",
                "HEAD",
                Vec::new(),
                Vec::new(),
                0,
            )
            .expect("member request should validate"),
            RequestContext::with_ids(InterfaceKind::Cli, "req-set-member", "trace-set-member"),
        )
        .await
        .expect("repository set member should add");
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

async fn raw_mcp_request<const N: usize>(
    router: &mut Router,
    payload: Value,
    headers: [(&str, &str); N],
) -> (StatusCode, Value) {
    let (status, _headers, value) =
        raw_custom_response(router, "POST", "/mcp", &payload.to_string(), headers).await;
    (status, value)
}

async fn raw_mcp_response<const N: usize>(
    router: &mut Router,
    payload: Value,
    headers: [(&str, &str); N],
) -> (StatusCode, HeaderMap, Value) {
    raw_custom_response(router, "POST", "/mcp", &payload.to_string(), headers).await
}

async fn raw_custom_response<const N: usize>(
    router: &mut Router,
    method: &str,
    uri: &str,
    body: &str,
    headers: [(&str, &str); N],
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(MCP_PROTOCOL_VERSION_HEADER, MCP_PROTOCOL_VERSION);
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

struct FixtureRepo {
    path: PathBuf,
}

impl FixtureRepo {
    fn create(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("relay-knowledge-{name}-{nanos}"));
        fs::create_dir_all(path.join("src")).expect("repo directory should be created");
        let repo = Self { path };
        repo.git(["init"]);
        repo.git(["config", "user.email", "relay@example.invalid"]);
        repo.git(["config", "user.name", "Relay Test"]);
        repo
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }

    fn git<const N: usize>(&self, args: [&str; N]) {
        let output = git_command(&self.path, args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_text<const N: usize>(&self, args: [&str; N]) -> String {
        let output = git_command(&self.path, args)
            .output()
            .expect("git should run");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

impl Drop for FixtureRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn git_command<const N: usize>(path: &Path, args: [&str; N]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    command
}

async fn wait_for_audit_line(path: &Path, operation: &str) -> String {
    for _ in 0..40 {
        if let Ok(contents) = fs::read_to_string(path) {
            if let Some(line) = contents
                .lines()
                .find(|line| line.contains(operation))
                .map(str::to_owned)
            {
                return line;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    panic!(
        "audit line for {operation} was not written to {}",
        path.display()
    );
}
