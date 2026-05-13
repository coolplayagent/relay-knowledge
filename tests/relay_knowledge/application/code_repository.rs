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
    let doc_comment = query(&service, "budget", CodeQueryKind::Definition).await;
    let references = query(&service, "retry_policy", CodeQueryKind::References).await;
    let imports = query(&service, "crate::retry_policy", CodeQueryKind::Imports).await;

    assert!(
        definitions
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
    assert!(
        doc_comment
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
    assert_eq!(doc_comment.scope.resolved_commit_sha, first_commit.as_str());
    assert_eq!(doc_comment.scope.path_filters, ["src"]);
    assert!(
        doc_comment
            .scope
            .index_versions
            .iter()
            .any(|version| version.starts_with("code:"))
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

    repo.write("src/late.rs", "pub fn late_change() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "late change"]);
    let stale_head_error = service
        .impact_code_repository(
            CodeImpactRequest::new(
                selector("fixture", &updated.summary.resolved_commit_sha),
                &updated.summary.resolved_commit_sha,
                "HEAD",
                10,
            )
            .expect("impact request should validate"),
            context("impact-stale-head"),
        )
        .await
        .expect_err("impact head must match indexed snapshot");

    assert!(stale_head_error.message.contains("impact head ref"));
}

#[tokio::test]
async fn incremental_index_rejects_base_that_is_not_current_snapshot() {
    let repo = FixtureRepo::create("code-incremental-base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let initial = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
            },
            context("register-incremental-base"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-incremental-base"),
        )
        .await
        .expect("initial index should succeed");

    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update to one"]);
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::incremental(initial.clone(), "HEAD")
                    .expect("incremental mode should validate"),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-current-base"),
        )
        .await
        .expect("first incremental index should succeed");

    repo.write("src/lib.rs", "pub fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "return to zero"]);
    let error = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::incremental(initial, "HEAD")
                    .expect("incremental mode should validate"),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-stale-base"),
        )
        .await
        .expect_err("stale incremental base should be rejected");

    assert!(error.message.contains("incremental base ref"));
}

#[tokio::test]
async fn callee_query_does_not_reuse_caller_symbol_identity_for_unresolved_edges() {
    let repo = FixtureRepo::create("code-unresolved-callee");
    repo.write(
        "src/lib.rs",
        r#"
pub fn caller_missing() {
    missing_dependency();
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
                language_filters: vec!["rust".to_owned()],
            },
            context("register-unresolved-callee"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-unresolved-callee"),
        )
        .await
        .expect("repository should index");

    let response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "caller_missing",
                selector("fixture", "HEAD"),
                CodeQueryKind::Callees,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-unresolved-callee"),
        )
        .await
        .expect("callee query should succeed");
    let hit = response
        .results
        .iter()
        .find(|hit| hit.excerpt.contains("missing_dependency"))
        .expect("unresolved callee edge should be returned");

    assert_eq!(hit.symbol_snapshot_id, None);
}

#[tokio::test]
async fn worktree_overlay_requires_explicit_worktree_ref_for_queries() {
    let repo = FixtureRepo::create("code-overlay");
    repo.write("src/lib.rs", "pub fn retry_policy() -> u32 { 3 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
            },
            context("register-overlay"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-overlay"),
        )
        .await
        .expect("clean repository should index");

    repo.write(
        "src/lib.rs",
        "pub fn retry_policy() -> u32 { 5 }\npub fn retry_policy_v2() -> u32 { retry_policy() }\n",
    );
    let overlay = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::WorktreeOverlay,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("overlay"),
        )
        .await
        .expect("worktree overlay should index");

    assert!(overlay.summary.resolved_commit_sha.starts_with("worktree:"));
    let clean_error = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "retry_policy_v2",
                selector("fixture", "HEAD"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-clean-overlay"),
        )
        .await
        .expect_err("clean commit query should not read overlay rows");
    assert!(clean_error.message.contains("indexed at worktree:"));

    let overlay_query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "retry_policy_v2",
                selector("fixture", "worktree"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-overlay"),
        )
        .await
        .expect("explicit worktree query should succeed");
    assert!(
        overlay_query
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
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
