use std::{path::PathBuf, sync::Arc};

use super::super::retention::RETAIN_FAILED_TASK_AUDIT_ROWS;
use super::{
    MAX_SCOPE_SLOTS_PER_REPOSITORY, MAX_UNFINISHED_TASKS_GLOBAL,
    MAX_UNFINISHED_TASKS_PER_REPOSITORY, queue_task,
};
use crate::{
    domain::{
        CodeIndexMode, CodeIndexResourceBudget, CodeIndexTaskState, CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskSeed, CodeRepositoryStore,
        CodeScopeRetentionRequest, SqliteGraphStore, StorageError,
    },
};
use tokio::sync::Barrier;

#[tokio::test]
async fn queue_reuses_unfinished_fingerprint_and_keeps_distinct_work_independent() {
    let store = registered_store().await;
    let first = store
        .run(|connection| queue_task(connection, seed("fp-a", "scope-a", 100)))
        .await
        .expect("task should queue");
    let duplicate = store
        .run(|connection| queue_task(connection, seed("fp-a", "scope-a", 101)))
        .await
        .expect("unfinished fingerprint should reuse task");
    let distinct = store
        .run(|connection| queue_task(connection, seed("fp-b", "scope-b", 101)))
        .await
        .expect("distinct fingerprint should queue");

    assert_eq!(first.task_id, duplicate.task_id);
    assert_ne!(first.task_id, distinct.task_id);
    assert_eq!(first.state, CodeIndexTaskState::Queued);
    assert_eq!(first.path_filters, ["src"]);
    assert_eq!(first.language_filters, ["rust"]);
    assert_eq!(first.mode, CodeIndexMode::Full);
}

#[tokio::test]
async fn queue_does_not_reopen_identical_dead_letter_work() {
    let store = registered_store().await;
    let first = store
        .run(|connection| {
            queue_task(
                connection,
                seed_with_payload("git-ref", "scope-a", 100, "old"),
            )
        })
        .await
        .expect("task should queue");
    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repository_index_tasks SET state = 'dead_letter' WHERE task_id = ?1",
                [&first.task_id],
            )?;
            Ok(())
        })
        .await
        .expect("task should become dead letter");

    let unchanged = store
        .run(|connection| {
            queue_task(
                connection,
                seed_with_payload("git-ref", "scope-a", 200, "old"),
            )
        })
        .await
        .expect("same failed input should be observed");
    let changed = store
        .run(|connection| {
            queue_task(
                connection,
                seed_with_payload("git-ref", "scope-b", 300, "new"),
            )
        })
        .await
        .expect("new ref input should reopen the durable slot");

    assert_eq!(unchanged.state, CodeIndexTaskState::DeadLetter);
    assert_eq!(changed.state, CodeIndexTaskState::Queued);
    assert_eq!(changed.created_at_ms, 300);
    assert_eq!(changed.payload_json, "new");
}

#[tokio::test]
async fn periodic_worktree_reconcile_reuses_an_identical_succeeded_observation() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "base", "base-scope").await;
    let payload =
        r#"{"watcher":{"kind":"periodic_worktree_reconcile","observation_fingerprint":"abc"}}"#;
    let mut first_seed = seed_with_payload("periodic-overlay", "overlay-scope", 100, payload);
    first_seed.mode = CodeIndexMode::WorktreeOverlay;
    first_seed.ref_selector = "base".to_owned();
    first_seed.resolved_commit_sha = "worktree:pending:base".to_owned();
    let first = store
        .queue_code_index_task(first_seed.clone())
        .await
        .expect("periodic overlay should queue");
    store
        .run({
            let task_id = first.task_id.clone();
            move |connection| {
                connection.execute(
                    "UPDATE code_repository_index_tasks SET state = 'succeeded' WHERE task_id = ?1",
                    [&task_id],
                )?;
                Ok(())
            }
        })
        .await
        .expect("periodic observation should complete");
    first_seed.now_ms = 200;

    let duplicate = store
        .queue_code_index_task(first_seed)
        .await
        .expect("identical periodic observation should reuse success");

    assert_eq!(duplicate.task_id, first.task_id);
    assert_eq!(duplicate.state, CodeIndexTaskState::Succeeded);
}

