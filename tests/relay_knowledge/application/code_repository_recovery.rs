use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{CodeRepositoryRegisterRequest, ErrorKind, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeIndexBatch, CodeIndexMode, CodeIndexRequest, CodeIndexResourceBudget, CodeIndexSession,
        CodeIndexTaskState, CodeParseStatus, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeFileRecord,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{
        CodeIndexPublicationStore as _, CodeIndexTaskClaimRequest, CodeIndexTaskCompletion,
        CodeIndexTaskSeed, CodeIndexTaskStore as _, RepositoryCatalogStore as _, SqliteGraphStore,
    },
};

#[tokio::test]
async fn full_index_worker_uses_the_budget_persisted_with_its_task() {
    let repo = FixtureRepo::create("code-background-durable-budget");
    repo.write("src/a.rs", "pub fn first_budgeted_file() {}\n");
    repo.write("src/b.rs", "pub fn second_budgeted_file() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    register_fixture_repo(&service, &repo, "register-durable-budget").await;
    let request = full_index_request();
    let preview = service
        .preview_code_repository_scope(request.clone(), context("preview-durable-budget"))
        .await
        .expect("scope should preview");
    let budget = CodeIndexResourceBudget::new(1, 1024 * 1024, 10_000)
        .expect("non-default budget should validate");
    let task = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: preview.preview.repository_id,
            alias: preview.preview.alias,
            ref_selector: request.repository.ref_selector.clone(),
            resolved_commit_sha: preview.preview.resolved_commit_sha,
            tree_hash: preview.preview.tree_hash,
            source_scope: preview.scope.scope_id.clone(),
            path_filters: preview.scope.path_filters,
            language_filters: preview.scope.language_filters,
            mode: request.mode.clone(),
            input_fingerprint: format!("test-durable-budget:{}", preview.scope.scope_id),
            resource_budget: budget,
            payload_json: serde_json::to_string(&request).expect("request should serialize"),
            now_ms: 1,
        })
        .await
        .expect("budgeted task should queue");

    service
        .run_code_index_task_once(Some(task.task_id), context("run-durable-budget"))
        .await
        .expect("worker should run")
        .expect("task should be claimed");
    let checkpoint = store
        .code_index_checkpoint(task.source_scope)
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");

    assert_eq!(checkpoint.resource_budget, budget);
    assert_eq!(checkpoint.batch_count, 2);
}

#[tokio::test]
async fn full_index_worker_restarts_a_retained_same_tree_scope_for_a_new_commit_alias() {
    let repo = FixtureRepo::create("code-background-retained-same-tree-restart");
    let original_source = "pub fn retained_content() -> u32 { 1 }\n";
    repo.write("src/lib.rs", original_source);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "content-a"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    register_fixture_repo(&service, &repo, "register-retained-same-tree").await;
    let first = service
        .index_code_repository(full_index_request(), context("index-content-a"))
        .await
        .expect("first content version should index");
    let retained_scope = first.summary.source_scope.clone();
    let first_commit = first.summary.resolved_commit_sha.clone();

    repo.write("src/lib.rs", "pub fn retained_content() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "content-b"]);
    let second = service
        .index_code_repository(full_index_request(), context("index-content-b"))
        .await
        .expect("second content version should index");
    assert_ne!(second.summary.source_scope, retained_scope);
    let retained_checkpoint = store
        .code_index_checkpoint(retained_scope.clone())
        .await
        .expect("retained checkpoint should load")
        .expect("the previous content scope should remain retained");
    assert_eq!(retained_checkpoint.state, "completed");

    repo.write("src/lib.rs", original_source);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "content-a-again"]);
    let preview = service
        .preview_code_repository_scope(full_index_request(), context("preview-content-a-again"))
        .await
        .expect("restored content should preview");
    assert_eq!(preview.scope.scope_id, retained_scope);
    assert_ne!(preview.preview.resolved_commit_sha, first_commit);
    let started = service
        .start_code_repository_index(full_index_request(), context("start-content-a-again"))
        .await
        .expect("restored content should queue");
    let queued = started
        .task
        .expect("the retained inactive scope still requires a worker attempt");
    assert_eq!(queued.source_scope, retained_scope);
    assert_eq!(
        queued.resolved_commit_sha,
        preview.preview.resolved_commit_sha
    );

    let completed = service
        .run_code_index_task_once(Some(queued.task_id.clone()), context("run-content-a-again"))
        .await
        .expect("content-equivalent restart should not become an internal error")
        .expect("queued content-equivalent task should claim");

    assert_eq!(completed.state, CodeIndexTaskState::Succeeded);
    let checkpoint = store
        .code_index_checkpoint(retained_scope)
        .await
        .expect("replacement checkpoint should load")
        .expect("replacement checkpoint should remain durable");
    assert_eq!(checkpoint.state, "completed");
    assert_eq!(
        checkpoint.resolved_commit_sha,
        preview.preview.resolved_commit_sha
    );
    let task = store
        .code_index_task(queued.task_id)
        .await
        .expect("completed task should load")
        .expect("completed task should remain observable");
    assert_eq!(task.state, CodeIndexTaskState::Succeeded);
    assert_ne!(task.state, CodeIndexTaskState::DeadLetter);
}

