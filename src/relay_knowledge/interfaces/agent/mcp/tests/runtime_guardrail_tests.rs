//! Cross-owner MCP runtime guardrail scenarios.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::http::StatusCode;
use serde_json::{Value, json};

use super::test_support::SlowSearchStore;

use super::tests::{
    call_mcp, call_mcp_with_session, initialize_session, raw_custom_request, server_and_service,
    server_and_service_with_store, tool_call, tool_names,
};
use super::*;
use crate::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{CodeIndexMode, CodeIndexRequest, CodeRepositorySelector, FreshnessPolicy},
    env::{EnvironmentConfig, PlatformKind},
    interfaces::agent::AgentAuditStatus,
};

#[tokio::test]
async fn initialize_tools_list_and_readonly_diagnostics_do_not_open_storage() {
    let home = unique_temp_dir("mcp-lazy-init");
    let home_text = home.to_str().expect("temp path should be UTF-8").to_owned();
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", home_text.as_str()),
            ("RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED", "true"),
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let database_path = runtime.paths.database_file();
    let service = RelayKnowledgeService::new(runtime.clone());
    let server = McpServer::new(service, runtime.network.clone(), runtime.agent.clone());
    let mut router = server.clone().router();

    assert!(!database_path.exists());
    let session_id = initialize_session(&mut router).await;
    let tools = call_mcp_with_session(
        &mut router,
        json!({"jsonrpc": "2.0", "id": "tools", "method": "tools/list"}),
        &session_id,
    )
    .await;
    let health = tool_call(&mut router, "health", "relay_health", json!({})).await;
    let status = tool_call(
        &mut router,
        "service-status",
        "relay_service_status",
        json!({}),
    )
    .await;

    assert!(tool_names(&tools).contains(&"relay_code_query".to_owned()));
    assert_eq!(server.metrics.snapshot().cold_start_total, 1);
    assert!(
        tools["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .any(|tool| tool["inputSchema"]["properties"]["query"]
                .get("maxLength")
                .is_some())
    );
    assert_eq!(health["result"]["isError"], false);
    assert_eq!(status["result"]["isError"], false);
    assert!(!database_path.exists());
    let index_status =
        tool_call(&mut router, "index-status", "relay_index_status", json!({})).await;
    assert_eq!(index_status["result"]["isError"], false);
    assert!(database_path.exists());
    let _ = fs::remove_dir_all(home);
}

#[tokio::test]
async fn tool_inputs_reject_oversized_queries_and_paths() {
    let server = server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")])
        .await
        .0;
    let mut router = server.router();
    let too_long_query = "q".repeat(10_001);
    let too_long_path = "p".repeat(4_097);

    let retrieve = tool_call(
        &mut router,
        "long-retrieve",
        "relay_retrieve_context",
        json!({"query": too_long_query, "source_scope": "docs"}),
    )
    .await;
    let code = tool_call(
        &mut router,
        "long-path",
        "relay_code_query",
        json!({
            "repository": "docs",
            "query": "target",
            "path_filters": [too_long_path]
        }),
    )
    .await;

    assert_eq!(
        retrieve["result"]["structuredContent"]["error_kind"],
        "invalid_argument"
    );
    assert_eq!(
        code["result"]["structuredContent"]["error_kind"],
        "invalid_argument"
    );
}