#[tokio::test]
async fn code_index_task_queue_bounds_scope_backlog_until_maintenance_releases_capacity() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for index in 0..MAX_SCOPE_SLOTS_PER_REPOSITORY - 1 {
                connection.execute(
                    "INSERT INTO code_repository_scopes (
                         source_scope, repository_id, resolved_commit_sha, tree_hash,
                         path_filters_json, language_filters_json, indexed_file_count,
                         symbol_count, reference_count, chunk_count, stale,
                         degraded_reason, retiring
                     ) VALUES (?1, 'repo', ?2, ?3, '[\"src\"]', '[\"rust\"]',
                               0, 0, 0, 0, 0, NULL, 0)",
                    rusqlite::params![
                        format!("scope-{index:03}"),
                        format!("commit-{index:03}"),
                        format!("tree-{index:03}")
                    ],
                )?;
            }
            connection.execute(
                "INSERT INTO code_repository_index_checkpoints (
                     source_scope, repository_id, state, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, total_path_count,
                     parsed_file_count, committed_file_count, committed_symbol_count,
                     committed_reference_count, committed_chunk_count, batch_count,
                     last_path, resource_budget_json, updated_at_ms, error_message
                 ) VALUES (
                     'scope-partial', 'repo', 'indexing', 'commit-partial', 'tree-partial',
                     '[\"src\"]', '[\"rust\"]', 0, 0, 0, 0, 0, 0, 0,
                     NULL, '{}', 99, NULL
                 )",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("scope backlog should insert");

    store
        .queue_code_index_task(seed("existing-partial", "scope-partial", 99))
        .await
        .expect("an existing checkpoint target should reuse its occupied scope slot");

    let error = store
        .queue_code_index_task(seed("over-capacity", "scope-next", 100))
        .await
        .expect_err("new target scope should observe maintenance backpressure");
    assert!(matches!(
        error,
        StorageError::CapacityExceeded(message)
            if message.contains("managed scope maintenance")
    ));

    store
        .run(|connection| {
            connection.execute(
                "DELETE FROM code_repository_scopes WHERE source_scope = 'scope-000'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("bounded maintenance release should persist");
    store
        .queue_code_index_task(seed("released-capacity", "scope-next", 101))
        .await
        .expect("released scope capacity should admit one new target");
}

#[tokio::test]
async fn newer_worktree_observation_cancels_only_pending_overlay() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "commit-scope-running", "base-overlay-scope").await;
    let running = store
        .queue_code_index_task(pinned_worktree_seed(
            "overlay-running",
            "scope-running",
            100,
            "commit-scope-running",
        ))
        .await
        .expect("running candidate should queue");
    store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(running.task_id.clone()),
            lease_owner: "queue-test".to_owned(),
            lease_duration_ms: 1_000,
            max_attempts: 3,
            now_ms: 101,
        })
        .await
        .expect("claim should succeed")
        .expect("task should be claimable");
    let pending = store
        .queue_code_index_task(pinned_worktree_seed(
            "overlay-pending",
            "scope-pending",
            102,
            "commit-scope-running",
        ))
        .await
        .expect("pending overlay should queue");
    let newest = store
        .queue_code_index_task(pinned_worktree_seed(
            "overlay-newest",
            "scope-newest",
            103,
            "commit-scope-running",
        ))
        .await
        .expect("newest overlay should queue");

    assert_eq!(
        task_state(&store, &running.task_id).await,
        CodeIndexTaskState::Running
    );
    assert_eq!(
        task_state(&store, &pending.task_id).await,
        CodeIndexTaskState::Cancelled
    );
    assert_eq!(
        task_state(&store, &newest.task_id).await,
        CodeIndexTaskState::Queued
    );
}

