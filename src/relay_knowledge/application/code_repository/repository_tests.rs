use std::{fs, sync::Arc};

use crate::{
    api::{
        CodeRepositoryFreshnessDiagnostics, CodeRepositoryFreshnessState, CodeRepositoryIndexLag,
        CodeRepositoryPendingIndexWork, CodeRepositoryRegisterRequest,
    },
    code::{reset_tracked_entries_call_count_for_root, tracked_entries_call_count_for_root},
    domain::{
        CodeFeatureFlagRequest, CodeImpactRequest, CodeIndexMode, CodeIndexRequest,
        CodeIndexResourceBudget, CodeIndexSession, CodeQueryKind, CodeRepositorySelector,
        CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest, FreshnessPolicy,
        RepositoryCodeRange, SoftwareGlobalKind, SoftwareGlobalRequest, StalenessHint,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

use super::repository_test_support::*;

static TRACKED_ENTRIES_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn git_fresh_full_index_skips_tracked_entry_plan_build() {
    let _guard = TRACKED_ENTRIES_TEST_LOCK.lock().await;
    let repo = FixtureRepo::create("git-full-noop-fast-path");
    repo.write("src/lib.rs", "pub fn stable_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;
    let observed_root = repo
        .path
        .canonicalize()
        .expect("repo path should canonicalize");

    register_fixture_repo(&service, &repo, "register-git-full-noop-fast-path").await;
    reset_tracked_entries_call_count_for_root(observed_root.clone());
    let first = service
        .index_code_repository(request("fixture", "HEAD"), context("index-git-full-first"))
        .await
        .expect("initial full index should succeed");
    assert!(
        tracked_entries_call_count_for_root(&observed_root) > 0,
        "cold full index should enumerate tracked entries"
    );

    reset_tracked_entries_call_count_for_root(observed_root.clone());
    let second = service
        .index_code_repository(request("fixture", "HEAD"), context("index-git-full-second"))
        .await
        .expect("fresh full index should reuse scope");

    assert_eq!(second.summary.source_scope, first.summary.source_scope);
    assert_eq!(second.summary.progress.blob_read_count, 0);
    assert_eq!(tracked_entries_call_count_for_root(&observed_root), 0);
}

#[tokio::test]
async fn git_fresh_filtered_full_index_uses_scope_generated_symbol_counts() {
    let _guard = TRACKED_ENTRIES_TEST_LOCK.lock().await;
    let repo = FixtureRepo::create("git-filtered-fast-path-generated-counts");
    repo.write("src/narrow/lib.rs", "pub fn narrow_policy() -> u32 { 1 }\n");
    repo.write(
        "src/generated/api.pb.go",
        "package generated\nfunc GeneratedPolicy() int { return 1 }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;
    let observed_root = repo
        .path
        .canonicalize()
        .expect("repo path should canonicalize");
    let narrow_request = CodeIndexRequest {
        repository: CodeRepositorySelector::new(
            "fixture",
            "HEAD",
            vec!["src/narrow".to_owned()],
            Vec::new(),
        )
        .expect("selector should validate"),
        mode: CodeIndexMode::Full,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
    };

    register_fixture_repo(&service, &repo, "register-git-filtered-counts").await;
    let narrow = service
        .index_code_repository(
            narrow_request.clone(),
            context("index-filtered-counts-narrow"),
        )
        .await
        .expect("narrow index should succeed");
    service
        .index_code_repository(
            request("fixture", "HEAD"),
            context("index-filtered-counts-broad"),
        )
        .await
        .expect("broad index should succeed");

    reset_tracked_entries_call_count_for_root(observed_root.clone());
    let reused_narrow = service
        .index_code_repository(narrow_request, context("index-filtered-counts-reuse"))
        .await
        .expect("fresh narrow index should reuse scoped status");

    assert_eq!(
        reused_narrow.summary.source_scope,
        narrow.summary.source_scope
    );
    assert_eq!(reused_narrow.summary.progress.blob_read_count, 0);
    assert_eq!(reused_narrow.summary.handwritten_symbol_count, 1);
    assert_eq!(reused_narrow.summary.generated_symbol_count, 0);
    assert_eq!(tracked_entries_call_count_for_root(&observed_root), 0);
}

#[tokio::test]
async fn duplicate_active_full_index_start_skips_tracked_entry_plan_build() {
    let _guard = TRACKED_ENTRIES_TEST_LOCK.lock().await;
    let repo = FixtureRepo::create("git-active-duplicate-fast-path");
    repo.write("src/lib.rs", "pub fn queued_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;
    let observed_root = repo
        .path
        .canonicalize()
        .expect("repo path should canonicalize");
    let request = CodeIndexRequest {
        repository: selector("fixture", "HEAD"),
        mode: CodeIndexMode::Full,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::AllowStale,
    };

    register_fixture_repo(&service, &repo, "register-git-active-duplicate-fast-path").await;
    reset_tracked_entries_call_count_for_root(observed_root.clone());
    let first = service
        .start_code_repository_index(request.clone(), context("start-git-active-first"))
        .await
        .expect("cold full index should queue");

    reset_tracked_entries_call_count_for_root(observed_root.clone());
    let duplicate = service
        .start_code_repository_index(request.clone(), context("start-git-active-duplicate"))
        .await
        .expect("duplicate full index should reuse queued task");

    assert_eq!(
        duplicate.task.as_ref().map(|task| task.task_id.as_str()),
        first.task.as_ref().map(|task| task.task_id.as_str())
    );
    assert_eq!(tracked_entries_call_count_for_root(&observed_root), 0);

    let workspace_request = CodeIndexRequest {
        workspace_detection: crate::domain::CodeWorkspaceDetectionConfig::enabled_all(),
        ..request.clone()
    };
    reset_tracked_entries_call_count_for_root(observed_root.clone());
    let workspace_distinct = service
        .start_code_repository_index(workspace_request, context("start-git-active-workspace"))
        .await
        .expect("workspace-aware full index request should queue separately");
    assert_ne!(
        workspace_distinct
            .task
            .as_ref()
            .map(|task| task.task_id.as_str()),
        first.task.as_ref().map(|task| task.task_id.as_str())
    );
    assert!(
        tracked_entries_call_count_for_root(&observed_root) > 0,
        "workspace-detection changes should still build a plan"
    );

    let distinct_request = CodeIndexRequest {
        repository: CodeRepositorySelector::new(
            "fixture",
            "HEAD",
            vec!["src".to_owned()],
            Vec::new(),
        )
        .expect("selector should validate"),
        mode: CodeIndexMode::Full,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::AllowStale,
    };
    reset_tracked_entries_call_count_for_root(observed_root.clone());
    service
        .start_code_repository_index(distinct_request, context("start-git-active-distinct"))
        .await
        .expect("distinct full index request should build a plan");
    assert!(
        tracked_entries_call_count_for_root(&observed_root) > 0,
        "non-identical active full-index starts should still build a plan"
    );
}

#[tokio::test]
async fn duplicate_active_filesystem_full_index_resolves_live_snapshot_before_reuse() {
    let source = FixtureSourceDir::create("filesystem-active-duplicate-current-ref");
    source.write("src/lib.rs", "pub fn queued_policy() -> u32 { 1 }\n");
    let service = service_with_memory_store().await;
    let request = CodeIndexRequest {
        repository: selector("fixture", "HEAD"),
        mode: CodeIndexMode::Full,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::AllowStale,
    };

    register_fixture_source(&service, &source, "register-filesystem-active-current-ref").await;
    let first = service
        .start_code_repository_index(request.clone(), context("start-filesystem-active-first"))
        .await
        .expect("cold filesystem full index should queue");
    source.write("src/lib.rs", "pub fn queued_policy() -> u32 { 2 }\n");
    let changed = service
        .start_code_repository_index(request, context("start-filesystem-active-changed"))
        .await
        .expect("changed filesystem full index should queue new snapshot");

    assert_ne!(
        changed.task.as_ref().map(|task| task.task_id.as_str()),
        first.task.as_ref().map(|task| task.task_id.as_str())
    );
    assert_ne!(
        changed
            .task
            .as_ref()
            .map(|task| task.resolved_commit_sha.as_str()),
        first
            .task
            .as_ref()
            .map(|task| task.resolved_commit_sha.as_str())
    );
}

#[tokio::test]
async fn queued_worktree_overlay_task_pins_payload_to_queued_base() {
    let repo = FixtureRepo::create("queued-worktree-overlay-pinned-base");
    repo.write("src/lib.rs", "pub fn overlay_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(Arc::clone(&store)).await;

    register_fixture_repo(&service, &repo, "register-queued-overlay").await;
    service
        .index_code_repository(
            request("fixture", "HEAD"),
            context("index-queued-overlay-base"),
        )
        .await
        .expect("base index should succeed");
    let queued_base = repo.git_text(["rev-parse", "HEAD"]);

    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::WorktreeOverlay,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("start-queued-overlay-pinned-base"),
        )
        .await
        .expect("worktree overlay should queue");
    let task = started.task.expect("overlay task should be queued");
    let payload: CodeIndexRequest =
        serde_json::from_str(&task.payload_json).expect("task payload should deserialize");
    assert_eq!(task.ref_selector, queued_base);
    assert_eq!(payload.repository.ref_selector, queued_base);

    repo.write("src/lib.rs", "pub fn overlay_policy() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "advance-head-before-worker"]);

    let error = service
        .run_code_index_task_once(Some(task.task_id), context("run-stale-queued-overlay"))
        .await
        .expect_err("queued overlay should reject a different checked-out HEAD");
    assert!(error.message.contains("checked-out HEAD"));
}

#[tokio::test]
async fn worktree_overlay_requires_indexed_head_base_scope() {
    let repo = FixtureRepo::create("worktree-overlay-requires-base");
    repo.write("src/lib.rs", "pub fn base_required() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-worktree-base-required").await;
    let error = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::WorktreeOverlay,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-worktree-without-base"),
        )
        .await
        .expect_err("worktree overlay should require a HEAD base scope");

    assert!(
        error
            .message
            .contains("requires an indexed HEAD base scope")
    );
}

#[tokio::test]
async fn clean_worktree_overlay_query_uses_persisted_worktree_scope() {
    let repo = FixtureRepo::create("clean-worktree-overlay-query");
    repo.write(
        "src/lib.rs",
        "pub fn clean_worktree_policy() -> u32 { 1 }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-clean-worktree-overlay").await;
    service
        .index_code_repository(
            request("fixture", "HEAD"),
            context("index-clean-worktree-base"),
        )
        .await
        .expect("base index should succeed");
    let overlay = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::WorktreeOverlay,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-clean-worktree-overlay"),
        )
        .await
        .expect("clean worktree overlay should reuse the HEAD snapshot");

    assert_eq!(overlay.scope.requested_ref, "worktree");
    assert!(overlay.summary.resolved_commit_sha.starts_with("worktree:"));
    assert!(overlay.summary.tree_hash.starts_with("worktree:"));

    let query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "clean_worktree_policy",
                selector("fixture", "worktree"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-clean-worktree-overlay"),
        )
        .await
        .expect("worktree query should read the persisted clean overlay scope");
    assert!(query.results.iter().any(|hit| hit.path == "src/lib.rs"));
}

#[tokio::test]
async fn start_worktree_overlay_queues_task_and_query_revalidates_worktree() {
    let repo = FixtureRepo::create("queued-worktree-overlay-revalidation");
    repo.write(
        "src/lib.rs",
        "pub fn queued_worktree_policy() -> u32 { 1 }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-queued-worktree-revalidation").await;
    service
        .index_code_repository(
            request("fixture", "HEAD"),
            context("index-head-before-overlay"),
        )
        .await
        .expect("HEAD index should succeed");
    repo.write("src/worktree.rs", "pub fn queued_overlay_policy() {}\n");
    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::WorktreeOverlay,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-worktree-overlay-task"),
        )
        .await
        .expect("worktree overlay should queue");
    let task_id = started.task.as_ref().expect("queued task").task_id.clone();
    assert_eq!(started.scope.requested_ref, "worktree");

    service
        .run_code_index_task_once(Some(task_id), context("run-worktree-overlay-task"))
        .await
        .expect("queued overlay should run");
    repo.write("src/later.rs", "pub fn later_worktree_policy() {}\n");
    let error = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "later_worktree_policy",
                selector("fixture", "worktree"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-stale-worktree-overlay"),
        )
        .await
        .expect_err("changed worktree should require overlay reindex");

    assert!(error.message.contains("worktree overlay is stale"));

    let error = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                selector("fixture", "worktree"),
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("feature flag request should validate"),
            context("feature-flags-stale-worktree-overlay"),
        )
        .await
        .expect_err("feature flags should require overlay reindex");
    assert!(error.message.contains("worktree overlay is stale"));

    let error = service
        .software_global_projection(
            SoftwareGlobalRequest::new(
                selector("fixture", "worktree"),
                SoftwareGlobalKind::All,
                FreshnessPolicy::AllowStale,
                10,
            )
            .expect("software request should validate"),
            context("software-stale-worktree-overlay"),
        )
        .await
        .expect_err("software projection should require overlay reindex");
    assert!(error.message.contains("worktree overlay is stale"));
}

#[tokio::test]
async fn worktree_overlay_freshness_uses_persisted_scope_filters() {
    let repo = FixtureRepo::create("filtered-worktree-overlay-freshness");
    repo.write(
        "src/selected.rs",
        "pub fn selected_overlay_policy() -> u32 { 1 }\n",
    );
    repo.write("src/ignored.rs", "pub fn ignored_overlay_policy() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-filtered-worktree-overlay"),
        )
        .await
        .expect("repository should register");
    let scoped_head = CodeRepositorySelector::new(
        "fixture",
        "HEAD",
        vec!["src/selected.rs".to_owned()],
        Vec::new(),
    )
    .expect("selector should validate");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: scoped_head.clone(),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-filtered-worktree-base"),
        )
        .await
        .expect("filtered base index should succeed");
    repo.write(
        "src/selected.rs",
        "pub fn selected_overlay_policy_v2() -> u32 { 2 }\n",
    );
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: scoped_head,
                mode: CodeIndexMode::WorktreeOverlay,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-filtered-worktree-overlay"),
        )
        .await
        .expect("filtered worktree overlay should index");
    repo.write("src/ignored.rs", "pub fn ignored_overlay_policy_v2() {}\n");

    let scoped_worktree = CodeRepositorySelector::new(
        "fixture",
        "worktree",
        vec!["src/selected.rs".to_owned()],
        Vec::new(),
    )
    .expect("selector should validate");
    let query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "selected_overlay_policy_v2",
                scoped_worktree,
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-filtered-worktree-overlay"),
        )
        .await
        .expect("filtered worktree query should ignore out-of-scope changes");

    assert!(
        query
            .results
            .iter()
            .any(|hit| hit.path == "src/selected.rs")
    );
}

