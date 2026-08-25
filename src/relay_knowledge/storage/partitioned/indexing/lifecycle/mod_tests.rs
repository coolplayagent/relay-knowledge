use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexSession,
        CodeIndexSnapshot, CodeRepositoryRegistration, code_snapshot_scope_id,
    },
    storage::{
        CodeIndexFinalizationStep, CodeIndexTaskClaimRequest, CodeIndexTaskSeed,
        CodeRepositoryStore, PartitionedSqliteKnowledgeStore,
    },
};

use super::super::test_support::{partitioned_store, partitioned_store_with_paths};
use super::clear_workspace;

const REPOSITORY_ID: &str = "repo-partitioned-rebind";
const ALIAS: &str = "partitioned-rebind";
const BASE_COMMIT: &str = "base-commit";

#[tokio::test]
async fn clearing_an_unpublished_workspace_requires_a_partitioned_fence() {
    let store = partitioned_store("clear-workspace");

    clear_workspace(
        &store,
        "repo-missing".to_owned(),
        "scope-missing".to_owned(),
    )
    .await
    .expect_err("partitioned workspace clearing must require a task fence");
}

#[tokio::test]
async fn staged_route_survives_reopen_and_resumes_the_same_fenced_checkpoint() {
    let (store, control_path, paths) = partitioned_store_with_paths("checkpoint-route-reopen");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                REPOSITORY_ID,
                ALIAS,
                "/tmp/partitioned-checkpoint-route",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    let source_scope = code_snapshot_scope_id(REPOSITORY_ID, "resume-tree", &[], &[]);
    store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: REPOSITORY_ID.to_owned(),
            alias: ALIAS.to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "resume-commit".to_owned(),
            tree_hash: "resume-tree".to_owned(),
            source_scope: source_scope.clone(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::Full,
            input_fingerprint: format!("checkpoint-route-{source_scope}"),
            resource_budget: CodeIndexResourceBudget::default(),
            payload_json: "{}".to_owned(),
            now_ms: now_millis(),
        })
        .await
        .expect("full task should queue");
    let running = claim_task(&store, "resume-worker", now_millis(), 60_000).await;
    let publication_fence = fence(&running, "resume-worker");
    let session = CodeIndexSession {
        repository_id: REPOSITORY_ID.to_owned(),
        source_scope: source_scope.clone(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "resume-commit".to_owned(),
        tree_hash: "resume-tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count: 0,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        changed_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget: CodeIndexResourceBudget::default(),
    };
    let checkpoint = store
        .begin_code_index_session_with_fence(session.clone(), publication_fence.clone())
        .await
        .expect("checkpoint and its pre-staged route should commit");

    drop(store);
    let reopened = PartitionedSqliteKnowledgeStore::open(control_path.clone(), paths.clone())
        .expect("partitioned store should reopen after the simulated crash");
    let recovered = reopened
        .code_index_checkpoint(source_scope)
        .await
        .expect("pre-staged route should locate the shard checkpoint")
        .expect("checkpoint should survive reopen");
    assert_eq!(recovered, checkpoint);

    let resumed = reopened
        .begin_code_index_session_at_checkpoint_with_fence(
            session,
            Some(recovered.clone()),
            publication_fence,
        )
        .await
        .expect("the same task and fence should resume its exact checkpoint");
    assert_eq!(resumed, recovered);
}

