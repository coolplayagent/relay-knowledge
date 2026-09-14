//! Cross-database publication-fence takeover tests owned by indexing lifecycle.

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    domain::{
        CodeIncrementalSummaryReceipt, CodeIndexBatch, CodeIndexMode, CodeIndexPublicationFence,
        CodeIndexResourceBudget, CodeIndexSession, CodeIndexSnapshot, CodeParseStatus,
        CodeQueryKind, CodeRepositoryRegistration, CodeRepositorySelector, CodeRetrievalRequest,
        FreshnessPolicy, RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        code_snapshot_scope_id,
    },
    storage::{
        CodeIndexPublicationStore as _, CodeIndexPublicationTarget, CodeIndexTaskClaimRequest,
        CodeIndexTaskCompletion, CodeIndexTaskSeed, CodeIndexTaskStore as _,
        CodeQueryReadStore as _, PartitionedSqliteKnowledgeStore, RepositoryCatalogStore as _,
        SoftwareProjectionStore as _, StorageError,
    },
};

use super::super::super::routing::is_missing_code_scope_error;
use super::super::test_support::partitioned_store;

#[tokio::test]
async fn takeover_fences_stale_shard_and_control_publication() {
    let store = partitioned_store("publication-fence-takeover");
    let source_scope = code_snapshot_scope_id("repo", "tree", &[], &[]);
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let queued = store
        .queue_code_index_task(task_seed(&source_scope))
        .await
        .expect("task should queue");
    let first = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id.clone()),
            lease_owner: "worker-old".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("first claim should run")
        .expect("first attempt should claim");
    let shard = store
        .catalog
        .staged_repository_store("repo".to_owned())
        .await
        .expect("repository shard should open");
    store
        .catalog
        .import_control_repository_metadata(Arc::clone(&shard), "repo".to_owned())
        .await
        .expect("control repository should import into the shard");
    let first_fence = publication_fence(&first, "worker-old");
    shard
        .apply_code_index_snapshot_with_fence(snapshot(&source_scope), first_fence.clone())
        .await
        .expect("first attempt should commit its shard while its lease is live");
    crate::storage::stage_empty_business_projection_with_fence_for_test(
        shard.as_ref(),
        first.repository_id.clone(),
        source_scope.clone(),
        first.resolved_commit_sha.clone(),
        first_fence.clone(),
    )
    .await
    .expect("first attempt should stage its business projection");
    shard
        .refresh_software_global_projection_with_fence(source_scope.clone(), first_fence)
        .await
        .expect("first attempt should durably publish the shard scope");
    store
        .catalog
        .stage_scope_with_fence(
            "repo".to_owned(),
            source_scope.clone(),
            publication_fence(&first, "worker-old"),
        )
        .await
        .expect("first attempt should durably stage its route");
    expire_task_lease(&store, &first.task_id).await;
    let second = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "worker-new".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("takeover claim should run")
        .expect("expired task should be reclaimed");

    let shard_error = shard
        .apply_code_index_snapshot_with_fence(
            snapshot(&source_scope),
            publication_fence(&first, "worker-old"),
        )
        .await
        .expect_err("stale shard writer must be fenced");
    assert!(matches!(shard_error, StorageError::InvalidInput(_)));
    let stage_error = store
        .catalog
        .stage_scope_with_fence(
            "repo".to_owned(),
            source_scope.clone(),
            publication_fence(&first, "worker-old"),
        )
        .await
        .expect_err("stale writer must not stage catalog routing after takeover");
    assert!(matches!(stage_error, StorageError::InvalidInput(_)));
    let mut staged_status = shard
        .code_repository_status("repo".to_owned())
        .await
        .expect("shard status should load")
        .expect("shard status should exist");
    staged_status.last_indexed_scope_id = Some(source_scope.clone());
    staged_status.state = "fresh".to_owned();
    staged_status.stale = false;
    let catalog_error = store
        .catalog
        .publish_scope_status_with_fence(
            "repo".to_owned(),
            source_scope.clone(),
            staged_status,
            publication_fence(&first, "worker-old"),
        )
        .await
        .expect_err("stale writer must not atomically activate and mirror after takeover");
    assert!(
        matches!(catalog_error, StorageError::InvalidInput(_)),
        "unexpected stale control publication error: {catalog_error:?}"
    );
    assert_eq!(
        store
            .catalog
            .repository_for_scope(source_scope.clone())
            .await
            .expect("catalog scope should load")
            .as_deref(),
        Some("repo")
    );
    assert!(
        store
            .catalog
            .active_repository_for_scope(source_scope.clone())
            .await
            .expect("active catalog scope should load")
            .is_none(),
        "failed combined publication must leave the route staged"
    );
    let current_fence = publication_fence(&second, "worker-new");
    let target = CodeIndexPublicationTarget {
        task_id: second.task_id.clone(),
        repository_id: second.repository_id.clone(),
        source_scope: second.source_scope.clone(),
        resolved_commit_sha: second.resolved_commit_sha.clone(),
        tree_hash: second.tree_hash.clone(),
        path_filters: second.path_filters.clone(),
        language_filters: second.language_filters.clone(),
    };
    let reconciled = store
        .reconcile_code_index_publication_with_fence(target, current_fence)
        .await
        .expect("current shard writer should reconcile its already committed scope");
    assert!(
        reconciled,
        "current writer should adopt the fenced shard publication"
    );

    let status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("status should load")
        .expect("repository should exist");
    assert_eq!(
        status.last_indexed_scope_id.as_deref(),
        Some(source_scope.as_str())
    );
}

