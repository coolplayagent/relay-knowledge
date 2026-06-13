use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{
        CodeRepositoryFreshnessState, CodeRepositoryRegisterRequest, InterfaceKind, RequestContext,
    },
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeGraphContextRequest, CodeIndexMode, CodeIndexRequest, CodeRepositorySelector,
        CodeRetrievalLayer, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn codegraph_context_packs_entry_points_edges_excerpts_and_budget() {
    let repo = FixtureRepo::create("codegraph-context");
    repo.write(
        "src/lib.rs",
        r#"
pub fn retry_policy() -> u32 {
    3
}

pub fn retry_policy_v2() -> u32 {
    retry_policy()
}
"#,
    );
    repo.write(
        "src/main.rs",
        r#"
use crate::retry_policy;

fn run_worker() {
    retry_policy();
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-context"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-context"),
        )
        .await
        .expect("repository should index");

    let response = service
        .codegraph_context(
            CodeGraphContextRequest::new(
                selector("fixture", "HEAD"),
                "retry_policy",
                5,
                FreshnessPolicy::AllowStale,
                16_384,
                true,
                false,
            )
            .expect("context request should validate"),
            context("context"),
        )
        .await
        .expect("context should build");

    assert_eq!(response.repository_scope.alias, "fixture");
    assert_eq!(
        response.freshness.state,
        CodeRepositoryFreshnessState::Fresh
    );
    assert!(response.budget.context_bytes <= response.budget.max_context_bytes);
    assert!(response.budget.candidate_count >= response.pack.entry_points.len());
    assert!(response.retrieval_layers.iter().any(|layer| matches!(
        layer,
        CodeRetrievalLayer::Definition | CodeRetrievalLayer::Symbol
    )));
    assert!(
        response
            .pack
            .entry_points
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
    assert!(
        !response.pack.related_symbols.is_empty() || !response.pack.graph_paths.is_empty(),
        "context should include structural expansion evidence: {:?}",
        response.pack
    );
    assert!(
        response
            .pack
            .code_excerpts
            .iter()
            .any(|excerpt| excerpt.excerpt.contains("retry_policy"))
    );

    let no_code = service
        .codegraph_context(
            CodeGraphContextRequest::new(
                selector("fixture", "HEAD"),
                "retry_policy",
                5,
                FreshnessPolicy::AllowStale,
                16_384,
                false,
                false,
            )
            .expect("context request should validate"),
            context("context-no-code"),
        )
        .await
        .expect("context should build without code excerpts");

    assert!(no_code.pack.code_excerpts.is_empty());
    assert!(
        no_code
            .pack
            .entry_points
            .iter()
            .chain(&no_code.pack.related_symbols)
            .chain(&no_code.pack.graph_paths)
            .all(|hit| hit.excerpt.is_empty())
    );
}

async fn service_with_memory_store() -> RelayKnowledgeService {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    RelayKnowledgeService::with_store(
        runtime,
        Arc::new(SqliteGraphStore::open_in_memory().expect("store should open")),
    )
}

fn selector(alias: &str, ref_selector: &str) -> CodeRepositorySelector {
    CodeRepositorySelector::new(alias, ref_selector, Vec::new(), Vec::new())
        .expect("selector should validate")
}

fn context(name: &str) -> RequestContext {
    RequestContext::with_ids(
        InterfaceKind::Cli,
        format!("req-{name}"),
        format!("trace-{name}"),
    )
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
        let status = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .status()
            .expect("git command should run");
        assert!(status.success(), "git command should succeed");
    }
}