#[tokio::test]
async fn full_index_worker_rejects_a_bad_last_path_before_begin_mutates_repository_status() {
    let repo = FixtureRepo::create("code-background-corrupt-last-path");
    repo.write("src/lib.rs", "pub fn checkpoint_prefix() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    register_fixture_repo(&service, &repo, "register-corrupt-last-path").await;
    let request = full_index_request();
    let preview = service
        .preview_code_repository_scope(request.clone(), context("preview-corrupt-last-path"))
        .await
        .expect("scope should preview");
    let target_session = CodeIndexSession {
        repository_id: preview.preview.repository_id.clone(),
        source_scope: preview.scope.scope_id.clone(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: preview.preview.resolved_commit_sha.clone(),
        tree_hash: preview.preview.tree_hash.clone(),
        path_filters: preview.scope.path_filters.clone(),
        language_filters: preview.scope.language_filters.clone(),
        full_replace: true,
        total_path_count: preview.preview.selected_file_count,
        changed_path_count: preview.preview.selected_file_count,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        changed_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget: CodeIndexResourceBudget::default(),
    };
    store
        .begin_code_index_session(target_session.clone())
        .await
        .expect("target session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: target_session.repository_id.clone(),
            source_scope: target_session.source_scope.clone(),
            batch_index: 1,
            parsed_byte_count: 1,
            files: vec![RepositoryCodeFileRecord {
                repository_id: target_session.repository_id.clone(),
                source_scope: target_session.source_scope.clone(),
                file_id: "corrupt-prefix-file".to_owned(),
                path: "src/not-the-prefix.rs".to_owned(),
                language_id: "rust".to_owned(),
                blob_hash: "corrupt-prefix-blob".to_owned(),
                byte_len: 1,
                line_count: 1,
                parse_status: CodeParseStatus::Parsed,
                is_generated: false,
                degraded_reason: None,
            }],
            symbols: Vec::new(),
            references: Vec::new(),
            imports: Vec::new(),
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            framework_nodes: Vec::new(),
            framework_edges: Vec::new(),
            routes: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("corrupt prefix checkpoint should persist through public batch APIs");
    let mut published_session = target_session.clone();
    published_session.source_scope = "git_snapshot:published-baseline".to_owned();
    published_session.resolved_commit_sha = "published-baseline-commit".to_owned();
    published_session.tree_hash = "published-baseline-tree".to_owned();
    published_session.total_path_count = 0;
    published_session.changed_path_count = 0;
    store
        .begin_code_index_session(published_session.clone())
        .await
        .expect("unrelated baseline session should begin");
    store
        .finalize_code_index_session(published_session)
        .await
        .expect("unrelated baseline should publish a fresh repository status");
    let task = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: target_session.repository_id.clone(),
            alias: preview.preview.alias,
            ref_selector: request.repository.ref_selector.clone(),
            resolved_commit_sha: target_session.resolved_commit_sha.clone(),
            tree_hash: target_session.tree_hash.clone(),
            source_scope: target_session.source_scope.clone(),
            path_filters: target_session.path_filters.clone(),
            language_filters: target_session.language_filters.clone(),
            mode: request.mode.clone(),
            input_fingerprint: format!("test-corrupt-prefix:{}", target_session.source_scope),
            resource_budget: target_session.resource_budget,
            payload_json: serde_json::to_string(&request).expect("request should serialize"),
            now_ms: 1,
        })
        .await
        .expect("target task should queue");
    let status_before = store
        .code_repository_status(target_session.repository_id.clone())
        .await
        .expect("repository status should load")
        .expect("repository should exist");
    assert_ne!(status_before.state, "indexing");
    assert!(!status_before.stale);

    let error = service
        .run_code_index_task_once(Some(task.task_id.clone()), context("run-corrupt-last-path"))
        .await
        .expect_err("bad checkpoint prefix should fail internally");

    assert_eq!(error.error_kind, ErrorKind::Internal);
    let status = store
        .code_repository_status(target_session.repository_id.clone())
        .await
        .expect("repository status should load")
        .expect("repository should exist");
    assert_eq!(status.state, status_before.state);
    assert_eq!(status.stale, status_before.stale);
    assert_eq!(
        status.last_indexed_scope_id,
        status_before.last_indexed_scope_id
    );
    let checkpoint = store
        .code_index_checkpoint(target_session.source_scope)
        .await
        .expect("checkpoint should load")
        .expect("corrupt checkpoint should remain observable");
    assert_eq!(
        checkpoint.last_path.as_deref(),
        Some("src/not-the-prefix.rs")
    );
    let failed_task = store
        .code_index_task(task.task_id)
        .await
        .expect("failed task should load")
        .expect("failed task should remain observable");
    assert_eq!(failed_task.state, CodeIndexTaskState::DeadLetter);
    assert_eq!(failed_task.attempt_count, 1);
    assert_eq!(
        failed_task.last_error_kind.as_deref(),
        Some("checkpoint_invariant")
    );
}

#[tokio::test]
async fn startup_recovery_requeues_expired_code_index_task_before_status_poll() {
    let repo = FixtureRepo::create("code-background-startup-recovery");
    repo.write(
        "src/lib.rs",
        "pub fn startup_recovered_index() -> u32 { 1 }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    register_fixture_repo(&service, &repo, "register-startup-recovery").await;
    let task = queue_index_task(&service, "start-startup-recovery").await;
    store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(task.task_id.clone()),
            lease_owner: "dead-service-worker".to_owned(),
            lease_duration_ms: 1,
            max_attempts: 3,
            now_ms: task.next_retry_at_ms,
        })
        .await
        .expect("task should claim")
        .expect("task should be running");
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    service
        .reconcile_startup_code_index_tasks()
        .await
        .expect("startup recovery should run");
    let recovered = store
        .code_index_task(task.task_id.clone())
        .await
        .expect("task should load")
        .expect("task should exist");

    assert_eq!(recovered.state, CodeIndexTaskState::Retrying);
    assert!(recovered.lease_owner.is_none());
    assert_eq!(recovered.last_error_kind.as_deref(), Some("lease_expired"));

    let completed = service
        .run_code_index_task_once(Some(task.task_id), context("run-startup-recovery"))
        .await
        .expect("worker should run")
        .expect("recovered task should claim");
    assert_eq!(completed.state, CodeIndexTaskState::Succeeded);
}