#[tokio::test]
async fn code_index_task_worktree_supersession_keeps_terminal_history_bounded() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "HEAD", "base-overlay-scope").await;
    let running = store
        .queue_code_index_task(mode_seed(
            "overlay-running-bounded",
            "scope-running-bounded",
            1,
            CodeIndexMode::WorktreeOverlay,
        ))
        .await
        .expect("running overlay should queue");
    store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(running.task_id.clone()),
            lease_owner: "queue-test".to_owned(),
            lease_duration_ms: 10_000,
            max_attempts: 3,
            now_ms: 2,
        })
        .await
        .expect("claim should succeed")
        .expect("task should be claimable");
    for index in 0..600 {
        store
            .queue_code_index_task(mode_seed(
                &format!("overlay-bounded-{index}"),
                &format!("scope-bounded-{index}"),
                3 + index,
                CodeIndexMode::WorktreeOverlay,
            ))
            .await
            .expect("newer worktree observation should queue");
    }

    let (running_count, queued_count, terminal_count) = store
        .run(|connection| {
            let count = |state: &str| {
                connection.query_row(
                    "SELECT COUNT(*) FROM code_repository_index_tasks
                     WHERE repository_id = 'repo' AND state = ?1",
                    [state],
                    |row| row.get::<_, usize>(0),
                )
            };
            Ok((count("running")?, count("queued")?, count("cancelled")?))
        })
        .await
        .expect("task history counts should load");

    assert_eq!(running_count, 1);
    assert_eq!(queued_count, 1);
    assert!(terminal_count <= RETAIN_FAILED_TASK_AUDIT_ROWS);
}

#[tokio::test]
async fn incremental_commit_cancels_stale_pending_overlay() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "base", "base-scope").await;
    insert_compatible_base_scope(&store, "commit-scope-overlay", "base-overlay-stale-scope").await;
    let overlay = store
        .queue_code_index_task(pinned_worktree_seed(
            "overlay-stale",
            "scope-overlay",
            100,
            "commit-scope-overlay",
        ))
        .await
        .expect("overlay should queue");
    let incremental = store
        .queue_code_index_task(mode_seed(
            "commit-update",
            "scope-commit",
            101,
            CodeIndexMode::incremental("base", "head").expect("mode"),
        ))
        .await
        .expect("incremental should queue");

    assert_eq!(
        task_state(&store, &overlay.task_id).await,
        CodeIndexTaskState::Cancelled
    );
    assert_eq!(incremental.state, CodeIndexTaskState::Queued);
}

#[tokio::test]
async fn worktree_observation_waits_for_an_unfinished_commit_update() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "base", "base-scope").await;
    store
        .queue_code_index_task(mode_seed(
            "commit-update-first",
            "scope-commit-first",
            100,
            CodeIndexMode::incremental("base", "head").expect("mode"),
        ))
        .await
        .expect("commit update should queue");
    let mut overlay = mode_seed(
        "overlay-after-commit",
        "scope-overlay-after-commit",
        101,
        CodeIndexMode::WorktreeOverlay,
    );
    overlay.ref_selector = "base".to_owned();
    overlay.resolved_commit_sha = "worktree:pending:base".to_owned();

    let error = store
        .queue_code_index_task(overlay)
        .await
        .expect_err("old-base worktree observation must wait for commit publication");

    assert!(matches!(
        error,
        StorageError::CapacityExceeded(message)
            if message.contains("immutable commit update is unfinished")
                && message.contains("retry after the managed commit task publishes")
    ));
}

