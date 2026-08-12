use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexSnapshot,
        CodeRepositoryRegistration, code_snapshot_scope_id,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskSeed, CodeRepositoryStore,
        PartitionedSqliteKnowledgeStore,
    },
};

use super::super::test_support::{partitioned_store, partitioned_store_with_paths};
use super::clear_workspace;

const REPOSITORY_ID: &str = "repo-partitioned-rebind";
const ALIAS: &str = "partitioned-rebind";
const BASE_COMMIT: &str = "base-commit";

#[tokio::test]
async fn clearing_an_unpublished_workspace_remains_idempotent() {
    let store = partitioned_store("clear-workspace");

    clear_workspace(
        &store,
        "repo-missing".to_owned(),
        "scope-missing".to_owned(),
    )
    .await
    .expect("empty workspace clearing should succeed");
}

#[tokio::test]
async fn prepared_worktree_rebind_survives_reopen_and_retries_shard_publication() {
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
    let reopened = PartitionedSqliteKnowledgeStore::open(control_path, paths)
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
        .expect("normal retry should idempotently publish the prepared target");
    assert_eq!(summary.source_scope, real_scope);
}

#[tokio::test]
async fn stale_generation_cannot_prepare_partitioned_worktree_rebind() {
    let store = partitioned_store("worktree-rebind-stale-generation");
    let (pending_scope, real_scope, snapshot) = worktree_fixture(&store).await;
    let now_ms = now_millis();
    let old = claim_task(&store, "worker-old", now_ms, 10).await;
    let current = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(old.task_id.clone()),
            lease_owner: "worker-current".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_ms.saturating_add(11),
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
    assert!(error.to_string().contains("cannot prepare scope"));
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
    store
        .apply_code_index_snapshot(empty_snapshot(
            base_scope,
            None,
            BASE_COMMIT,
            "base-tree",
            true,
        ))
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
