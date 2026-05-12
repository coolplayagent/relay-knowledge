use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
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
    api::{CodeRepositoryRegisterRequest, IngestEvidence, IngestRequest},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{CodeIndexMode, CodeIndexRequest, CodeRepositorySelector, FreshnessPolicy},
    env::{EnvironmentConfig, PlatformKind},
    interfaces::agent::AgentAuditStatus,
    storage::SqliteGraphStore,
};

use super::mcp_test_support::RefreshFailStore;

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
    register_and_index_fixture(&service, &repo, "HEAD", CodeIndexMode::Full).await;
    let mut router = server.clone().router();

    let response = tool_call(
        &mut router,
        "code-query",
        "relay.code_query",
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
    assert_eq!(event.operation, "relay.code_query");
    assert_eq!(event.source_scope.as_deref(), Some("fixture"));
    assert_eq!(event.result_count, Some(1));
    assert_eq!(event.status, AgentAuditStatus::Completed);
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
    register_and_index_fixture(&service, &repo, "HEAD", CodeIndexMode::Full).await;
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
        "relay.code_impact",
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
    assert_eq!(structured["changed_paths"][0], "src/lib.rs");
    assert!(
        !structured["results"]
            .as_array()
            .expect("results")
            .is_empty()
    );

    let audit = server.audit_snapshot();
    let event = audit.last().expect("tool call should write audit event");
    assert_eq!(event.operation, "relay.code_impact");
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
        "relay.inspect_graph",
        json!({"source_scope": "docs"}),
    )
    .await;
    let health = tool_call(&mut router, "health", "relay.health", json!({})).await;
    let service_status = tool_call(&mut router, "service", "relay.service_status", json!({})).await;
    let index_status = tool_call(&mut router, "index", "relay.index_status", json!({})).await;

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
async fn refresh_indexes_tool_requires_policy_and_valid_kinds() {
    let disabled = server_with_env([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let mut disabled_router = disabled.router();
    let denied = tool_call(
        &mut disabled_router,
        "refresh-denied",
        "relay.refresh_indexes",
        json!({"kinds": ["bm25"]}),
    )
    .await;

    let enabled = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ("RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH", "true"),
    ])
    .await;
    let mut enabled_router = enabled.router();
    let listed = call_mcp(
        &mut enabled_router,
        json!({
            "jsonrpc": "2.0",
            "id": "tools",
            "method": "tools/list"
        }),
    )
    .await;
    let invalid = tool_call(
        &mut enabled_router,
        "refresh-invalid",
        "relay.refresh_indexes",
        json!({"kinds": ["other"]}),
    )
    .await;
    let refreshed = tool_call(
        &mut enabled_router,
        "refresh-ok",
        "relay.refresh_indexes",
        json!({"kinds": ["semantic"]}),
    )
    .await;

    assert_eq!(
        denied["result"]["structuredContent"]["error_kind"],
        "permission_denied"
    );
    assert!(tool_names(&listed).contains(&"relay.refresh_indexes".to_owned()));
    assert_eq!(
        invalid["result"]["structuredContent"]["error_kind"],
        "invalid_argument"
    );
    assert_eq!(
        refreshed["result"]["structuredContent"]["indexes"][0]["kind"],
        "semantic"
    );
}

#[tokio::test]
async fn service_api_errors_preserve_error_kind_in_tool_results() {
    let store = Arc::new(RefreshFailStore);
    let (server, _service) = server_and_service_with_store(
        [
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
            ("RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH", "true"),
        ],
        store,
    )
    .await;
    let mut router = server.router();

    let response = tool_call(
        &mut router,
        "refresh-fails",
        "relay.refresh_indexes",
        json!({"kinds": ["bm25"]}),
    )
    .await;

    assert_eq!(
        response["result"]["structuredContent"]["error_kind"],
        "storage_unavailable"
    );
}

async fn register_and_index_fixture(
    service: &RelayKnowledgeService,
    repo: &FixtureRepo,
    ref_selector: &str,
    mode: CodeIndexMode,
) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-register", "trace-register"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: CodeRepositorySelector::new(
                    "fixture",
                    ref_selector,
                    Vec::new(),
                    Vec::new(),
                )
                .expect("selector should validate"),
                mode,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-index", "trace-index"),
        )
        .await
        .expect("repository should index");
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