#[tokio::test]
async fn staged_replacement_keeps_the_previous_active_scope_queryable() {
    let store = partitioned_store("staged-keeps-old-readable");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    super::seed_snapshot_for_test(&store, snapshot("scope-old-readable"))
        .await
        .expect("old scope should publish");
    let mut replacement = snapshot("scope-new-staged");
    replacement.resolved_commit_sha = "commit-new".to_owned();
    replacement.tree_hash = "tree-new".to_owned();
    let mut seed = task_seed("scope-new-staged");
    seed.resolved_commit_sha = replacement.resolved_commit_sha.clone();
    seed.tree_hash = replacement.tree_hash.clone();
    let queued = store
        .queue_code_index_task(seed)
        .await
        .expect("replacement task should queue");
    let claimed = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("replacement task should claim")
        .expect("replacement task should be available");

    let fence = publication_fence(&claimed, "worker");
    let session = session_from_snapshot(&replacement);
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("replacement checkpoint should begin");
    store
        .apply_code_index_batch_with_fence(batch_from_snapshot(replacement), fence.clone())
        .await
        .expect("replacement batch should commit");
    store
        .finalize_code_index_session_with_fence(session, fence.clone())
        .await
        .expect("replacement code facts should finalize");
    let shard = store
        .catalog
        .checkpoint_repository_store("repo".to_owned())
        .await
        .expect("replacement shard should resolve")
        .expect("replacement shard should exist");
    crate::storage::stage_empty_business_projection_with_fence_for_test(
        shard.as_ref(),
        claimed.repository_id.clone(),
        claimed.source_scope.clone(),
        claimed.resolved_commit_sha.clone(),
        fence.clone(),
    )
    .await
    .expect("replacement business projection should stage");
    shard
        .refresh_software_global_projection_with_fence("scope-new-staged".to_owned(), fence)
        .await
        .expect("shard publication should commit before the simulated control-plane crash");
    assert_eq!(
        store
            .code_index_checkpoint("scope-new-staged".to_owned())
            .await
            .expect("staged checkpoint should query")
            .expect("staged checkpoint should exist")
            .state,
        "finalizing:partitioned_publish"
    );
    assert_eq!(
        store
            .latest_code_index_checkpoint("repo".to_owned())
            .await
            .expect("latest staged checkpoint should query")
            .expect("latest staged checkpoint should exist")
            .state,
        "finalizing:partitioned_publish"
    );
    assert_eq!(
        store
            .catalog
            .active_repository_for_scope("scope-old-readable".to_owned())
            .await
            .expect("old active route should query")
            .as_deref(),
        Some("repo")
    );
    assert!(
        store
            .catalog
            .active_repository_for_scope("scope-new-staged".to_owned())
            .await
            .expect("new active route should query")
            .is_none()
    );
    let hits = store
        .search_code(
            CodeRetrievalRequest::new(
                "indexed contract",
                selector(),
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("retrieval request should validate"),
        )
        .await
        .expect("old published scope should remain queryable through the shard");
    assert!(hits.iter().any(|hit| hit.scope_id == "scope-old-readable"));
    assert!(hits.iter().all(|hit| hit.scope_id != "scope-new-staged"));

    let new_selector = CodeRepositorySelector::new("fixture", "commit-new", Vec::new(), Vec::new())
        .expect("new selector should validate");
    let error = store
        .search_code(
            CodeRetrievalRequest::new(
                "indexed contract",
                new_selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("new retrieval request should validate"),
        )
        .await
        .expect_err("new scope must remain hidden until the control route activates");
    assert!(is_missing_code_scope_error(&error));

    let report = store
        .code_repository_report("fixture".to_owned())
        .await
        .expect("report should fall back to the control publication");
    assert_eq!(report.resolved_commit_sha.as_deref(), Some("commit"));
    assert_eq!(report.tree_hash.as_deref(), Some("tree"));
}

#[tokio::test]
async fn same_target_worktree_task_adopts_active_partition_without_reprojection() {
    let store = partitioned_store("same-target-worktree-adoption");
    let source_scope = "scope-worktree-already-published";
    let resolved_worktree = "worktree:base:0123456789abcdef";
    let worktree_hash = "worktree:0123456789abcdef";
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let mut base = snapshot("scope-worktree-base");
    base.resolved_commit_sha = "base".to_owned();
    base.tree_hash = "tree-base".to_owned();
    super::seed_snapshot_for_test(&store, base)
        .await
        .expect("worktree base scope should publish");
    let mut published = snapshot(source_scope);
    published.base_resolved_commit_sha = Some("base".to_owned());
    published.resolved_commit_sha = resolved_worktree.to_owned();
    published.tree_hash = worktree_hash.to_owned();
    super::seed_snapshot_for_test(&store, published)
        .await
        .expect("worktree target code should publish");
    store
        .refresh_software_global_projection(source_scope.to_owned())
        .await
        .expect("worktree target software should publish");
    let mut seed = task_seed(source_scope);
    seed.mode = CodeIndexMode::WorktreeOverlay;
    seed.ref_selector = "base".to_owned();
    seed.resolved_commit_sha = resolved_worktree.to_owned();
    seed.tree_hash = worktree_hash.to_owned();
    seed.input_fingerprint = "same-target-worktree-new-task".to_owned();
    let queued = store
        .queue_code_index_task(seed)
        .await
        .expect("new worktree observation should queue");
    let task = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "worktree-adopter".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("worktree claim should run")
        .expect("worktree task should claim");
    let target = CodeIndexPublicationTarget {
        task_id: task.task_id.clone(),
        repository_id: task.repository_id.clone(),
        source_scope: task.source_scope.clone(),
        resolved_commit_sha: task.resolved_commit_sha.clone(),
        tree_hash: task.tree_hash.clone(),
        path_filters: task.path_filters.clone(),
        language_filters: task.language_filters.clone(),
    };

    assert!(
        store
            .reconcile_code_index_publication_with_fence(
                target,
                publication_fence(&task, "worktree-adopter"),
            )
            .await
            .expect("active partition should be adopted")
    );
    assert!(
        store
            .code_index_publication_receipt(
                task.task_id,
                task.repository_id,
                source_scope.to_owned(),
                now_millis(),
            )
            .await
            .expect("control publication receipt should load")
    );
    let status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("published status should load")
        .expect("published status should exist");
    assert_eq!(status.last_indexed_scope_id.as_deref(), Some(source_scope));
    assert!(!status.stale);
}

#[tokio::test]
async fn same_tree_commit_adoption_clears_the_previous_shard_incremental_receipt() {
    let store = partitioned_store("same-tree-commit-adoption");
    let source_scope = "scope-same-tree-active";
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let initial_snapshot = snapshot(source_scope);
    let initial_session = session_from_snapshot(&initial_snapshot);
    let initial_queued = store
        .queue_code_index_task(task_seed(source_scope))
        .await
        .expect("initial fenced full task should queue");
    let initial_task = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(initial_queued.task_id),
            lease_owner: "same-tree-publisher".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("initial fenced full claim should run")
        .expect("initial fenced full task should claim");
    let initial_fence = publication_fence(&initial_task, "same-tree-publisher");
    store
        .begin_code_index_session_with_fence(initial_session.clone(), initial_fence.clone())
        .await
        .expect("initial fenced full session should begin");
    store
        .apply_code_index_batch_with_fence(
            batch_from_snapshot(initial_snapshot),
            initial_fence.clone(),
        )
        .await
        .expect("initial fenced full batch should persist");
    store
        .finalize_code_index_session_with_fence(initial_session, initial_fence.clone())
        .await
        .expect("initial fenced full facts should finalize");
    crate::storage::stage_empty_business_projection_with_fence_for_test(
        &store,
        initial_task.repository_id.clone(),
        source_scope.to_owned(),
        initial_task.resolved_commit_sha.clone(),
        initial_fence.clone(),
    )
    .await
    .expect("initial business projection should stage");
    store
        .refresh_software_global_projection_with_fence(source_scope.to_owned(), initial_fence)
        .await
        .expect("initial fenced full publication should complete");
    let shard = store
        .catalog
        .checkpoint_repository_store("repo".to_owned())
        .await
        .expect("published shard should resolve")
        .expect("published shard should exist");
    let initial_task_id = initial_task.task_id.clone();
    let previous_receipt = CodeIncrementalSummaryReceipt {
        task_id: initial_task_id.clone(),
        base_resolved_commit_sha: "base".to_owned(),
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_path_count: 0,
        affected_path_count: 1,
        blob_read_count: 1,
        parsed_file_count: 1,
        sqlite_write_count: 1,
        degraded_file_count: 0,
        batch_count: 1,
    };
    let previous_receipt_json =
        serde_json::to_string(&previous_receipt).expect("previous receipt should encode");
    shard
        .run(move |connection| {
            let changed = connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET incremental_summary_json = ?1
                 WHERE source_scope = ?2 AND state = 'finalizing:partitioned_publish'",
                rusqlite::params![previous_receipt_json, source_scope],
            )?;
            if changed != 1 {
                return Err(StorageError::Invariant(
                    "raw shard receipt fixture did not update exactly once".to_owned(),
                ));
            }
            Ok(())
        })
        .await
        .expect("previous shard receipt should persist");
    store
        .complete_code_index_task(CodeIndexTaskCompletion {
            task_id: initial_task_id,
            lease_owner: "same-tree-publisher".to_owned(),
            attempt_count: initial_task.attempt_count,
            publication_generation: initial_task.publication_generation,
            now_ms: now_millis(),
        })
        .await
        .expect("initial fenced full task should complete");
    assert_eq!(
        store
            .code_index_checkpoint(source_scope.to_owned())
            .await
            .expect("projected published checkpoint should load")
            .expect("projected published checkpoint should exist")
            .state,
        "completed"
    );
    assert_eq!(
        shard
            .code_index_checkpoint(source_scope.to_owned())
            .await
            .expect("raw published checkpoint should load")
            .expect("raw published checkpoint should exist")
            .state,
        "finalizing:partitioned_publish"
    );

    let mut seed = task_seed(source_scope);
    seed.resolved_commit_sha = "commit-empty".to_owned();
    seed.input_fingerprint = "same-tree-empty-commit".to_owned();
    let queued = store
        .queue_code_index_task(seed)
        .await
        .expect("same-tree commit task should queue");
    let task = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "same-tree-adopter".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("same-tree claim should run")
        .expect("same-tree task should claim");
    let target = CodeIndexPublicationTarget {
        task_id: task.task_id.clone(),
        repository_id: task.repository_id.clone(),
        source_scope: task.source_scope.clone(),
        resolved_commit_sha: task.resolved_commit_sha.clone(),
        tree_hash: task.tree_hash.clone(),
        path_filters: task.path_filters.clone(),
        language_filters: task.language_filters.clone(),
    };

    assert!(
        store
            .reconcile_code_index_publication_with_fence(
                target,
                publication_fence(&task, "same-tree-adopter"),
            )
            .await
            .expect("same-tree partition should adopt the new commit alias")
    );
    assert!(
        store
            .code_index_publication_receipt(
                task.task_id,
                task.repository_id,
                source_scope.to_owned(),
                now_millis(),
            )
            .await
            .expect("same-tree control receipt should load")
    );
    let current = store
        .code_repository_scope_status(
            "repo".to_owned(),
            "commit-empty".to_owned(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("new commit status should query")
        .expect("new commit alias should resolve");
    let previous = store
        .code_repository_scope_status(
            "repo".to_owned(),
            "commit".to_owned(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("previous commit status should query")
        .expect("previous commit alias should remain queryable");
    assert_eq!(current.last_indexed_scope_id.as_deref(), Some(source_scope));
    assert_eq!(
        previous.last_indexed_scope_id.as_deref(),
        Some(source_scope)
    );
    assert_eq!(current.indexed_file_count, previous.indexed_file_count);
    assert_eq!(current.symbol_count, previous.symbol_count);
    let raw_checkpoint = shard
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("raw adopted checkpoint should load")
        .expect("raw adopted checkpoint should exist");
    assert_eq!(raw_checkpoint.resolved_commit_sha, "commit-empty");
    assert_eq!(raw_checkpoint.state, "finalizing:partitioned_publish");
    assert!(
        raw_checkpoint.incremental_summary.is_none(),
        "T2 adoption must atomically clear T1's raw shard receipt"
    );
    assert_eq!(
        store
            .code_index_checkpoint(source_scope.to_owned())
            .await
            .expect("projected adopted checkpoint should load")
            .expect("projected adopted checkpoint should exist")
            .state,
        "completed"
    );
}

#[tokio::test]
async fn exact_completed_content_checkpoint_restarts_a_retained_partition_for_a_new_commit() {
    let store = partitioned_store("retained-content-checkpoint-restart");
    let retained_scope = "scope-retained-content-restart";
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let retained_snapshot = snapshot(retained_scope);
    let retained_session = session_from_snapshot(&retained_snapshot);
    let retained_queued = store
        .queue_code_index_task(task_seed(retained_scope))
        .await
        .expect("retained checkpoint task should queue");
    let retained_claimed = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(retained_queued.task_id),
            lease_owner: "retained-content-worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("retained checkpoint task should claim")
        .expect("retained checkpoint task should be available");
    let retained_fence = publication_fence(&retained_claimed, "retained-content-worker");
    store
        .begin_code_index_session_with_fence(retained_session.clone(), retained_fence.clone())
        .await
        .expect("retained checkpointed session should begin");
    store
        .apply_code_index_batch_with_fence(
            batch_from_snapshot(retained_snapshot),
            retained_fence.clone(),
        )
        .await
        .expect("retained checkpointed batch should persist");
    store
        .finalize_code_index_session_with_fence(retained_session.clone(), retained_fence.clone())
        .await
        .expect("retained checkpointed session should complete");
    crate::storage::stage_empty_business_projection_with_fence_for_test(
        &store,
        retained_claimed.repository_id.clone(),
        retained_scope.to_owned(),
        retained_claimed.resolved_commit_sha.clone(),
        retained_fence.clone(),
    )
    .await
    .expect("retained business projection should stage");
    store
        .refresh_software_global_projection_with_fence(retained_scope.to_owned(), retained_fence)
        .await
        .expect("retained scope should publish");
    let retained_shard = store
        .catalog
        .checkpoint_repository_store("repo".to_owned())
        .await
        .expect("retained shard should resolve")
        .expect("retained shard should exist");
    assert_eq!(
        retained_shard
            .code_index_checkpoint(retained_scope.to_owned())
            .await
            .expect("raw retained checkpoint should load")
            .expect("raw retained checkpoint should exist")
            .state,
        "finalizing:partitioned_publish"
    );
    store
        .complete_code_index_task(CodeIndexTaskCompletion {
            task_id: retained_claimed.task_id,
            lease_owner: "retained-content-worker".to_owned(),
            attempt_count: retained_claimed.attempt_count,
            publication_generation: retained_claimed.publication_generation,
            now_ms: now_millis(),
        })
        .await
        .expect("retained checkpoint task should complete");
    let mut newer = snapshot("scope-newer-active");
    newer.resolved_commit_sha = "newer-commit".to_owned();
    newer.tree_hash = "newer-tree".to_owned();
    super::seed_snapshot_for_test(&store, newer)
        .await
        .expect("newer scope should index");
    store
        .refresh_software_global_projection("scope-newer-active".to_owned())
        .await
        .expect("newer scope should become current");
    let expected = store
        .code_index_checkpoint(retained_scope.to_owned())
        .await
        .expect("retained partition checkpoint should load")
        .expect("retained partition checkpoint should remain routed");
    assert_eq!(expected.state, "completed");
    let mut replacement = retained_session;
    replacement.resolved_commit_sha = "same-content-new-commit".to_owned();
    let mut seed = task_seed(retained_scope);
    seed.resolved_commit_sha = replacement.resolved_commit_sha.clone();
    seed.input_fingerprint = "retained-content-checkpoint-restart".to_owned();
    let queued = store
        .queue_code_index_task(seed)
        .await
        .expect("replacement task should queue");
    let claimed = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "content-restart-worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("replacement task should claim")
        .expect("replacement task should be available");

    let restarted = store
        .begin_code_index_session_at_checkpoint_with_fence(
            replacement.clone(),
            Some(expected),
            publication_fence(&claimed, "content-restart-worker"),
        )
        .await
        .expect("the exact projected checkpoint should restart under its shard fence");

    assert_eq!(restarted.state, "indexing");
    assert_eq!(
        restarted.resolved_commit_sha,
        replacement.resolved_commit_sha
    );
    assert_eq!(restarted.parsed_file_count, 0);
    assert_eq!(restarted.committed_file_count, 0);
    assert_eq!(restarted.committed_symbol_count, 0);
    assert_eq!(restarted.committed_reference_count, 0);
    assert_eq!(restarted.committed_chunk_count, 0);
    assert_eq!(restarted.batch_count, 0);
    assert!(restarted.last_path.is_none());
    assert_eq!(
        store
            .code_index_checkpoint(retained_scope.to_owned())
            .await
            .expect("restarted checkpoint should query")
            .expect("restarted checkpoint should exist"),
        restarted
    );
}

#[tokio::test]
async fn partitioned_partial_commit_mismatch_remains_an_invariant() {
    let store = partitioned_store("partial-content-checkpoint-mismatch");
    let source_scope = "scope-partial-content-mismatch";
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let staged_snapshot = snapshot(source_scope);
    let session = session_from_snapshot(&staged_snapshot);
    super::seed_session_for_test(&store, session.clone())
        .await
        .expect("partial partitioned session should begin");
    let expected = super::seed_batch_for_test(&store, batch_from_snapshot(staged_snapshot))
        .await
        .expect("partial partitioned checkpoint should persist");
    let mut mismatched = session;
    mismatched.resolved_commit_sha = "different-partial-commit".to_owned();

    let error =
        super::seed_session_at_checkpoint_for_test(&store, mismatched, Some(expected.clone()))
            .await
            .expect_err("partial progress from another commit must not restart");

    assert!(matches!(error, StorageError::Invariant(_)));
    assert_eq!(
        store
            .code_index_checkpoint(source_scope.to_owned())
            .await
            .expect("partial checkpoint should query")
            .expect("partial checkpoint should remain durable"),
        expected
    );
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

pub(super) fn registration() -> CodeRepositoryRegistration {
    CodeRepositoryRegistration::new("repo", "fixture", "/tmp/fixture", Vec::new(), Vec::new())
        .expect("registration should validate")
}

fn selector() -> CodeRepositorySelector {
    CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate")
}

pub(super) fn task_seed(source_scope: &str) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        source_scope: source_scope.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        mode: CodeIndexMode::Full,
        input_fingerprint: "partitioned-contract".to_owned(),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        now_ms: 1,
    }
}

pub(super) fn publication_fence(
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

pub(super) fn snapshot(source_scope: &str) -> CodeIndexSnapshot {
    let file = RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        file_id: "file".to_owned(),
        path: "src/lib.rs".to_owned(),
        language_id: "rust".to_owned(),
        blob_hash: "hash".to_owned(),
        byte_len: 16,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    };
    let chunk = RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        chunk_id: "chunk".to_owned(),
        file_id: file.file_id.clone(),
        path: file.path.clone(),
        language_id: file.language_id.clone(),
        content: "indexed contract".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 16 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: None,
    };

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: vec![chunk],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

pub(super) fn session_from_snapshot(snapshot: &CodeIndexSnapshot) -> CodeIndexSession {
    CodeIndexSession {
        repository_id: snapshot.repository_id.clone(),
        source_scope: snapshot.source_scope.clone(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: snapshot.resolved_commit_sha.clone(),
        tree_hash: snapshot.tree_hash.clone(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count: 1,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        changed_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget: CodeIndexResourceBudget::default(),
    }
}

pub(super) fn batch_from_snapshot(snapshot: CodeIndexSnapshot) -> CodeIndexBatch {
    CodeIndexBatch {
        repository_id: snapshot.repository_id,
        source_scope: snapshot.source_scope,
        batch_index: 1,
        parsed_byte_count: snapshot.files.iter().map(|file| file.byte_len).sum(),
        files: snapshot.files,
        symbols: snapshot.symbols,
        references: snapshot.references,
        imports: snapshot.imports,
        dependencies: snapshot.dependencies,
        feature_flags: snapshot.feature_flags,
        framework_nodes: snapshot.framework_nodes,
        framework_edges: snapshot.framework_edges,
        routes: snapshot.routes,
        chunks: snapshot.chunks,
        diagnostics: snapshot.diagnostics,
    }
}

pub(super) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