#[tokio::test]
async fn code_index_task_queue_rejects_incremental_base_after_gc_schedules_retirement() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "base-commit", "base-scope").await;
    insert_compatible_base_scope(&store, "active-commit", "active-scope").await;
    let scheduled = store
        .prune_code_repository_scopes(CodeScopeRetentionRequest {
            repository_id: "repo".to_owned(),
            active_scope: "active-scope".to_owned(),
            retain_recent_successful_scopes: 0,
            repository_retention_cutoff_ms: None,
            repository_retention_cutoff_generation: None,
            repository_retention_initial_scope: None,
        })
        .await
        .expect("retention should schedule the unpinned base");
    assert_eq!(scheduled.retiring_job_count, 1);

    let error = store
        .queue_code_index_task(mode_seed(
            "incremental-after-gc",
            "target-scope",
            2_000,
            CodeIndexMode::incremental("base-commit", "head-commit").expect("mode"),
        ))
        .await
        .expect_err("a retiring base must not be admitted after application preflight");

    assert!(matches!(
        error,
        StorageError::InvalidInput(message)
            if message.contains("base commit 'base-commit'")
                && message.contains("no compatible non-retiring scope")
                && message.contains("full index")
    ));
}

#[tokio::test]
async fn code_index_task_queue_accepts_incremental_base_through_non_retiring_commit_alias() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "newer-same-tree", "alias-base-scope").await;
    store
        .run(|connection| {
            connection.execute(
                "INSERT INTO code_repository_commit_scopes (
                     repository_id, resolved_commit_sha, source_scope, published_sequence
                 ) VALUES ('repo', 'older-base-commit', 'alias-base-scope', 1)",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("commit alias should persist");

    let queued = store
        .queue_code_index_task(mode_seed(
            "incremental-alias-base",
            "target-scope",
            2_000,
            CodeIndexMode::incremental("older-base-commit", "head-commit").expect("mode"),
        ))
        .await
        .expect("non-retiring compatible alias must satisfy base admission");

    assert_eq!(queued.state, CodeIndexTaskState::Queued);
}

#[tokio::test]
async fn code_index_task_queue_rejects_worktree_overlay_after_base_retirement() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "base-commit", "base-scope").await;
    insert_compatible_base_scope(&store, "active-commit", "active-scope").await;
    store
        .prune_code_repository_scopes(CodeScopeRetentionRequest {
            repository_id: "repo".to_owned(),
            active_scope: "active-scope".to_owned(),
            retain_recent_successful_scopes: 0,
            repository_retention_cutoff_ms: None,
            repository_retention_cutoff_generation: None,
            repository_retention_initial_scope: None,
        })
        .await
        .expect("retention should schedule the overlay base");
    let mut overlay = mode_seed(
        "overlay-after-gc",
        "overlay-target",
        2_000,
        CodeIndexMode::WorktreeOverlay,
    );
    overlay.ref_selector = "base-commit".to_owned();
    overlay.resolved_commit_sha = "worktree:pending:base-commit".to_owned();

    let error = store
        .queue_code_index_task(overlay)
        .await
        .expect_err("worktree overlay cannot pin an already-retiring clean base");

    assert!(matches!(
        error,
        StorageError::InvalidInput(message)
            if message.contains("base commit 'base-commit'")
                && message.contains("full index")
    ));
}

#[tokio::test]
async fn code_index_task_queue_pins_incremental_base_before_gc_plans_retirement() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "base-commit", "base-scope").await;
    insert_compatible_base_scope(&store, "active-commit", "active-scope").await;
    store
        .queue_code_index_task(mode_seed(
            "incremental-before-gc",
            "target-scope",
            2_000,
            CodeIndexMode::incremental("base-commit", "head-commit").expect("mode"),
        ))
        .await
        .expect("queue transaction should durably pin its compatible base");

    let retention = store
        .prune_code_repository_scopes(CodeScopeRetentionRequest {
            repository_id: "repo".to_owned(),
            active_scope: "active-scope".to_owned(),
            retain_recent_successful_scopes: 0,
            repository_retention_cutoff_ms: None,
            repository_retention_cutoff_generation: None,
            repository_retention_initial_scope: None,
        })
        .await
        .expect("retention should observe the unfinished task pin");
    let base_retiring = store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT retiring FROM code_repository_scopes WHERE source_scope = 'base-scope'",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("base retirement state should query");

    assert!(retention.retained_scopes.contains(&"base-scope".to_owned()));
    assert!(!base_retiring);
}

