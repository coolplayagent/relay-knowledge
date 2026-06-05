use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{
        CodeIndexWorkerRunRequest, CodeRepositoryFreshnessState, CodeRepositoryRegisterRequest,
        InterfaceKind, RequestContext,
    },
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeFeatureFlagRequest, CodeIndexMode, CodeIndexRequest, CodeIndexResourceBudget,
        CodeIndexSession, CodeIndexTaskState, CodeQueryKind, CodeRepositorySelector,
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

    let completed_response = service
        .run_code_index_worker_preview(
            CodeIndexWorkerRunRequest {
                task_id: Some(task.task_id.clone()),
            },
            context("run-background-index"),
        )
        .await
        .expect("worker preview should run");
    let completed = completed_response
        .task
        .as_ref()
        .expect("worker should claim task");
    let after_completed = service
        .project_status(context("status-after-background-index"))
        .await
        .expect("project status should load");

    assert_eq!(completed.task_id, task.task_id);
    assert_eq!(completed.state, CodeIndexTaskState::Succeeded);
    assert_eq!(
        completed_response.metadata.graph_version,
        after_completed.metadata.graph_version
    );
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
async fn repository_status_reports_checkpoint_without_active_task() {
    let repo = FixtureRepo::create("code-background-orphan-checkpoint");
    repo.write(
        "src/lib.rs",
        "pub fn orphan_checkpoint_policy() -> u32 { 1 }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    register_fixture_repo(&service, &repo, "register-orphan-checkpoint").await;
    let baseline = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-orphan-checkpoint-baseline"),
        )
        .await
        .expect("baseline index should queue");
    service
        .run_code_index_task_once(
            baseline.task.map(|task| task.task_id),
            context("run-orphan-checkpoint-baseline"),
        )
        .await
        .expect("baseline worker should run")
        .expect("baseline worker should complete");
    let registered = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("registered status should load")
        .expect("repository should be registered");
    let session = CodeIndexSession {
        repository_id: registered.repository_id,
        source_scope: "git_snapshot:orphan-checkpoint".to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count: 8,
        changed_path_count: 8,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        resource_budget: CodeIndexResourceBudget::default(),
    };
    store
        .begin_code_index_session(session)
        .await
        .expect("session should begin");

    let status = service
        .code_repository_status(
            selector("fixture", "HEAD"),
            context("status-orphan-checkpoint"),
        )
        .await
        .expect("status should load checkpoint");

    assert!(status.active_task.is_none());
    assert_eq!(
        status
            .checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.state.as_str()),
        Some("indexing")
    );
    assert_eq!(
        status
            .checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.source_scope.as_str()),
        Some("git_snapshot:orphan-checkpoint")
    );
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
async fn code_index_sqlite_lock_cases_shared_store_reuses_running_task() {
    let repo = FixtureRepo::create("code-shared-store-running-index");
    repo.write("src/lib.rs", "pub fn shared_store_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let database_path = unique_database_path("code-shared-store-running-index");
    let store_a = Arc::new(SqliteGraphStore::open(&database_path).expect("store a should open"));
    let store_b = Arc::new(SqliteGraphStore::open(&database_path).expect("store b should open"));
    let service_a = service_with_store(store_a.clone()).await;
    let service_b = service_with_store(store_b.clone()).await;
    register_fixture_repo(&service_a, &repo, "register-shared-store-running").await;

    let started = service_a
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-shared-store-running"),
        )
        .await
        .expect("cold index should queue");
    let task = started.task.expect("cold index should return task");
    let running = store_a
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(task.task_id.clone()),
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: task.next_retry_at_ms,
        })
        .await
        .expect("task claim should query")
        .expect("task should claim");

    let duplicate = service_b
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-shared-store-duplicate"),
        )
        .await
        .expect("duplicate start should reuse running task");
    let duplicate_task = duplicate.task.expect("duplicate should return task");
    assert_eq!(duplicate_task.task_id, running.task_id);
    assert_eq!(duplicate_task.state, CodeIndexTaskState::Running);
    assert_eq!(duplicate_task.lease_owner.as_deref(), Some("worker-a"));

    repo.write(
        "src/lib.rs",
        "pub fn shared_store_policy_v2() -> u32 { 2 }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update"]);
    let moved = service_b
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-shared-store-moved-head"),
        )
        .await
        .expect("moved head should queue a later task");
    let moved_task = moved.task.expect("moved head should return task");
    assert_ne!(moved_task.task_id, running.task_id);
    let worker_attempt = service_b
        .run_code_index_task_once(
            Some(moved_task.task_id.clone()),
            context("run-shared-store-moved-head"),
        )
        .await
        .expect("second worker should check the queued task");
    assert!(
        worker_attempt.is_none(),
        "same-repository task must wait for the live writer lease"
    );

    let status = service_b
        .code_repository_status(
            selector("fixture", "HEAD"),
            context("status-shared-store-running"),
        )
        .await
        .expect("status should load active running task");
    assert_eq!(
        status
            .active_task
            .as_ref()
            .map(|task| task.task_id.as_str()),
        Some(running.task_id.as_str())
    );
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

