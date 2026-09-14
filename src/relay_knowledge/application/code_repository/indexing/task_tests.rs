use super::*;
use crate::{
    domain::{
        CodeIndexMode, CodeIndexSnapshot, CodeRepositoryRegistration, code_snapshot_scope_id,
    },
    storage::{
        CodeIndexPublicationStore as _, CodeIndexTaskClaimRequest, CodeIndexTaskSeed,
        CodeIndexTaskStore as _, KnowledgeStore, RepositoryCatalogStore as _, SqliteGraphStore,
    },
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[tokio::test]
async fn worktree_recovery_reloads_the_durably_rebound_task_scope() {
    let sqlite = SqliteGraphStore::open_in_memory().expect("store should open");
    sqlite
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo-recovery-rebind",
                "recovery-rebind",
                "/tmp/recovery-rebind",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    let base_commit = "base-commit";
    let base_tree = "base-tree";
    let base_scope = code_snapshot_scope_id("repo-recovery-rebind", base_tree, &[], &[]);
    let base_snapshot = CodeIndexSnapshot {
        repository_id: "repo-recovery-rebind".to_owned(),
        source_scope: base_scope,
        base_resolved_commit_sha: None,
        resolved_commit_sha: base_commit.to_owned(),
        tree_hash: base_tree.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
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
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    };
    sqlite
        .apply_code_index_snapshot(base_snapshot.clone())
        .await
        .expect("base snapshot should publish");

    let pending_tree = format!("worktree:pending:{base_commit}");
    let pending_scope = code_snapshot_scope_id("repo-recovery-rebind", &pending_tree, &[], &[]);
    let queued = sqlite
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: "repo-recovery-rebind".to_owned(),
            alias: "recovery-rebind".to_owned(),
            ref_selector: base_commit.to_owned(),
            resolved_commit_sha: pending_tree.clone(),
            tree_hash: pending_tree,
            source_scope: pending_scope,
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::WorktreeOverlay,
            input_fingerprint: "recovery-rebind".to_owned(),
            resource_budget: crate::domain::CodeIndexResourceBudget::default(),
            payload_json: "{}".to_owned(),
            now_ms: now_millis(),
        })
        .await
        .expect("worktree task should queue");
    let running = sqlite
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "worker-recovery".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("worktree task should claim")
        .expect("worktree task should be available");
    let stale_lease = lease_from_task(&running, "worker-recovery");

    let overlay_hash = "0123456789abcdef";
    let actual_tree = format!("worktree:{overlay_hash}");
    let actual_commit = format!("worktree:{base_commit}:{overlay_hash}");
    let actual_scope = code_snapshot_scope_id("repo-recovery-rebind", &actual_tree, &[], &[]);
    let mut overlay = base_snapshot;
    overlay.source_scope = actual_scope.clone();
    overlay.base_resolved_commit_sha = Some(base_commit.to_owned());
    overlay.resolved_commit_sha = actual_commit.clone();
    overlay.tree_hash = actual_tree.clone();
    overlay.full_replace = false;
    overlay.changed_path_count = 1;
    sqlite
        .apply_code_index_snapshot_with_fence(overlay, stale_lease.publication_fence.clone())
        .await
        .expect("verified overlay should durably rebind its task");

    let store: Arc<dyn KnowledgeStore> = Arc::new(sqlite);
    let restored = restore_rebound_worktree_task_lease(
        &store,
        &CodeIndexMode::WorktreeOverlay,
        Some(stale_lease.clone()),
    )
    .await
    .expect("recovery should reload the rebound task")
    .expect("leased recovery should retain its attempt");
    assert_eq!(restored.source_scope, actual_scope);
    assert_eq!(restored.resolved_commit_sha, actual_commit);
    assert_eq!(restored.tree_hash, actual_tree);
    assert_eq!(restored.publication_fence, stale_lease.publication_fence);

    assert!(
        restore_rebound_worktree_task_lease(&store, &CodeIndexMode::WorktreeOverlay, None)
            .await
            .expect("an unleased request should bypass task recovery")
            .is_none()
    );
    let clean_lease = restore_rebound_worktree_task_lease(
        &store,
        &CodeIndexMode::Full,
        Some(stale_lease.clone()),
    )
    .await
    .expect("a clean task should keep its claimed identity")
    .expect("the clean lease should remain present");
    assert_eq!(clean_lease.source_scope, stale_lease.source_scope);

    let mut missing_task = stale_lease.clone();
    missing_task.task_id = "missing-task".to_owned();
    let missing_error = restore_rebound_worktree_task_lease(
        &store,
        &CodeIndexMode::WorktreeOverlay,
        Some(missing_task),
    )
    .await
    .expect_err("a vanished task must fail recovery closed");
    assert!(
        missing_error
            .message
            .contains("disappeared before recovery")
    );

    let mut stale_attempt = stale_lease;
    stale_attempt.publication_fence.generation += 1;
    let error = restore_rebound_worktree_task_lease(
        &store,
        &CodeIndexMode::WorktreeOverlay,
        Some(stale_attempt),
    )
    .await
    .expect_err("recovery must reject a different fenced attempt");
    assert!(error.message.contains("changed attempt identity"));
}