#[tokio::test]
async fn worktree_query_rejects_dirty_worktree_after_head_only_index() {
    let repo = FixtureRepo::create("dirty-worktree-query-with-head-only-index");
    repo.write("src/lib.rs", "pub fn head_only_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-head-only-worktree").await;
    service
        .index_code_repository(
            request("fixture", "HEAD"),
            context("index-head-only-worktree"),
        )
        .await
        .expect("HEAD index should succeed");
    repo.write("src/worktree.rs", "pub fn unindexed_worktree_policy() {}\n");

    let error = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "unindexed_worktree_policy",
                selector("fixture", "worktree"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-head-only-dirty-worktree"),
        )
        .await
        .expect_err("worktree query must not fall back to a plain HEAD index");

    assert!(error.message.contains("has no active worktree overlay"));
}

#[tokio::test]
async fn full_index_refreshes_running_watcher_after_initial_scope() {
    let repo = FixtureRepo::create("watcher-refresh-after-initial-index");
    repo.write("src/lib.rs", "pub fn watched_after_index() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(Arc::clone(&store)).await;
    let handle = service
        .start_code_repository_watcher()
        .await
        .expect("watcher should start")
        .expect("watcher should be enabled");

    register_fixture_repo(&service, &repo, "register-watcher-refresh").await;
    assert_eq!(handle.repository_count().await, 0);

    service
        .index_code_repository(request("fixture", "HEAD"), context("index-watcher-refresh"))
        .await
        .expect("initial index should refresh watcher");

    assert_eq!(handle.repository_count().await, 1);
    handle.request_shutdown();
}

#[tokio::test]
async fn allow_stale_query_reports_pending_freshness_and_source_read_requirement() {
    let repo = FixtureRepo::create("code-query-pending-freshness");
    repo.write("src/lib.rs", "pub fn pending_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-code-query-pending-freshness").await;
    let indexed = service
        .index_code_repository(
            request("fixture", "HEAD"),
            context("index-code-query-pending-base"),
        )
        .await
        .expect("base index should succeed");
    repo.write("src/lib.rs", "pub fn pending_policy() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "changed"]);
    let queued = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-code-query-pending-refresh"),
        )
        .await
        .expect("changed index should queue");

    let query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "pending_policy",
                selector("fixture", "HEAD"),
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-code-query-pending-freshness"),
        )
        .await
        .expect("allow-stale query should return previous index");

    assert!(query.metadata.stale);
    assert!(query.scope.stale);
    assert_eq!(query.results[0].path, "src/lib.rs");
    assert!(query.results[0].stale);
    assert_eq!(
        query.results[0].staleness_hint,
        Some(StalenessHint::PendingIndex {})
    );
    assert_eq!(query.freshness.state, CodeRepositoryFreshnessState::Pending);
    assert!(query.freshness.direct_source_read_required);
    assert_eq!(
        query.freshness.index_lag.served_ref,
        indexed.summary.resolved_commit_sha
    );
    assert_ne!(
        query.freshness.index_lag.requested_resolved_ref,
        query.freshness.index_lag.served_ref
    );
    assert!(!query.freshness.index_lag.requested_ref_indexed);
    assert!(query.freshness.pending.active_matches_request);
    assert_eq!(
        query.freshness.pending.active_task_id.as_deref(),
        queued.task.as_ref().map(|task| task.task_id.as_str())
    );
    assert_eq!(
        query.freshness.direct_source_read_paths,
        vec!["src/lib.rs".to_owned()]
    );
    assert!(
        query
            .freshness
            .agent_instructions
            .iter()
            .any(|instruction| instruction.contains("read direct source"))
    );
}

