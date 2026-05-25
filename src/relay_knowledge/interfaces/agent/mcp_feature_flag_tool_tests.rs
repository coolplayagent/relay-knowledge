use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use super::*;
use crate::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{CodeIndexMode, CodeIndexRequest, CodeRepositorySelector, FreshnessPolicy},
    env::{EnvironmentConfig, PlatformKind},
    interfaces::agent::AgentAuditStatus,
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn code_feature_flags_tool_routes_through_mcp_dispatcher() {
    let repo = FixtureRepo::create("mcp-code-feature-flags");
    repo.write(
        "src/lib.rs",
        r#"
pub fn checkout_enabled() -> bool {
    std::env::var("CHECKOUT_V2").is_ok()
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let (server, service) =
        server_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "fixture")]).await;
    register_and_index_fixture(&service, &repo, "fixture").await;

    let outcome = run_cancellable_tool_call(
        &server,
        ToolCallParams {
            name: CODE_FEATURE_FLAGS_TOOL.to_owned(),
            arguments: json!({
                "repository": "fixture",
                "query": "CHECKOUT",
                "limit": 5,
                "freshness": "wait-until-fresh"
            }),
        },
        "code-feature-flags".to_owned(),
    )
    .await;
    record_mcp_tool_audit(
        &server,
        &outcome.operation,
        &outcome.request_id,
        &outcome.result,
        outcome.duration_ms,
    )
    .await;

    let structured = &outcome.result["structuredContent"];
    assert_eq!(outcome.result["isError"], false);
    assert_eq!(structured["flags"][0]["source_key"], "CHECKOUT_V2");

    let audit = server.audit_snapshot();
    let event = audit.last().expect("tool call should write audit event");
    assert_eq!(event.operation, "relay_code_feature_flags");
    assert_eq!(event.source_scope.as_deref(), Some("fixture"));
    assert_eq!(event.result_count, Some(1));
    assert_eq!(event.status, AgentAuditStatus::Completed);
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
                language_filters: vec!["rust".to_owned()],
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
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-index", "trace-index"),
        )
        .await
        .expect("repository should index");
}

async fn server_and_service<const N: usize>(
    pairs: [(&str, &str); N],
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
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = RelayKnowledgeService::with_store(runtime.clone(), store);
    let server = McpServer::new(
        service.clone(),
        runtime.network.clone(),
        runtime.agent.clone(),
    );

    (server, service)
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