fn lease_from_task(
    task: &crate::domain::CodeIndexTaskRecord,
    lease_owner: &str,
) -> CodeIndexTaskLeaseContext {
    CodeIndexTaskLeaseContext {
        task_id: task.task_id.clone(),
        lease_owner: lease_owner.to_owned(),
        attempt_count: task.attempt_count,
        lease_duration_ms: 60_000,
        publication_fence: crate::domain::CodeIndexPublicationFence {
            repository_id: task.repository_id.clone(),
            task_id: task.task_id.clone(),
            lease_owner: lease_owner.to_owned(),
            attempt_count: task.attempt_count,
            generation: task.publication_generation,
        },
        source_scope: task.source_scope.clone(),
        resolved_commit_sha: task.resolved_commit_sha.clone(),
        tree_hash: task.tree_hash.clone(),
        path_filters: task.path_filters.clone(),
        language_filters: task.language_filters.clone(),
        resource_budget: task.resource_budget,
    }
}

#[test]
fn recognizes_only_default_optional_code_index_lease_unavailable_errors() {
    assert!(storage_error_message_is(
        &StorageError::InvalidInput(CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE.to_owned()),
        CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
    ));
    assert!(storage_error_message_is(
        &StorageError::InvalidInput(CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE.to_owned()),
        CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE,
    ));
    assert!(!storage_error_message_is(
        &StorageError::InvalidInput("code index task lease expired".to_owned()),
        CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
    ));
}

#[test]
fn code_index_worker_pid_parses_only_owned_worker_leases() {
    assert_eq!(code_index_worker_pid("code-index-worker-123"), Some(123));
    assert_eq!(code_index_worker_pid("worker-123"), None);
    assert_eq!(code_index_worker_pid("code-index-worker-"), None);
    assert_eq!(code_index_worker_pid("code-index-worker-pid"), None);
}

#[test]
fn current_process_is_treated_as_running() {
    assert!(process_is_running(
        std::process::id(),
        std::path::Path::new("tasklist.exe")
    ));
}

#[tokio::test]
async fn code_index_task_finalization_driver_renews_before_and_after_every_step() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let steps = Arc::new(Mutex::new(VecDeque::from([
        Ok(CodeIndexFinalizationStep::Pending {
            checkpoint_state: "phase-1".to_owned(),
        }),
        Ok(CodeIndexFinalizationStep::Ready(Box::new(test_summary()))),
    ])));

    let summary = drive_code_index_finalization(
        "scope",
        "indexing".to_owned(),
        CODE_INDEX_FINALIZATION_MAX_STEPS,
        {
            let events = Arc::clone(&events);
            move || {
                events.lock().expect("events should lock").push("renew");
                async { Ok(()) }
            }
        },
        {
            let events = Arc::clone(&events);
            let steps = Arc::clone(&steps);
            move || {
                events.lock().expect("events should lock").push("advance");
                let step = steps
                    .lock()
                    .expect("steps should lock")
                    .pop_front()
                    .expect("a step should remain");
                async move { step }
            }
        },
    )
    .await
    .expect("bounded steps should complete");

    assert_eq!(summary.source_scope, "scope");
    assert_eq!(
        *events.lock().expect("events should lock"),
        ["renew", "advance", "renew", "renew", "advance", "renew"]
    );
}