#[tokio::test]
async fn prepared_worktree_rebind_survives_reopen_and_publishes_direct_snapshot() {
    let (store, control_path, paths) = partitioned_store_with_paths("worktree-rebind-reopen");
    let (pending_scope, real_scope, snapshot) = worktree_fixture(&store).await;
    let running = claim_task(&store, "worker-reopen", now_millis(), 60_000).await;
    assert_eq!(running.source_scope, pending_scope);
    let publication_fence = fence(&running, "worker-reopen");

    store
        .catalog
        .prepare_snapshot_target(&snapshot, publication_fence.clone())
        .await
        .expect("control WAL should durably prepare the real target");
    let prepared = store
        .active_code_index_task(REPOSITORY_ID.to_owned())
        .await
        .expect("prepared task should load")
        .expect("prepared task should remain active");
    assert_eq!(prepared.source_scope, real_scope);

    drop(store);
    let reopened = PartitionedSqliteKnowledgeStore::open(control_path.clone(), paths.clone())
        .expect("partitioned store should reopen after the simulated crash");
    let recovered = reopened
        .active_code_index_task(REPOSITORY_ID.to_owned())
        .await
        .expect("reopened task should load")
        .expect("durable handoff should remain active");
    assert_eq!(recovered.source_scope, real_scope);

    let summary = reopened
        .apply_code_index_snapshot_with_fence(snapshot, publication_fence)
        .await
        .expect("a bounded worktree overlay should publish without a clean-full fallback");
    assert_eq!(summary.source_scope, real_scope);
    assert!(
        reopened
            .catalog
            .staged_scope_owned_by_task(
                REPOSITORY_ID.to_owned(),
                real_scope.clone(),
                running.task_id.clone(),
            )
            .await
            .expect("staged route should remain inspectable")
    );
    assert!(
        reopened
            .code_index_checkpoint(real_scope.clone())
            .await
            .expect("routed target checkpoint should query")
            .is_none(),
        "the direct worktree protocol must not fabricate a durable clone checkpoint"
    );

    drop(reopened);
    let reopened = PartitionedSqliteKnowledgeStore::open(control_path, paths)
        .expect("partitioned store should reopen after direct shard publication");
    assert!(
        reopened
            .catalog
            .staged_scope_owned_by_task(REPOSITORY_ID.to_owned(), real_scope, running.task_id,)
            .await
            .expect("staged direct route should survive response loss and reopen")
    );
}

#[tokio::test]
async fn stale_generation_cannot_prepare_partitioned_worktree_rebind() {
    let store = partitioned_store("worktree-rebind-stale-generation");
    let (pending_scope, real_scope, snapshot) = worktree_fixture(&store).await;
    let old = claim_task(&store, "worker-old", now_millis(), 60_000).await;
    expire_task_lease(&store, &old.task_id).await;
    let current = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(old.task_id.clone()),
            lease_owner: "worker-current".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("expired attempt takeover should run")
        .expect("expired attempt should be reclaimed");
    assert!(current.publication_generation > old.publication_generation);

    let error = store
        .catalog
        .prepare_snapshot_target(&snapshot, fence(&old, "worker-old"))
        .await
        .expect_err("stale generation must not prepare the control handoff");
    assert!(
        matches!(error, crate::storage::StorageError::InvalidInput(message) if message.contains("no longer active"))
    );
    let active = store
        .active_code_index_task(REPOSITORY_ID.to_owned())
        .await
        .expect("current task should load")
        .expect("current task should remain active");
    assert_eq!(active.source_scope, pending_scope);

    store
        .catalog
        .prepare_snapshot_target(&snapshot, fence(&current, "worker-current"))
        .await
        .expect("current generation should prepare the handoff");
    let prepared = store
        .active_code_index_task(REPOSITORY_ID.to_owned())
        .await
        .expect("prepared task should load")
        .expect("prepared task should remain active");
    assert_eq!(prepared.source_scope, real_scope);
}

