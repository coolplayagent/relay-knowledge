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
        CodeIndexMode, CodeIndexRequest, CodeIndexTaskState, CodeQueryKind, CodeRepositorySelector,
        CodeRetrievalRequest, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn cold_full_index_start_queues_task_and_worker_completes_it() {
    let repo = FixtureRepo::create("code-background-index");
    repo.write("src/lib.rs", "pub fn background_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let initial_commit = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-background-index").await;
    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-background-index"),
        )
        .await
        .expect("cold index should queue");
    let duplicate = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-background-index-duplicate"),
        )
        .await
        .expect("duplicate cold index should reuse queued task");

    let task = started.task.expect("cold start should return a task");
    assert!(started.summary.is_none());
    assert_eq!(
        duplicate.task.as_ref().map(|task| task.task_id.as_str()),
        Some(task.task_id.as_str())
    );
    repo.write(
        "src/lib.rs",
        "pub fn later_background_policy() -> u32 { 2 }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "move-head"]);
    let moved_commit = repo.git_text(["rev-parse", "HEAD"]);
    let moved = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-background-index-moved-head"),
        )
        .await
        .expect("moved ref should queue a distinct task");
    let moved_task = moved.task.expect("moved ref should return a task");
    assert_ne!(moved_task.task_id, task.task_id);
    assert_eq!(moved_task.resolved_commit_sha, moved_commit);

    let completed = service
        .run_code_index_task_once(Some(task.task_id.clone()), context("run-background-index"))
        .await
        .expect("worker should run")
        .expect("worker should claim task");

    assert_eq!(completed.task_id, task.task_id);
    assert_eq!(completed.state, CodeIndexTaskState::Succeeded);
    let hits = query_ref(
        &service,
        "background_policy",
        &initial_commit,
        CodeQueryKind::Definition,
    )
    .await;
    assert!(hits.results.iter().any(|hit| hit.path == "src/lib.rs"));
    service
        .run_code_index_task_once(
            Some(moved_task.task_id.clone()),
            context("run-background-index-moved-head"),
        )
        .await
        .expect("moved worker should run")
        .expect("moved worker should claim task");
    let moved_hits = query(
        &service,
        "later_background_policy",
        CodeQueryKind::Definition,
    )
    .await;
    assert!(
        moved_hits
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
    let status = service
        .code_repository_status(
            selector("fixture", "HEAD"),
            context("status-background-index"),
        )
        .await
        .expect("status should load");
    assert!(status.active_task.is_none());
    assert_eq!(
        status.checkpoint.as_ref().map(|c| c.committed_file_count),
        Some(1)
    );
}

#[tokio::test]
async fn background_index_prunes_scopes_beyond_active_and_recent_budget() {
    let repo = FixtureRepo::create("code-background-retention");
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-background-retention").await;
    let mut commits = Vec::new();
    for version in 1..=3 {
        repo.write(
            "src/lib.rs",
            &format!("pub fn retention_policy_v{version}() -> u32 {{ {version} }}\n"),
        );
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "retention"]);
        let commit = repo.git_text(["rev-parse", "HEAD"]);
        commits.push(commit.clone());
        let started = service
            .start_code_repository_index(
                CodeIndexRequest {
                    repository: selector("fixture", &commit),
                    mode: CodeIndexMode::Full,
                    freshness_policy: FreshnessPolicy::AllowStale,
                },
                context("start-retention"),
            )
            .await
            .expect("index should queue");
        let task_id = started.task.expect("task").task_id;
        service
            .run_code_index_task_once(Some(task_id), context("run-retention"))
            .await
            .expect("worker should run")
            .expect("worker should complete");
    }

    let status = service
        .code_repository_status(
            selector("fixture", commits.last().expect("latest commit")),
            context("status-retention"),
        )
        .await
        .expect("status should load");

    assert_eq!(status.retention.prunable_scope_count, 0);
    assert!(status.retention.retained_scope_count <= 2);
    let old = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "retention_policy_v1",
                selector("fixture", commits.first().expect("first commit")),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-pruned-retention"),
        )
        .await
        .expect_err("oldest pruned scope should no longer query");

    assert!(old.message.contains("no index for ref"));
}

async fn query(
    service: &RelayKnowledgeService,
    query: &str,
    kind: CodeQueryKind,
) -> relay_knowledge::api::CodeRepositoryQueryResponse {
    query_ref(service, query, "HEAD", kind).await
}

async fn query_ref(
    service: &RelayKnowledgeService,
    query: &str,
    ref_selector: &str,
    kind: CodeQueryKind,
) -> relay_knowledge::api::CodeRepositoryQueryResponse {
    service
        .query_code_repository(
            CodeRetrievalRequest::new(
                query,
                selector("fixture", ref_selector),
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

async fn register_fixture_repo(service: &RelayKnowledgeService, repo: &FixtureRepo, name: &str) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
            },
            context(name),
        )
        .await
        .expect("repository should register");
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