#[tokio::test]
async fn reloaded_finalization_bound_covers_pages_added_after_zero_count_begin() {
    let pending_page_count = 40usize;
    let session = finalization_session();
    let begin_checkpoint = finalization_checkpoint(&session, 0, 0);
    let latest_checkpoint = finalization_checkpoint(&session, 1, 17);
    assert_eq!(begin_checkpoint.committed_reference_count, 0);
    let latest_checkpoint =
        require_leased_finalization_checkpoint(&session, Some(latest_checkpoint))
            .expect("the post-writer checkpoint should be complete and retain its identity");
    let mut queued_steps = (1..=pending_page_count)
        .map(|page| CodeIndexFinalizationStep::Pending {
            checkpoint_state: format!("reference-page-{page}"),
        })
        .collect::<VecDeque<_>>();
    queued_steps.push_back(CodeIndexFinalizationStep::Ready(Box::new(test_summary())));
    let steps = Arc::new(Mutex::new(queued_steps));
    let events = Arc::new(Mutex::new(Vec::new()));
    let begin_max_steps =
        code_index_finalization_max_steps(begin_checkpoint.committed_reference_count)
            .expect("zero-reference begin checkpoint should retain a finite bound");
    assert!(pending_page_count + 1 > begin_max_steps);
    let max_steps = code_index_finalization_max_steps(latest_checkpoint.committed_reference_count)
        .expect("reference count should derive a bound above thirty pages");

    let summary = drive_code_index_finalization(
        "scope",
        latest_checkpoint.state,
        max_steps,
        {
            let events = Arc::clone(&events);
            move || {
                events.lock().expect("events should lock").push("renew");
                async { Ok(()) }
            }
        },
        {
            let events = Arc::clone(&events);
            let steps = Arc::clone(&steps);
            move || {
                events.lock().expect("events should lock").push("advance");
                let step = steps
                    .lock()
                    .expect("steps should lock")
                    .pop_front()
                    .expect("a derived-bound step should remain");
                async move { Ok(step) }
            }
        },
    )
    .await
    .expect("derived bound must not retain the former thirty-step failure");

    assert_eq!(summary.source_scope, "scope");
    assert!(steps.lock().expect("steps should lock").is_empty());
    let events = events.lock().expect("events should lock");
    assert_eq!(events.len(), (pending_page_count + 1) * 3);
    assert!(
        events
            .chunks_exact(3)
            .all(|chunk| chunk == ["renew", "advance", "renew"])
    );
}

#[test]
fn leased_finalization_checkpoint_rejects_missing_identity_and_prefix_drift() {
    let session = finalization_session();
    let missing = require_leased_finalization_checkpoint(&session, None)
        .expect_err("a vanished post-writer checkpoint must fail closed");
    assert!(missing.to_string().contains("disappeared"));

    let mut identity_drift = finalization_checkpoint(&session, 1, 17);
    identity_drift.tree_hash = "different-tree".to_owned();
    let identity_error = require_leased_finalization_checkpoint(&session, Some(identity_drift))
        .expect_err("a reloaded checkpoint with another identity must fail closed");
    assert!(identity_error.to_string().contains("identity"));

    let incomplete = finalization_checkpoint(&session, 0, 17);
    let incomplete_error = require_leased_finalization_checkpoint(&session, Some(incomplete))
        .expect_err("finalization must require the complete committed prefix");
    assert!(incomplete_error.to_string().contains("incomplete"));
}

#[tokio::test]
async fn code_index_task_finalization_driver_stops_on_boundary_failures() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let renewal_error = drive_code_index_finalization(
        "scope",
        "indexing".to_owned(),
        CODE_INDEX_FINALIZATION_MAX_STEPS,
        {
            let events = Arc::clone(&events);
            move || {
                events.lock().expect("events should lock").push("renew");
                async { Err(ApiError::storage_unavailable("renew failed")) }
            }
        },
        {
            let events = Arc::clone(&events);
            move || {
                events.lock().expect("events should lock").push("advance");
                async { Ok(CodeIndexFinalizationStep::Ready(Box::new(test_summary()))) }
            }
        },
    )
    .await
    .expect_err("failed pre-step renewal must stop the driver");
    assert!(renewal_error.message.contains("renew failed"));
    assert_eq!(*events.lock().expect("events should lock"), ["renew"]);

    let events = Arc::new(Mutex::new(Vec::new()));
    let advance_error = drive_code_index_finalization(
        "scope",
        "indexing".to_owned(),
        CODE_INDEX_FINALIZATION_MAX_STEPS,
        {
            let events = Arc::clone(&events);
            move || {
                events.lock().expect("events should lock").push("renew");
                async { Ok(()) }
            }
        },
        {
            let events = Arc::clone(&events);
            move || {
                events.lock().expect("events should lock").push("advance");
                async { Err(ApiError::storage_unavailable("advance failed")) }
            }
        },
    )
    .await
    .expect_err("failed advance must stop before post-step renewal");
    assert!(advance_error.message.contains("advance failed"));
    assert_eq!(
        *events.lock().expect("events should lock"),
        ["renew", "advance"]
    );

    let events = Arc::new(Mutex::new(Vec::new()));
    let renewal_count = Arc::new(Mutex::new(0usize));
    let post_renewal_error = drive_code_index_finalization(
        "scope",
        "indexing".to_owned(),
        CODE_INDEX_FINALIZATION_MAX_STEPS,
        {
            let events = Arc::clone(&events);
            let renewal_count = Arc::clone(&renewal_count);
            move || {
                events.lock().expect("events should lock").push("renew");
                let mut count = renewal_count.lock().expect("count should lock");
                *count += 1;
                let result = if *count == 1 {
                    Ok(())
                } else {
                    Err(ApiError::storage_unavailable("post renew failed"))
                };
                async move { result }
            }
        },
        {
            let events = Arc::clone(&events);
            move || {
                events.lock().expect("events should lock").push("advance");
                async { Ok(CodeIndexFinalizationStep::Ready(Box::new(test_summary()))) }
            }
        },
    )
    .await
    .expect_err("failed post-step renewal must reject Ready and stop");
    assert!(post_renewal_error.message.contains("post renew failed"));
    assert_eq!(
        *events.lock().expect("events should lock"),
        ["renew", "advance", "renew"]
    );

    let no_progress = drive_code_index_finalization(
        "scope",
        "indexing".to_owned(),
        CODE_INDEX_FINALIZATION_MAX_STEPS,
        || async { Ok(()) },
        || async {
            Ok(CodeIndexFinalizationStep::Pending {
                checkpoint_state: "indexing".to_owned(),
            })
        },
    )
    .await
    .expect_err("a Pending step must change the durable state");
    assert!(no_progress.message.contains("did not advance"));
}