#[tokio::test]
async fn fenced_finalization_advances_one_durable_staged_shard_checkpoint_per_call() {
    let store = partitioned_store("single-step-finalization");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                REPOSITORY_ID,
                ALIAS,
                "/tmp/partitioned-single-step",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    let tree_hash = "single-step-tree";
    let source_scope = code_snapshot_scope_id(REPOSITORY_ID, tree_hash, &[], &[]);
    store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: REPOSITORY_ID.to_owned(),
            alias: ALIAS.to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "single-step-commit".to_owned(),
            tree_hash: tree_hash.to_owned(),
            source_scope: source_scope.clone(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::Full,
            input_fingerprint: format!("single-step-{source_scope}"),
            resource_budget: CodeIndexResourceBudget::default(),
            payload_json: "{}".to_owned(),
            now_ms: now_millis(),
        })
        .await
        .expect("single-step task should queue");
    let running = claim_task(&store, "worker-single-step", now_millis(), 60_000).await;
    let publication_fence = fence(&running, "worker-single-step");
    let session = CodeIndexSession {
        repository_id: REPOSITORY_ID.to_owned(),
        source_scope: source_scope.clone(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "single-step-commit".to_owned(),
        tree_hash: tree_hash.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count: 0,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        changed_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget: CodeIndexResourceBudget::default(),
    };
    store
        .begin_code_index_session_with_fence(session.clone(), publication_fence.clone())
        .await
        .expect("single-step session should begin in the staged shard");
    let shard = store
        .catalog
        .checkpoint_repository_store(REPOSITORY_ID.to_owned())
        .await
        .expect("staged shard route should load")
        .expect("staged shard should exist");

    let first = store
        .advance_code_index_session_with_fence(session.clone(), publication_fence.clone())
        .await
        .expect("first finalization quantum should advance");
    let CodeIndexFinalizationStep::Pending {
        checkpoint_state: first_state,
    } = first
    else {
        panic!("first finalization quantum must remain pending");
    };
    assert_eq!(first_state, "finalizing:build_query_indexes:v3:0");
    assert_eq!(
        shard
            .code_index_checkpoint(source_scope.clone())
            .await
            .expect("first durable checkpoint should load")
            .expect("first durable checkpoint should exist")
            .state,
        first_state
    );

    let second = store
        .advance_code_index_session_with_fence(session, publication_fence)
        .await
        .expect("second finalization quantum should advance");
    let CodeIndexFinalizationStep::Pending {
        checkpoint_state: second_state,
    } = second
    else {
        panic!("second finalization quantum must remain pending");
    };
    assert_eq!(second_state, "finalizing:build_query_indexes:v3:2");
    assert_eq!(
        shard
            .code_index_checkpoint(source_scope)
            .await
            .expect("second durable checkpoint should load")
            .expect("second durable checkpoint should exist")
            .state,
        second_state
    );
}

#[tokio::test]
async fn fenced_full_scope_routes_to_its_staged_shard_before_catalog_activation() {
    let store = partitioned_store("fenced-full-software-barrier");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                REPOSITORY_ID,
                ALIAS,
                "/tmp/partitioned-fenced-full",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    let tree_hash = "fenced-full-tree";
    let source_scope = code_snapshot_scope_id(REPOSITORY_ID, tree_hash, &[], &[]);
    store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: REPOSITORY_ID.to_owned(),
            alias: ALIAS.to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "fenced-full-commit".to_owned(),
            tree_hash: tree_hash.to_owned(),
            source_scope: source_scope.clone(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::Full,
            input_fingerprint: format!("fenced-full-{source_scope}"),
            resource_budget: CodeIndexResourceBudget::default(),
            payload_json: "{}".to_owned(),
            now_ms: now_millis(),
        })
        .await
        .expect("full task should queue");
    let running = claim_task(&store, "worker-full", now_millis(), 60_000).await;
    let publication_fence = fence(&running, "worker-full");

    store
        .apply_code_index_snapshot_with_fence(
            empty_snapshot(
                source_scope.clone(),
                None,
                "fenced-full-commit",
                tree_hash,
                true,
            ),
            publication_fence.clone(),
        )
        .await
        .expect("full code scope should stage in its repository shard");
    assert_eq!(
        store
            .catalog
            .repository_for_scope(source_scope.clone())
            .await
            .expect("staged catalog route should query")
            .as_deref(),
        Some(REPOSITORY_ID),
        "checkpoint and retry routing must retain the staged shard owner"
    );
    assert!(
        store
            .catalog
            .active_repository_for_scope(source_scope.clone())
            .await
            .expect("active catalog route should query")
            .is_none(),
        "the new scope must remain hidden from ordinary reads before projection"
    );

    let projection = store
        .refresh_software_global_projection_with_fence(source_scope.clone(), publication_fence)
        .await
        .expect("software projection should route by fenced repository ownership");

    assert!(!projection.status.stale);
    assert_eq!(
        store
            .catalog
            .repository_for_scope(source_scope.clone())
            .await
            .expect("published catalog route should query")
            .as_deref(),
        Some(REPOSITORY_ID)
    );
    assert_eq!(
        store
            .catalog
            .active_repository_for_scope(source_scope.clone())
            .await
            .expect("published route should query")
            .as_deref(),
        Some(REPOSITORY_ID)
    );
    let status = store
        .code_repository_status(REPOSITORY_ID.to_owned())
        .await
        .expect("repository status should query")
        .expect("repository status should exist");
    assert!(!status.stale);
    assert_eq!(
        status.last_indexed_scope_id.as_deref(),
        Some(source_scope.as_str())
    );
}