#[tokio::test]
async fn fresh_ref_query_ignores_unmatched_active_task_checkpoint() {
    let repo = FixtureRepo::create("code-query-unmatched-active-checkpoint");
    repo.write("src/lib.rs", "pub fn stable_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let initial_commit = repo.git_text(["rev-parse", "HEAD"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(Arc::clone(&store)).await;

    register_fixture_repo(&service, &repo, "register-unmatched-active-checkpoint").await;
    let indexed = service
        .index_code_repository(
            request("fixture", &initial_commit),
            context("index-unmatched-active-base"),
        )
        .await
        .expect("initial commit should index");
    repo.write("src/lib.rs", "pub fn stable_policy() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "changed"]);
    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-unmatched-active-head"),
        )
        .await
        .expect("head refresh should queue");
    let active_task = started.task.expect("active task should be queued");
    store
        .begin_code_index_session(CodeIndexSession {
            repository_id: active_task.repository_id.clone(),
            source_scope: active_task.source_scope.clone(),
            base_resolved_commit_sha: Some(initial_commit.clone()),
            resolved_commit_sha: active_task.resolved_commit_sha.clone(),
            tree_hash: active_task.tree_hash.clone(),
            path_filters: active_task.path_filters.clone(),
            language_filters: active_task.language_filters.clone(),
            full_replace: true,
            total_path_count: 8,
            changed_path_count: 8,
            skipped_unchanged_count: 0,
            deleted_paths: Vec::new(),
            tombstones: Vec::new(),
            workspaces: Vec::new(),
            resource_budget: CodeIndexResourceBudget::default(),
        })
        .await
        .expect("unmatched active checkpoint should begin");

    let query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "stable_policy",
                selector("fixture", &initial_commit),
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-unmatched-active-base"),
        )
        .await
        .expect("fresh indexed ref should query while another ref is active");

    assert_eq!(query.freshness.state, CodeRepositoryFreshnessState::Fresh);
    assert!(query.freshness.pending.active_for_repository);
    assert!(!query.freshness.pending.active_matches_request);
    assert_eq!(
        query
            .freshness
            .cursor
            .as_ref()
            .map(|cursor| cursor.source_scope.as_str()),
        Some(indexed.summary.source_scope.as_str())
    );
    assert_ne!(
        query
            .freshness
            .cursor
            .as_ref()
            .map(|cursor| cursor.source_scope.as_str()),
        Some(active_task.source_scope.as_str())
    );
}