#[tokio::test]
async fn code_index_task_finalization_driver_enforces_its_derived_step_bound() {
    let next = Arc::new(Mutex::new(0usize));
    let error = drive_code_index_finalization(
        "scope",
        "indexing".to_owned(),
        CODE_INDEX_FINALIZATION_MAX_STEPS,
        || async { Ok(()) },
        {
            let next = Arc::clone(&next);
            move || {
                let mut next = next.lock().expect("counter should lock");
                *next += 1;
                let state = format!("phase-{}", *next);
                async move {
                    Ok(CodeIndexFinalizationStep::Pending {
                        checkpoint_state: state,
                    })
                }
            }
        },
    )
    .await
    .expect_err("too many distinct steps must remain bounded");

    assert!(error.message.contains("exceeded its durable step bound"));
    assert_eq!(
        *next.lock().expect("counter should lock"),
        CODE_INDEX_FINALIZATION_MAX_STEPS
    );
}

#[test]
fn finalization_step_bound_covers_worst_case_preserve_resume_repair() {
    let repair_creates = crate::domain::CODE_QUERY_INDEX_PLAN_UNIT_COUNT;
    let restore_original_coarse_state = 1;
    let remaining_coarse_and_terminal_observation =
        crate::storage::CODE_INDEX_FINALIZATION_COARSE_PHASE_COUNT + 1;

    assert!(
        repair_creates + restore_original_coarse_state + remaining_coarse_and_terminal_observation
            <= CODE_INDEX_FINALIZATION_MAX_STEPS
    );
}

#[test]
fn finalization_step_bound_derives_worst_case_byte_limited_reference_pages() {
    let reference_count = 17usize;
    let derived = code_index_finalization_max_steps(reference_count)
        .expect("bounded reference count should derive a step cap");

    assert_eq!(
        derived,
        CODE_INDEX_FINALIZATION_MAX_STEPS + reference_count * 4 + 6
    );
}

fn test_summary() -> CodeIndexSummary {
    CodeIndexSummary {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        indexed_file_count: 0,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_path_count: 0,
        symbol_count: 0,
        handwritten_symbol_count: 0,
        generated_symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        degraded_file_count: 0,
        progress: crate::domain::CodeIndexProgressSummary::default(),
    }
}

fn finalization_session() -> CodeIndexSession {
    CodeIndexSession {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        full_replace: true,
        total_path_count: 1,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        changed_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget: crate::domain::CodeIndexResourceBudget::default(),
    }
}

fn finalization_checkpoint(
    session: &CodeIndexSession,
    committed_file_count: usize,
    committed_reference_count: usize,
) -> CodeIndexCheckpoint {
    CodeIndexCheckpoint {
        repository_id: session.repository_id.clone(),
        source_scope: session.source_scope.clone(),
        resolved_commit_sha: session.resolved_commit_sha.clone(),
        tree_hash: session.tree_hash.clone(),
        path_filters: session.path_filters.clone(),
        language_filters: session.language_filters.clone(),
        state: "indexing".to_owned(),
        total_path_count: session.total_path_count,
        parsed_file_count: committed_file_count,
        committed_file_count,
        committed_symbol_count: 0,
        committed_reference_count,
        committed_chunk_count: 0,
        committed_fact_row_count: committed_file_count.saturating_add(committed_reference_count),
        incremental_summary: None,
        batch_count: usize::from(committed_file_count > 0),
        last_path: (committed_file_count > 0).then(|| "src/lib.rs".to_owned()),
        resource_budget: session.resource_budget,
        updated_at_ms: 1,
    }
}
