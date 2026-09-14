use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexSnapshot,
        CodeParseStatus, CodeRepositoryRegistration, RepositoryCodeFileRecord,
        code_snapshot_scope_id,
    },
    storage::{
        CodeIndexPublicationStore as _, CodeIndexTaskClaimRequest, CodeIndexTaskSeed,
        CodeIndexTaskStore as _, RepositoryCatalogStore as _, SqliteGraphStore, StorageError,
    },
};

const REPOSITORY_ID: &str = "durable-affected-path-pages";
const ALIAS: &str = "durable-affected-path-pages";
const BASE_COMMIT: &str = "base-commit";
const FILE_COUNT: usize = 8;

#[tokio::test]
async fn affected_path_ownership_pages_past_the_file_quantum_and_recovers_between_leases() {
    let store = SqliteGraphStore::open_in_memory().expect("database should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                REPOSITORY_ID,
                ALIAS,
                "/tmp/durable-affected-path-pages",
                vec![],
                vec![],
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    let base_tree = "affected-page-base-tree";
    let base_scope = code_snapshot_scope_id(REPOSITORY_ID, base_tree, &[], &[]);
    let budget = CodeIndexResourceBudget::new(6, 1_000_000, 96)
        .expect("bounded clone budget should validate");
    store
        .apply_code_index_snapshot(snapshot(&base_scope, base_tree, None, false, FILE_COUNT))
        .await
        .expect("base snapshot should publish");
    persist_base_fact_proof(&store, &base_scope, base_tree, budget).await;

    let overlay_hash = "fedcba9876543210";
    let actual_tree = format!("worktree:{overlay_hash}");
    let actual_commit = format!("worktree:{BASE_COMMIT}:{overlay_hash}");
    let actual_scope = code_snapshot_scope_id(REPOSITORY_ID, &actual_tree, &[], &[]);
    let pending_tree = format!("worktree:pending:{BASE_COMMIT}");
    let pending_scope = code_snapshot_scope_id(REPOSITORY_ID, &pending_tree, &[], &[]);
    let snapshot = snapshot(
        &actual_scope,
        &actual_tree,
        Some((&actual_commit, BASE_COMMIT)),
        true,
        7,
    );
    let queued = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: REPOSITORY_ID.to_owned(),
            alias: ALIAS.to_owned(),
            ref_selector: BASE_COMMIT.to_owned(),
            resolved_commit_sha: pending_tree.clone(),
            tree_hash: pending_tree,
            source_scope: pending_scope,
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::WorktreeOverlay,
            input_fingerprint: "durable-affected-path-pages".to_owned(),
            resource_budget: budget,
            payload_json: "{}".to_owned(),
            now_ms: now_millis(),
        })
        .await
        .expect("worktree task should queue");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "affected-page-worker-old".to_owned(),
            lease_duration_ms: 600_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("task claim should run")
        .expect("worktree task should claim");
    let old_fence = fence(&running, "affected-page-worker-old");

    assert_pending_step(
        store
            .apply_code_index_snapshot_with_fence(snapshot.clone(), old_fence.clone())
            .await,
        0,
    );
    assert_eq!(clone_owner_counts(&store, &actual_scope).await, (1, 0));
    assert_pending_step(
        store
            .apply_code_index_snapshot_with_fence(snapshot.clone(), old_fence)
            .await,
        1,
    );
    assert_eq!(clone_owner_counts(&store, &actual_scope).await, (1, 6));

    expire_task(&store, &running.task_id).await;
    let reclaimed = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(running.task_id),
            lease_owner: "affected-page-worker-new".to_owned(),
            lease_duration_ms: 600_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("affected-path takeover should run")
        .expect("expired task should be reclaimed");
    let recovered_fence = fence(&reclaimed, "affected-page-worker-new");
    assert_pending_step(
        store
            .apply_code_index_snapshot_with_fence(snapshot.clone(), recovered_fence.clone())
            .await,
        2,
    );
    assert_eq!(clone_owner_counts(&store, &actual_scope).await, (1, 7));

    for _ in 0..128 {
        match store
            .apply_code_index_snapshot_with_fence(snapshot.clone(), recovered_fence.clone())
            .await
        {
            Err(StorageError::DurableStagingPending { .. }) => {}
            Err(StorageError::DurableFinalizationRequired { checkpoint_state }) => {
                assert_eq!(checkpoint_state, "indexing");
                let checkpoint = store
                    .code_index_checkpoint(actual_scope.clone())
                    .await
                    .expect("checkpoint should load")
                    .expect("checkpoint should remain staged");
                let receipt = checkpoint
                    .incremental_summary
                    .expect("delta handoff receipt should persist");
                assert_eq!(receipt.batch_count, 2);
                assert_eq!(receipt.parsed_file_count, 7);
                assert_eq!(clone_owner_counts(&store, &actual_scope).await, (0, 0));
                return;
            }
            Ok(_) => panic!("durable clone must expose its finalization handoff"),
            Err(error) => panic!("durable affected-path clone failed: {error}"),
        }
    }
    panic!("durable affected-path clone exceeded its bounded call proof");
}