#[test]
fn active_pending_match_keeps_fresh_hit_when_source_read_is_not_required() {
    let mut hits = vec![test_hit()];
    let freshness = freshness_with_active_match(false);

    super::repository::annotate_query_result_staleness(&mut hits, &freshness);

    assert!(!hits[0].stale);
    assert_eq!(hits[0].staleness_hint, Some(StalenessHint::Fresh));
}

fn freshness_with_active_match(
    direct_source_read_required: bool,
) -> CodeRepositoryFreshnessDiagnostics {
    CodeRepositoryFreshnessDiagnostics {
        state: if direct_source_read_required {
            CodeRepositoryFreshnessState::Pending
        } else {
            CodeRepositoryFreshnessState::Fresh
        },
        freshness_policy: FreshnessPolicy::AllowStale,
        graph_version: 1,
        source_scope: Some("scope".to_owned()),
        scope_stale: direct_source_read_required,
        stale_reason: None,
        degraded_reason: None,
        index_lag: CodeRepositoryIndexLag {
            requested_ref: "HEAD".to_owned(),
            requested_resolved_ref: "commit".to_owned(),
            served_ref: if direct_source_read_required {
                "previous".to_owned()
            } else {
                "commit".to_owned()
            },
            requested_ref_indexed: !direct_source_read_required,
            pending_file_count: None,
            pending_task_count: 1,
        },
        pending: CodeRepositoryPendingIndexWork {
            active_for_repository: true,
            active_matches_request: true,
            active_task_id: Some("task".to_owned()),
            active_task_state: Some("running".to_owned()),
            active_task_source_scope: Some("scope".to_owned()),
            active_task_ref_selector: Some("HEAD".to_owned()),
            active_task_resolved_commit_sha: Some("commit".to_owned()),
            active_task_lease_expires_at_ms: Some(2),
            queue_depth: 1,
            queued_task_count: 0,
            running_task_count: 1,
            retrying_task_count: 0,
            dead_letter_task_count: 0,
            running_lease_count: 1,
            last_error: None,
        },
        cursor: None,
        direct_source_read_required,
        direct_source_read_paths: Vec::new(),
        agent_instructions: Vec::new(),
    }
}