#[tokio::test]
async fn code_index_task_repository_capacity_reuses_duplicate_before_rejecting_new() {
    let store = registered_store().await;
    let mut first_task_id = String::new();
    for index in 0..MAX_UNFINISHED_TASKS_PER_REPOSITORY {
        insert_compatible_base_scope(
            &store,
            &format!("base-{index}"),
            &format!("base-scope-{index}"),
        )
        .await;
        let fingerprint = format!("commit-{index}");
        let task = store
            .queue_code_index_task(mode_seed(
                &fingerprint,
                &format!("scope-{index}"),
                100 + index as u64,
                incremental_mode(index),
            ))
            .await
            .expect("task within repository capacity should queue");
        if index == 0 {
            first_task_id = task.task_id;
        }
    }
    insert_compatible_base_scope(
        &store,
        &format!("base-{MAX_UNFINISHED_TASKS_PER_REPOSITORY}"),
        "base-scope-overflow",
    )
    .await;

    let duplicate = store
        .queue_code_index_task(mode_seed("commit-0", "scope-0", 1_000, incremental_mode(0)))
        .await
        .expect("identical unfinished work should bypass admission");
    let rejected = store
        .queue_code_index_task(mode_seed(
            "commit-overflow",
            "scope-overflow",
            1_001,
            incremental_mode(MAX_UNFINISHED_TASKS_PER_REPOSITORY),
        ))
        .await
        .expect_err("distinct work beyond repository capacity should be rejected");

    assert_eq!(duplicate.task_id, first_task_id);
    assert!(matches!(
        rejected,
        StorageError::CapacityExceeded(message)
            if message.contains("repository 'repo'")
                && message.contains("retry after queued work completes")
    ));
}

#[tokio::test]
async fn code_index_task_terminal_work_releases_repository_capacity() {
    let store = registered_store().await;
    let mut completed_task_id = String::new();
    for index in 0..MAX_UNFINISHED_TASKS_PER_REPOSITORY {
        let task = store
            .queue_code_index_task(seed(
                &format!("full-{index}"),
                &format!("scope-{index}"),
                100 + index as u64,
            ))
            .await
            .expect("task within repository capacity should queue");
        if index == 0 {
            completed_task_id = task.task_id;
        }
    }
    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repository_index_tasks SET state = 'succeeded' WHERE task_id = ?1",
                [&completed_task_id],
            )?;
            Ok(())
        })
        .await
        .expect("terminal transition should persist");

    let replacement = store
        .queue_code_index_task(seed("replacement", "replacement-scope", 1_000))
        .await
        .expect("terminal task should no longer consume capacity");

    assert_eq!(replacement.state, CodeIndexTaskState::Queued);
}

#[tokio::test]
async fn code_index_task_overlay_coalescing_keeps_a_stable_pending_slot_at_repository_capacity() {
    let store = registered_store().await;
    insert_compatible_base_scope(
        &store,
        "commit-overlay-stale-scope",
        "base-overlay-capacity-scope",
    )
    .await;
    for index in 0..MAX_UNFINISHED_TASKS_PER_REPOSITORY - 1 {
        store
            .queue_code_index_task(seed(
                &format!("full-{index}"),
                &format!("scope-{index}"),
                100 + index as u64,
            ))
            .await
            .expect("background work should queue");
    }
    let mut stale_seed = mode_seed(
        "overlay-stale-at-capacity",
        "overlay-stale-scope",
        500,
        CodeIndexMode::WorktreeOverlay,
    );
    stale_seed.ref_selector = "commit-overlay-stale-scope".to_owned();
    stale_seed.resolved_commit_sha = "worktree:pending:commit-overlay-stale-scope".to_owned();
    let stale = store
        .queue_code_index_task(stale_seed)
        .await
        .expect("first overlay should fill the final slot");
    let mut newest_seed = mode_seed(
        "overlay-newest-at-capacity",
        "overlay-newest-scope",
        501,
        CodeIndexMode::WorktreeOverlay,
    );
    newest_seed.ref_selector = "commit-overlay-stale-scope".to_owned();
    newest_seed.resolved_commit_sha = "worktree:pending:commit-overlay-stale-scope".to_owned();
    let newest = store
        .queue_code_index_task(newest_seed)
        .await
        .expect("new overlay should supersede before capacity admission");

    assert_eq!(
        task_state(&store, &stale.task_id).await,
        CodeIndexTaskState::Cancelled
    );
    assert_eq!(newest.state, CodeIndexTaskState::Queued);
    let status = store
        .code_index_task_queue_status()
        .await
        .expect("queue status should load");
    assert_eq!(
        status.queued_task_count,
        MAX_UNFINISHED_TASKS_PER_REPOSITORY
    );
}

