use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use super::tests::{server_and_service, server_with_env, tool_call};
use crate::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::RelayKnowledgeService,
    domain::{CodeIndexMode, CodeIndexRequest, CodeRepositorySelector, FreshnessPolicy},
};

#[tokio::test]
async fn repository_graph_tool_enforces_node_and_edge_policy_limits() {
    let server = server_with_env([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "fixture"),
        ("RELAY_KNOWLEDGE_MCP_MAX_LIMIT", "2"),
    ])
    .await;
    let mut router = server.router();
    let node_limit = tool_call(
        &mut router,
        "repository-graph-node-limit",
        "relay_repository_graph",
        json!({
            "repository": "fixture",
            "focus_path": "focus.md",
            "path_filters": ["."],
            "node_limit": 3,
            "edge_limit": 2
        }),
    )
    .await;
    let edge_limit = tool_call(
        &mut router,
        "repository-graph-edge-limit",
        "relay_repository_graph",
        json!({
            "repository": "fixture",
            "focus_path": "focus.md",
            "path_filters": ["."],
            "node_limit": 2,
            "edge_limit": 3
        }),
    )
    .await;

    assert_eq!(
        node_limit["result"]["structuredContent"]["error_kind"],
        "limit_exceeded"
    );
    assert_eq!(
        edge_limit["result"]["structuredContent"]["error_kind"],
        "limit_exceeded"
    );
}

#[tokio::test]
async fn repository_graph_tool_clamps_omitted_limits_and_caps_full_structured_content() {
    let repo = FixtureRepo::create("mcp-repository-graph-budget");
    let description = "  bounded evidence for MCP output compaction\n".repeat(160);
    repo.write(
        "knowledge/focus.md",
        &format!(
            "---\ntype: Research Claim\ntitle: Focus\ndescription: |\n{description}---\n[Neighbor](neighbor.md)\n"
        ),
    );
    repo.write(
        "knowledge/neighbor.md",
        "---\ntype: Research Claim\ntitle: Neighbor\n---\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "add OKF bundle"]);
    let (server, service) = server_and_service([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "fixture"),
        ("RELAY_KNOWLEDGE_MCP_MAX_LIMIT", "2"),
        ("RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES", "4096"),
    ])
    .await;
    register_and_index_fixture(&service, &repo).await;
    let mut router = server.router();

    let response = tool_call(
        &mut router,
        "repository-graph-default-budget",
        "relay_repository_graph",
        json!({
            "repository": "fixture",
            "focus_path": "knowledge/focus.md",
            "path_filters": ["knowledge"]
        }),
    )
    .await;
    let structured = &response["result"]["structuredContent"];

    assert_eq!(
        response["result"]["isError"], false,
        "unexpected tool response: {response:#}"
    );
    assert_eq!(structured["request"]["node_limit"], 2);
    assert_eq!(structured["request"]["edge_limit"], 2);
    assert_eq!(structured["nodes"][0]["path"], "knowledge/focus.md");
    assert_eq!(structured["truncated"], true);
    assert!(
        serde_json::to_string(structured)
            .expect("structured JSON")
            .len()
            <= 4_096
    );
}

async fn register_and_index_fixture(service: &RelayKnowledgeService, repo: &FixtureRepo) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["knowledge".to_owned()],
                language_filters: Vec::new(),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-register", "trace-register"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), Vec::new())
                    .expect("selector should validate"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
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
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("relay-knowledge-{name}-{nanos}"));
        fs::create_dir_all(&path).expect("repo directory should be created");
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