fn test_hit() -> CodeRetrievalHit {
    let range = RepositoryCodeRange { start: 1, end: 2 };
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: "src/lib.rs".to_owned(),
        language_id: "rust".to_owned(),
        byte_range: range.clone(),
        line_range: range,
        symbol_snapshot_id: None,
        canonical_symbol_id: None,
        file_id: None,
        retrieval_layers: vec![CodeRetrievalLayer::Definition],
        index_versions: vec!["code:scope:tree".to_owned()],
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 1.0,
        excerpt: "pub fn stable_policy() -> u32 { 1 }".to_owned(),
    }
}

#[tokio::test]
async fn filesystem_impact_reports_deleted_base_paths_from_stored_fingerprints() {
    let source = FixtureSourceDir::create("filesystem-impact-deleted-base-path");
    source.write("src/lib.rs", "pub fn unchanged_policy() -> u32 { 1 }\n");
    source.write("src/api.rs", "pub fn removed_policy() -> u32 { 1 }\n");
    let service = service_with_memory_store().await;

    register_fixture_source(&service, &source, "register-filesystem-impact-delete").await;
    let base = service
        .index_code_repository(request("fixture", "HEAD"), context("index-filesystem-base"))
        .await
        .expect("base filesystem index should succeed");
    fs::remove_file(source.path.join("src/api.rs")).expect("fixture source should delete");
    service
        .index_code_repository(request("fixture", "HEAD"), context("index-filesystem-head"))
        .await
        .expect("head filesystem index should succeed");

    let impact = service
        .impact_code_repository(
            CodeImpactRequest::new(
                selector("fixture", "HEAD"),
                base.summary.resolved_commit_sha,
                "HEAD",
                10,
            )
            .expect("impact request should validate"),
            context("impact-filesystem-delete"),
        )
        .await
        .expect("filesystem impact should succeed");

    assert_eq!(
        impact.path_groups.in_scope_changed_paths,
        ["src/api.rs".to_owned()]
    );
}