#[tokio::test]
async fn code_index_task_global_capacity_bounds_unfinished_work_across_repositories() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let repository_count =
        MAX_UNFINISHED_TASKS_GLOBAL.div_ceil(MAX_UNFINISHED_TASKS_PER_REPOSITORY);
    for repository_index in 0..=repository_count {
        register_repository(
            &store,
            &format!("repo-{repository_index}"),
            &format!("fixture-{repository_index}"),
        )
        .await;
    }
    for task_index in 0..MAX_UNFINISHED_TASKS_GLOBAL {
        let repository_index = task_index / MAX_UNFINISHED_TASKS_PER_REPOSITORY;
        store
            .queue_code_index_task(repository_seed(
                &format!("repo-{repository_index}"),
                &format!("fixture-{repository_index}"),
                &format!("fingerprint-{task_index}"),
                &format!("scope-{task_index}"),
                100 + task_index as u64,
            ))
            .await
            .expect("task within global capacity should queue");
    }

    let rejected = store
        .queue_code_index_task(repository_seed(
            &format!("repo-{repository_count}"),
            &format!("fixture-{repository_count}"),
            "global-overflow",
            "global-overflow-scope",
            10_000,
        ))
        .await
        .expect_err("task beyond global capacity should be rejected");

    assert!(matches!(
        rejected,
        StorageError::CapacityExceeded(message)
            if message.contains("global code index task queue")
                && message.contains(&format!("capacity {MAX_UNFINISHED_TASKS_GLOBAL}"))
    ));
}

#[tokio::test]
async fn code_index_task_concurrent_admission_cannot_overfill_repository_capacity() {
    let database = TemporaryDatabase::new();
    let store = SqliteGraphStore::open(&database.path).expect("first store should open");
    register_repository(&store, "repo", "fixture").await;
    for index in 0..MAX_UNFINISHED_TASKS_PER_REPOSITORY - 1 {
        store
            .queue_code_index_task(seed(
                &format!("prefill-{index}"),
                &format!("prefill-scope-{index}"),
                100 + index as u64,
            ))
            .await
            .expect("prefill task should queue");
    }
    let concurrent_store =
        SqliteGraphStore::open(&database.path).expect("second store should open");
    let barrier = Arc::new(Barrier::new(3));
    let first = spawn_racing_enqueue(store.clone(), Arc::clone(&barrier), "race-a", 1_000);
    let second = spawn_racing_enqueue(concurrent_store, Arc::clone(&barrier), "race-b", 1_001);
    barrier.wait().await;
    let (first, second) = tokio::join!(first, second);
    let results = [
        first.expect("first enqueue task should join"),
        second.expect("second enqueue task should join"),
    ];

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| match result {
                Err(StorageError::CapacityExceeded(message)) => {
                    assert!(message.contains("repository 'repo'"));
                    true
                }
                _ => false,
            })
            .count(),
        1
    );
    let status = store
        .code_index_task_queue_status()
        .await
        .expect("queue status should load");
    assert_eq!(
        status.queued_task_count,
        MAX_UNFINISHED_TASKS_PER_REPOSITORY
    );
}

struct TemporaryDatabase {
    path: PathBuf,
}

