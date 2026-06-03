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
        CodeIndexMode, CodeIndexRequest, CodeIndexTaskState, CodeRepositorySelector,
        FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskCompletion, CodeRepositoryStore, SqliteGraphStore,
    },
};

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
    let completed = store
        .complete_code_index_task(CodeIndexTaskCompletion {
            task_id: running.task_id,
            lease_owner: "active-worker".to_owned(),
            attempt_count: running.attempt_count,
            now_ms: running
                .lease_expires_at_ms
                .expect("running task should have lease expiry")
                - 1,
        })
        .await
        .expect("active worker should keep its lease");
    assert_eq!(completed.state, CodeIndexTaskState::Succeeded);
}

async fn queue_index_task(
    service: &RelayKnowledgeService,
    context_name: &str,
) -> relay_knowledge::domain::CodeIndexTaskRecord {
    service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context(context_name),
        )
        .await
        .expect("cold index should queue")
        .task
        .expect("cold start should return task")
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