async fn worktree_fixture(
    store: &PartitionedSqliteKnowledgeStore,
) -> (String, String, CodeIndexSnapshot) {
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                REPOSITORY_ID,
                ALIAS,
                "/tmp/partitioned-rebind",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    let base_scope = code_snapshot_scope_id(REPOSITORY_ID, "base-tree", &[], &[]);
    super::seed_snapshot_for_test(
        store,
        empty_snapshot(base_scope, None, BASE_COMMIT, "base-tree", true),
    )
    .await
    .expect("base scope should publish");

    let pending_tree = format!("worktree:pending:{BASE_COMMIT}");
    let pending_scope = code_snapshot_scope_id(REPOSITORY_ID, &pending_tree, &[], &[]);
    store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: REPOSITORY_ID.to_owned(),
            alias: ALIAS.to_owned(),
            ref_selector: BASE_COMMIT.to_owned(),
            resolved_commit_sha: pending_tree.clone(),
            tree_hash: pending_tree,
            source_scope: pending_scope.clone(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::WorktreeOverlay,
            input_fingerprint: format!("partitioned-rebind-{pending_scope}"),
            resource_budget: CodeIndexResourceBudget::default(),
            payload_json: "{}".to_owned(),
            now_ms: now_millis(),
        })
        .await
        .expect("worktree task should queue");

    let overlay_hash = "0123456789abcdef";
    let real_tree = format!("worktree:{overlay_hash}");
    let real_scope = code_snapshot_scope_id(REPOSITORY_ID, &real_tree, &[], &[]);
    let snapshot = empty_snapshot(
        real_scope.clone(),
        Some(BASE_COMMIT.to_owned()),
        &format!("worktree:{BASE_COMMIT}:{overlay_hash}"),
        &real_tree,
        false,
    );
    (pending_scope, real_scope, snapshot)
}

fn empty_snapshot(
    source_scope: String,
    base_resolved_commit_sha: Option<String>,
    resolved_commit_sha: &str,
    tree_hash: &str,
    full_replace: bool,
) -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: REPOSITORY_ID.to_owned(),
        source_scope,
        base_resolved_commit_sha,
        resolved_commit_sha: resolved_commit_sha.to_owned(),
        tree_hash: tree_hash.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: Vec::new(),
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

async fn claim_task(
    store: &PartitionedSqliteKnowledgeStore,
    lease_owner: &str,
    now_ms: u64,
    lease_duration_ms: u64,
) -> crate::domain::CodeIndexTaskRecord {
    store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: None,
            lease_owner: lease_owner.to_owned(),
            lease_duration_ms,
            max_attempts: 3,
            now_ms,
        })
        .await
        .expect("task claim should run")
        .expect("worktree task should claim")
}

async fn expire_task_lease(store: &PartitionedSqliteKnowledgeStore, task_id: &str) {
    let task_id = task_id.to_owned();
    let changed = store
        .control
        .run(move |connection| {
            Ok(connection.execute(
                "UPDATE code_repository_index_tasks
                 SET lease_expires_at_ms = 0
                 WHERE task_id = ?1 AND state = 'running'",
                [task_id],
            )?)
        })
        .await
        .expect("task lease fixture should expire");
    assert_eq!(changed, 1, "one running task lease should expire");
}

fn fence(
    task: &crate::domain::CodeIndexTaskRecord,
    lease_owner: &str,
) -> CodeIndexPublicationFence {
    CodeIndexPublicationFence {
        repository_id: task.repository_id.clone(),
        task_id: task.task_id.clone(),
        lease_owner: lease_owner.to_owned(),
        attempt_count: task.attempt_count,
        generation: task.publication_generation,
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow epoch")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