#[tokio::test]
async fn allow_stale_query_uses_matching_completed_scope_filters_during_active_index() {
    let repo = FixtureRepo::create("code-stale-filtered-scope");
    repo.write("src/a.rs", "pub fn stable_a_policy() -> u32 { 1 }\n");
    repo.write("src/b.rs", "pub fn stable_b_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    register_fixture_repo(&service, &repo, "register-stale-filtered-scope").await;

    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src/a.rs"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-stale-filtered-a"),
        )
        .await
        .expect("a scope should index");
    repo.write("src/b.rs", "pub fn stable_b_policy() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update-b"]);
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src/b.rs"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-stale-filtered-b"),
        )
        .await
        .expect("b scope should index");
    repo.write("src/a.rs", "pub fn stable_a_policy() -> u32 { 3 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update-a"]);
    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src/a.rs"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-stale-filtered-a"),
        )
        .await
        .expect("a refresh should queue");
    assert!(started.task.is_some());
    let active = service
        .code_repository_status(
            selector("fixture", "HEAD"),
            context("status-stale-filtered-a"),
        )
        .await
        .expect("status should load")
        .active_task
        .expect("active task should be visible");
    assert_eq!(
        active.path_filters,
        vec!["src".to_owned(), "src/a.rs".to_owned()]
    );
    let compatible = store
        .latest_code_repository_scope_status(
            "fixture".to_owned(),
            vec!["src/a.rs".to_owned()],
            Vec::new(),
        )
        .await
        .expect("latest compatible status should read")
        .expect("compatible a scope should exist");
    assert_eq!(
        compatible.path_filters,
        vec!["src".to_owned(), "src/a.rs".to_owned()]
    );

    let query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "stable_a_policy",
                filtered_selector("fixture", "HEAD", "src/a.rs"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-stale-filtered-a"),
        )
        .await
        .expect("allow-stale should use the latest compatible a scope");

    assert!(query.metadata.stale);
    assert!(query.scope.stale);
    assert!(query.results.iter().any(|hit| hit.path == "src/a.rs"));
}

#[tokio::test]
async fn allow_stale_query_rejects_unmatched_active_narrow_scope_for_unfiltered_request() {
    let repo = FixtureRepo::create("code-stale-unmatched-active-scope");
    repo.write("src/a.rs", "pub fn stable_a_policy() -> u32 { 1 }\n");
    repo.write("src/b.rs", "pub fn stable_b_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-stale-unmatched-active").await;

    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src/a.rs"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-stale-unmatched-a"),
        )
        .await
        .expect("a scope should index");
    repo.write("src/a.rs", "pub fn stable_a_policy() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update-a"]);
    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src/a.rs"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-stale-unmatched-a"),
        )
        .await
        .expect("narrow refresh should queue");
    assert!(started.task.is_some());

    let error = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "stable_b_policy",
                selector("fixture", "HEAD"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-stale-unmatched-unfiltered"),
        )
        .await
        .expect_err("unfiltered request should not use a narrow active task as stale fallback");

    assert!(error.message.contains("has no index for ref"));
    assert!(error.message.contains("requested filters"));
}