fn snapshot(
    source_scope: &str,
    tree_hash: &str,
    commits: Option<(&str, &str)>,
    incremental: bool,
    file_count: usize,
) -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: REPOSITORY_ID.to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: commits.map(|(_, base)| base.to_owned()),
        resolved_commit_sha: commits.map_or(BASE_COMMIT, |(target, _)| target).to_owned(),
        tree_hash: tree_hash.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: !incremental,
        changed_path_count: file_count,
        skipped_unchanged_count: usize::from(incremental) * (FILE_COUNT - file_count),
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: (0..file_count)
            .map(|index| RepositoryCodeFileRecord {
                repository_id: REPOSITORY_ID.to_owned(),
                source_scope: source_scope.to_owned(),
                file_id: format!("file-{index:03}"),
                path: path(index),
                language_id: "rust".to_owned(),
                blob_hash: format!("blob-{index:03}"),
                byte_len: 32,
                line_count: 1,
                parse_status: CodeParseStatus::Parsed,
                is_generated: false,
                degraded_reason: None,
            })
            .collect(),
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

async fn persist_base_fact_proof(
    store: &SqliteGraphStore,
    source_scope: &str,
    tree_hash: &str,
    budget: CodeIndexResourceBudget,
) {
    let source_scope = source_scope.to_owned();
    let tree_hash = tree_hash.to_owned();
    let budget_json = serde_json::to_string(&budget).expect("budget should serialize");
    store
        .run(move |connection| {
            connection.execute(
                "INSERT INTO code_repository_index_checkpoints (
                     source_scope, repository_id, state, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, total_path_count,
                     parsed_file_count, committed_file_count, committed_symbol_count,
                     committed_reference_count, committed_chunk_count, committed_fact_row_count,
                     batch_count, last_path, resource_budget_json, updated_at_ms, error_message
                 ) VALUES (
                     ?1, ?2, 'completed', ?3, ?4, '[]', '[]', ?5,
                     ?5, ?5, 0, 0, 0, ?5, 2, ?6, ?7, ?8, NULL
                 )",
                rusqlite::params![
                    source_scope,
                    REPOSITORY_ID,
                    BASE_COMMIT,
                    tree_hash,
                    FILE_COUNT,
                    path(FILE_COUNT - 1),
                    budget_json,
                    now_millis(),
                ],
            )?;
            Ok(())
        })
        .await
        .expect("base fact proof should persist");
}

fn fence(task: &crate::domain::CodeIndexTaskRecord, owner: &str) -> CodeIndexPublicationFence {
    CodeIndexPublicationFence {
        repository_id: task.repository_id.clone(),
        task_id: task.task_id.clone(),
        lease_owner: owner.to_owned(),
        attempt_count: task.attempt_count,
        generation: task.publication_generation,
    }
}

fn assert_pending_step(
    result: Result<crate::domain::CodeIndexSummary, StorageError>,
    expected_step: usize,
) {
    match result {
        Err(StorageError::DurableStagingPending {
            completed_steps,
            max_steps,
        }) => {
            assert_eq!(completed_steps, expected_step);
            assert!(completed_steps <= max_steps);
        }
        Ok(_) => panic!("durable clone unexpectedly finished"),
        Err(error) => panic!("durable clone returned the wrong state: {error}"),
    }
}

async fn expire_task(store: &SqliteGraphStore, task_id: &str) {
    let task_id = task_id.to_owned();
    let changed = store
        .run(move |connection| {
            connection
                .execute(
                    "UPDATE code_repository_index_tasks
                     SET lease_expires_at_ms = 0
                     WHERE task_id = ?1 AND state = 'running'",
                    [task_id],
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("lease should expire");
    assert_eq!(changed, 1);
}

async fn clone_owner_counts(store: &SqliteGraphStore, source_scope: &str) -> (usize, usize) {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                         (SELECT COUNT(*) FROM code_repository_incremental_clone_progress
                          WHERE source_scope = ?1),
                         (SELECT COUNT(*) FROM code_repository_incremental_clone_affected_paths
                          WHERE source_scope = ?1)",
                    [source_scope],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("clone owner counts should load")
}

fn path(index: usize) -> String {
    format!("src/file-{index:03}.rs")
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow epoch")
        .as_millis() as u64
}