#[tokio::test]
async fn repository_index_reset_clears_stuck_task_lease() {
    let repo = FixtureRepo::create("code-background-reset");
    repo.write("src/lib.rs", "pub fn reset_stuck_index() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    register_fixture_repo(&service, &repo, "register-reset").await;
    let task = queue_index_task(&service, "start-reset").await;
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(task.task_id),
            lease_owner: "old-worker".to_owned(),
            lease_duration_ms: 1,
            max_attempts: 3,
            now_ms: task.next_retry_at_ms,
        })
        .await
        .expect("task should claim")
        .expect("task should be running");
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    let reset = service
        .reset_code_repository_index_tasks("fixture".to_owned(), context("reset-index-task"))
        .await
        .expect("reset should succeed");

    assert_eq!(reset.reset_task_count, 1);
    assert_eq!(reset.reset_tasks[0].task_id, running.task_id);
    assert_eq!(reset.reset_tasks[0].state, CodeIndexTaskState::Queued);
    assert!(reset.reset_tasks[0].lease_owner.is_none());
    assert_eq!(
        reset.active_task.as_ref().map(|task| task.task_id.as_str()),
        Some(running.task_id.as_str())
    );
    let stale_complete = store
        .complete_code_index_task(CodeIndexTaskCompletion {
            task_id: running.task_id.clone(),
            lease_owner: "old-worker".to_owned(),
            attempt_count: running.attempt_count,
            publication_generation: running.publication_generation,
            now_ms: running
                .lease_expires_at_ms
                .expect("running task should have lease expiry")
                - 1,
        })
        .await
        .expect_err("old worker should not complete reset task");
    assert!(stale_complete.to_string().contains("active lease"));

    let completed = service
        .run_code_index_task_once(Some(running.task_id), context("run-reset-task"))
        .await
        .expect("worker should run")
        .expect("reset task should claim");
    assert_eq!(completed.state, CodeIndexTaskState::Succeeded);
}