#[tokio::test]
async fn allow_stale_feature_flags_use_matching_completed_scope_filters_during_active_index() {
    let repo = FixtureRepo::create("code-stale-feature-flag-scope");
    repo.write(
        "src/a.rs",
        "pub fn stable_a_policy() -> bool { std::env::var(\"STALE_A_FLAG\").is_ok() }\n",
    );
    repo.write(
        "src/b.rs",
        "pub fn stable_b_policy() -> bool { std::env::var(\"STALE_B_FLAG\").is_ok() }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-stale-feature-flag-scope").await;

    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src/a.rs"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-stale-feature-flag-a"),
        )
        .await
        .expect("a scope should index");
    repo.write(
        "src/b.rs",
        "pub fn stable_b_policy() -> bool { std::env::var(\"STALE_B_FLAG_V2\").is_ok() }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update-b"]);
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src/b.rs"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-stale-feature-flag-b"),
        )
        .await
        .expect("b scope should index");
    repo.write(
        "src/a.rs",
        "pub fn stable_a_policy() -> bool { std::env::var(\"STALE_A_FLAG_V2\").is_ok() }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update-a"]);
    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: filtered_selector("fixture", "HEAD", "src/a.rs"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-stale-feature-flag-a"),
        )
        .await
        .expect("a refresh should queue");
    assert!(started.task.is_some());

    let flags = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                Some("STALE_A_FLAG".to_owned()),
                filtered_selector("fixture", "HEAD", "src/a.rs"),
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("feature flag request should validate"),
            context("query-stale-feature-flag-a"),
        )
        .await
        .expect("allow-stale feature flags should use the latest compatible a scope");

    assert!(flags.metadata.stale);
    assert!(flags.scope.stale);
    assert_eq!(flags.freshness.state, CodeRepositoryFreshnessState::Pending);
    assert!(flags.freshness.direct_source_read_required);
    assert_eq!(flags.freshness.direct_source_read_paths, ["src/a.rs"]);
    assert_eq!(
        flags.freshness.pending.active_task_id.as_deref(),
        started.task.as_ref().map(|task| task.task_id.as_str())
    );
    assert!(
        flags
            .flags
            .iter()
            .any(|flag| flag.source_key == "STALE_A_FLAG")
    );
    assert!(
        flags
            .flags
            .iter()
            .flat_map(|flag| flag.usages.iter())
            .all(|usage| usage.path == "src/a.rs")
    );
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

fn filtered_selector(alias: &str, ref_selector: &str, path: &str) -> CodeRepositorySelector {
    CodeRepositorySelector::new(alias, ref_selector, vec![path.to_owned()], Vec::new())
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
    let environment = test_environment();
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    RelayKnowledgeService::with_store(runtime, store)
}

#[cfg(windows)]
fn test_environment() -> EnvironmentConfig {
    EnvironmentConfig::from_pairs(
        PlatformKind::Windows,
        [
            ("USERPROFILE", "C:\\Users\\alice"),
            ("APPDATA", "C:\\Users\\alice\\AppData\\Roaming"),
            ("LOCALAPPDATA", "C:\\Users\\alice\\AppData\\Local"),
            ("TEMP", "C:\\Users\\alice\\AppData\\Local\\Temp"),
            ("RELAY_KNOWLEDGE_HOME", "C:\\relay"),
        ],
    )
    .expect("environment should parse")
}

#[cfg(not(windows))]
fn test_environment() -> EnvironmentConfig {
    EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse")
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
