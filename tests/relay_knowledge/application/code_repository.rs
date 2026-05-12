use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeImpactRequest, CodeIndexMode, CodeIndexRequest, CodeQueryKind, CodeRepositorySelector,
        CodeRetrievalRequest, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn indexes_tree_sitter_repository_and_queries_code_graph() {
    let repo = FixtureRepo::create("code-retrieval");
    repo.write(
        "src/lib.rs",
        r#"
/// Selects the retry budget.
pub fn retry_policy() -> u32 {
    3
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
    let first_commit = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
            },
            context("register"),
        )
        .await
        .expect("repository should register");
    let indexed = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index"),
        )
        .await
        .expect("repository should index");

    assert_eq!(indexed.summary.indexed_file_count, 2);
    assert!(indexed.summary.symbol_count >= 2);

    let definitions = query(&service, "retry_policy", CodeQueryKind::Definition).await;
    let references = query(&service, "retry_policy", CodeQueryKind::References).await;
    let imports = query(&service, "crate::retry_policy", CodeQueryKind::Imports).await;

    assert!(
        definitions
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
    assert!(
        references
            .results
            .iter()
            .any(|hit| hit.path == "src/main.rs")
    );
    assert!(imports.results.iter().any(|hit| hit.path == "src/main.rs"));

    repo.write(
        "src/lib.rs",
        r#"
/// Selects the retry budget.
pub fn retry_policy() -> u32 {
    5
}

pub fn retry_policy_v2() -> u32 {
    retry_policy()
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update policy"]);
    let updated = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::incremental(first_commit, "HEAD")
                    .expect("incremental mode should validate"),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("update"),
        )
        .await
        .expect("incremental update should index");

    assert_eq!(updated.summary.changed_path_count, 1);
    assert!(updated.summary.indexed_file_count >= 2);

    let v2 = query(&service, "retry_policy_v2", CodeQueryKind::Definition).await;
    assert!(v2.results.iter().any(|hit| hit.path == "src/lib.rs"));

    let impact = service
        .impact_code_repository(
            CodeImpactRequest::new(selector("fixture", "HEAD"), "HEAD~1", "HEAD", 10)
                .expect("impact request should validate"),
            context("impact"),
        )
        .await
        .expect("impact should succeed");

    assert!(impact.changed_paths.iter().any(|path| path == "src/lib.rs"));
    assert!(impact.results.iter().any(|hit| hit.path == "src/lib.rs"));
}

async fn query(
    service: &RelayKnowledgeService,
    query: &str,
    kind: CodeQueryKind,
) -> relay_knowledge::api::CodeRepositoryQueryResponse {
    service
        .query_code_repository(
            CodeRetrievalRequest::new(
                query,
                selector("fixture", "HEAD"),
                kind,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query"),
        )
        .await
        .expect("query should succeed")
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
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    RelayKnowledgeService::with_store(runtime, store)
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

fn git_command<const N: usize>(path: &Path, args: [&str; N]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    command
}