impl TemporaryDatabase {
    fn new() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should follow Unix epoch")
            .as_nanos();
        Self {
            path: std::env::temp_dir().join(format!(
                "relay-knowledge-code-index-queue-{}-{unique}.sqlite",
                std::process::id()
            )),
        }
    }
}

impl Drop for TemporaryDatabase {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.path.as_os_str().to_os_string();
            path.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(path));
        }
    }
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    register_repository(&store, "repo", "fixture").await;
    store
}

async fn register_repository(store: &SqliteGraphStore, repository_id: &str, alias: &str) {
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                repository_id,
                alias,
                "/tmp/repo",
                vec!["src".to_owned()],
                vec!["rust".to_owned()],
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
}

async fn insert_compatible_base_scope(store: &SqliteGraphStore, commit: &str, source_scope: &str) {
    let commit = commit.to_owned();
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection.execute(
                "
                INSERT INTO code_repository_scopes (
                    source_scope, repository_id, resolved_commit_sha, tree_hash,
                    path_filters_json, language_filters_json, indexed_file_count,
                    symbol_count, reference_count, chunk_count, stale,
                    degraded_reason, retiring
                ) VALUES (?1, 'repo', ?2, ?3, '[\"src\"]', '[\"rust\"]',
                          1, 0, 0, 0, 0, NULL, 0)
                ",
                rusqlite::params![source_scope, commit, format!("tree-{commit}")],
            )?;
            Ok(())
        })
        .await
        .expect("compatible base scope should persist");
}

fn seed(fingerprint: &str, scope: &str, now_ms: u64) -> CodeIndexTaskSeed {
    seed_with_payload(fingerprint, scope, now_ms, "{}")
}

fn seed_with_payload(
    fingerprint: &str,
    scope: &str,
    now_ms: u64,
    payload_json: &str,
) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: format!("commit-{scope}"),
        tree_hash: format!("tree-{scope}"),
        source_scope: scope.to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        mode: CodeIndexMode::Full,
        input_fingerprint: fingerprint.to_owned(),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: payload_json.to_owned(),
        now_ms,
    }
}

fn mode_seed(
    fingerprint: &str,
    scope: &str,
    now_ms: u64,
    mode: CodeIndexMode,
) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        mode,
        ..seed(fingerprint, scope, now_ms)
    }
}

fn pinned_worktree_seed(
    fingerprint: &str,
    scope: &str,
    now_ms: u64,
    base_commit: &str,
) -> CodeIndexTaskSeed {
    let pending_identity = format!("worktree:pending:{base_commit}");
    CodeIndexTaskSeed {
        ref_selector: base_commit.to_owned(),
        resolved_commit_sha: pending_identity.clone(),
        tree_hash: pending_identity,
        mode: CodeIndexMode::WorktreeOverlay,
        ..seed(fingerprint, scope, now_ms)
    }
}

fn repository_seed(
    repository_id: &str,
    alias: &str,
    fingerprint: &str,
    scope: &str,
    now_ms: u64,
) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: repository_id.to_owned(),
        alias: alias.to_owned(),
        ..seed(fingerprint, scope, now_ms)
    }
}

fn incremental_mode(index: usize) -> CodeIndexMode {
    CodeIndexMode::incremental(format!("base-{index}"), format!("head-{index}"))
        .expect("incremental mode should validate")
}

fn spawn_racing_enqueue(
    store: SqliteGraphStore,
    barrier: Arc<Barrier>,
    fingerprint: &'static str,
    now_ms: u64,
) -> tokio::task::JoinHandle<Result<crate::domain::CodeIndexTaskRecord, StorageError>> {
    tokio::spawn(async move {
        barrier.wait().await;
        store
            .queue_code_index_task(seed(fingerprint, fingerprint, now_ms))
            .await
    })
}

async fn task_state(store: &SqliteGraphStore, task_id: &str) -> CodeIndexTaskState {
    store
        .code_index_task(task_id.to_owned())
        .await
        .expect("task lookup should succeed")
        .expect("task should exist")
        .state
}
