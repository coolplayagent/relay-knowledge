use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeIndexTaskState, CodeQueryKind, CodeRepositorySelector,
        CodeRetrievalRequest, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{CodeIndexTaskClaimRequest, CodeRepositoryStore, KnowledgeStore, SqliteGraphStore},
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

#[tokio::test]
async fn repository_status_recovers_expired_code_index_task_lease() {
    let repo = FixtureRepo::create("code-background-expired-lease");
    repo.write("src/lib.rs", "pub fn recover_stuck_index() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    register_fixture_repo(&service, &repo, "register-expired-lease").await;
    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-expired-lease"),
        )
        .await
        .expect("cold index should queue");
    let task = started.task.expect("cold start should return task");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(task.task_id.clone()),
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 1,
            max_attempts: 3,
            now_ms: task.next_retry_at_ms,
        })
        .await
        .expect("task should claim")
        .expect("task should be running");
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    let status = service
        .code_repository_status(selector("fixture", "HEAD"), context("status-expired-lease"))
        .await
        .expect("status should recover expired task");
    let active = status
        .active_task
        .expect("recovered task should remain active");

    assert_eq!(active.task_id, running.task_id);
    assert_eq!(active.state, CodeIndexTaskState::Retrying);
    assert!(active.lease_owner.is_none());
    assert_eq!(active.last_error_kind.as_deref(), Some("lease_expired"));
    assert!(status.checkpoint.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_index_health_isolation_cases_health_and_query_respond_during_full_index() {
    let repo = FixtureRepo::create("code-health-isolation");
    repo.write("src/lib.rs", "pub fn stable_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_file_store("code-health-isolation").await;

    register_fixture_repo_without_language_filter(&service, &repo, "register-health-isolation")
        .await;
    let initial = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-health-isolation-initial"),
        )
        .await
        .expect("initial index should queue");
    service
        .run_code_index_task_once(
            initial.task.map(|task| task.task_id),
            context("run-health-isolation-initial"),
        )
        .await
        .expect("initial worker should run");

    for index in 0..260 {
        repo.write(
            &format!("src/generated/module_{index:03}.rs"),
            &format!(
                "pub fn generated_policy_{index:03}() -> u32 {{ {index} }}\n\
                 pub fn generated_caller_{index:03}() -> u32 {{ generated_policy_{index:03}() }}\n"
            ),
        );
    }
    repo.write(
        "src/lib.rs",
        "pub fn stable_policy() -> u32 { 2 }\npub fn stable_policy_caller() -> u32 { stable_policy() }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "large-index"]);
    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-health-isolation-large"),
        )
        .await
        .expect("large index should queue");
    let task_id = started
        .task
        .expect("large index should return task")
        .task_id;
    let worker_service = service.clone();
    let worker = tokio::spawn(async move {
        worker_service
            .run_code_index_task_once(Some(task_id), context("run-health-isolation-large"))
            .await
    });

    let health = tokio::time::timeout(
        Duration::from_secs(2),
        service.health(context("health-while-indexing")),
    )
    .await
    .expect("health should not hang during indexing")
    .expect("health should return a response");
    assert_eq!(health.metadata.request_id, "req-health-while-indexing");

    let query = tokio::time::timeout(
        Duration::from_secs(2),
        service.query_code_repository(
            CodeRetrievalRequest::new(
                "stable_policy",
                selector("fixture", "HEAD"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-while-indexing"),
        ),
    )
    .await
    .expect("query should not hang during indexing")
    .expect("query should read a committed scope");
    assert!(query.results.iter().any(|hit| hit.path == "src/lib.rs"));

    worker
        .await
        .expect("worker task should join")
        .expect("worker should complete");
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
                language_filters: Vec::new(),
            },
            context(name),
        )
        .await
        .expect("repository should register");
}

async fn register_fixture_repo_without_language_filter(
    service: &RelayKnowledgeService,
    repo: &FixtureRepo,
    name: &str,
) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
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
    service_with_store(Arc::new(
        SqliteGraphStore::open_in_memory().expect("store should open"),
    ))
    .await
}

async fn service_with_file_store(name: &str) -> RelayKnowledgeService {
    let path = unique_database_path(name);
    service_with_store(Arc::new(
        SqliteGraphStore::open(path).expect("file store should open"),
    ))
    .await
}

async fn service_with_store(store: Arc<dyn KnowledgeStore>) -> RelayKnowledgeService {
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

fn unique_database_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    std::env::temp_dir()
        .join("relay-knowledge-tests")
        .join(format!("{name}-{}-{nanos}.sqlite", std::process::id()))
}