#[tokio::test]
async fn repository_index_reset_preserves_live_running_task_lease() {
    let repo = FixtureRepo::create("code-background-live-reset");
    repo.write("src/lib.rs", "pub fn live_index_lease() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    register_fixture_repo(&service, &repo, "register-live-reset").await;
    let task = queue_index_task(&service, "start-live-reset").await;
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(task.task_id),
            lease_owner: "active-worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: task.next_retry_at_ms,
        })
        .await
        .expect("task should claim")
        .expect("task should be running");

    let reset = service
        .reset_code_repository_index_tasks("fixture".to_owned(), context("reset-live-index-task"))
        .await
        .expect("reset should succeed");

    assert_eq!(reset.reset_task_count, 0);
    assert_eq!(
        reset.active_task.as_ref().map(|task| task.task_id.as_str()),
        Some(running.task_id.as_str())
    );
    assert_eq!(
        reset
            .active_task
            .as_ref()
            .and_then(|task| task.lease_owner.as_deref()),
        Some("active-worker")
    );
    let preserved = store
        .code_index_task(running.task_id)
        .await
        .expect("preserved task should load")
        .expect("preserved task should still exist");
    assert_eq!(preserved.state, CodeIndexTaskState::Running);
    assert_eq!(preserved.lease_owner.as_deref(), Some("active-worker"));
    assert_eq!(preserved.attempt_count, running.attempt_count);
    assert_eq!(preserved.lease_expires_at_ms, running.lease_expires_at_ms);
}

async fn queue_index_task(
    service: &RelayKnowledgeService,
    context_name: &str,
) -> relay_knowledge::domain::CodeIndexTaskRecord {
    service
        .start_code_repository_index(full_index_request(), context(context_name))
        .await
        .expect("cold index should queue")
        .task
        .expect("cold start should return task")
}

fn full_index_request() -> CodeIndexRequest {
    CodeIndexRequest {
        repository: selector("fixture", "HEAD"),
        mode: CodeIndexMode::Full,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::AllowStale,
        reuse_historical: false,
    }
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

async fn service_with_store(store: Arc<SqliteGraphStore>) -> RelayKnowledgeService {
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
}

fn git_command<const N: usize>(path: &Path, args: [&str; N]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    command
}