#[tokio::test]
async fn code_query_returns_adaptive_budget_and_container_outlines() {
    let repo = FixtureRepo::create("mcp-issue-283-budget");
    for index in 0..6 {
        repo.write(
            &format!("src/file_{index}.rs"),
            &format!(
                "pub struct Target{index} {{\n    value: u32,\n}}\n\nimpl Target{index} {{\n    pub fn target_method(&self) -> u32 {{\n        self.value\n    }}\n}}\n"
            ),
        );
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let (server, service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "fixture")]).await;
    register_and_index_fixture(&service, &repo, "fixture").await;
    let mut router = server.router();

    let response = tool_call(
        &mut router,
        "budgeted-code-query",
        "relay_code_query",
        json!({
            "repository": "fixture",
            "query": "Target target_method",
            "kind": "hybrid",
            "limit": 10,
            "include_code": true,
            "freshness": "wait-until-fresh"
        }),
    )
    .await;

    let structured = &response["result"]["structuredContent"];
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(structured["explore_budget"]["calls"], 1);
    assert_eq!(structured["explore_budget"]["max_files"], 5);
    assert_eq!(structured["results"].as_array().expect("results").len(), 5);
    assert_eq!(
        response["result"]["content"][0]["text"],
        "code query returned 5 result(s)"
    );
    assert_eq!(structured["truncated"], true);
    assert_eq!(structured["agent_output"]["truncated"], true);
    let outline_response = tool_call(
        &mut router,
        "outlined-code-query",
        "relay_code_query",
        json!({
            "repository": "fixture",
            "query": "Target0",
            "kind": "hybrid",
            "limit": 5,
            "include_code": true,
            "freshness": "wait-until-fresh"
        }),
    )
    .await;
    let outlined = &outline_response["result"]["structuredContent"];
    assert!(
        outlined["results"]
            .as_array()
            .expect("results")
            .iter()
            .any(|result| result["source_outline"].as_bool() == Some(true)
                && result["excerpt"]
                    .as_str()
                    .is_some_and(|excerpt| excerpt.contains("pub struct Target")))
    );
}

async fn register_and_index_fixture(
    service: &RelayKnowledgeService,
    repo: &FixtureRepo,
    alias: &str,
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
                repository: CodeRepositorySelector::new(alias, "HEAD", Vec::new(), Vec::new())
                    .expect("selector should validate"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-index", "trace-index"),
        )
        .await
        .expect("repository should index");
}

struct FixtureRepo {
    path: PathBuf,
}

impl FixtureRepo {
    fn create(name: &str) -> Self {
        let path = unique_temp_dir(name);
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

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-{name}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    path
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
    let (mut server, _service) = server_and_service_with_store(
        [
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
            ("RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS", "1000"),
        ],
        store,
    )
    .await;
    // Keep the adapter deadline shorter than the storage budget so this test
    // exercises transport cancellation instead of a successful degraded health read.
    server.agent.access_policy.max_runtime_ms = 10;
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
async fn health_resource_and_metrics_return_before_the_shared_request_deadline() {
    let (server, _service) = server_and_service_with_store(
        [("RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS", "100")],
        Arc::new(SlowSearchStore),
    )
    .await;
    let mut router = server.clone().router();
    let resource = call_mcp(
        &mut router,
        json!({
            "jsonrpc": "2.0",
            "id": "degraded-health-resource",
            "method": "resources/read",
            "params": {"uri": "relay://service/health"}
        }),
    )
    .await;
    let health: Value = serde_json::from_str(
        resource["result"]["contents"][0]["text"]
            .as_str()
            .expect("health resource remains readable"),
    )
    .expect("health JSON");
    assert_eq!(health["healthy"], false);
    assert!(
        health["degraded_reason"]
            .as_str()
            .unwrap()
            .starts_with("storage_busy:")
    );
    assert_eq!(health["runtime"]["health_storage_budget_ms"], 50);
    let metrics = raw_custom_request(&mut router, "GET", "/mcp/metrics", "", []).await;
    assert_eq!(metrics.0, StatusCode::OK);
    assert_eq!(server.qos_snapshot().in_flight_requests, 0);
    let audit = server.audit_snapshot();
    let resource_audit = audit
        .iter()
        .find(|event| event.operation == "resources/read")
        .expect("resources/read audit event");
    assert_eq!(resource_audit.status, AgentAuditStatus::Completed);
    assert_eq!(resource_audit.error_kind, None);
}
