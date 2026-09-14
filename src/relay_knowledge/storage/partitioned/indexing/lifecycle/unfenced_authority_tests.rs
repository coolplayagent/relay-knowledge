//! Product-boundary tests for partitioned unfenced mutation rejection.

use crate::storage::{
    CodeIndexPublicationStore as _, CodeIndexTaskClaimRequest, CodeIndexTaskStore as _,
    RepositoryCatalogStore as _, StorageError,
};

use super::super::test_support::partitioned_store;
use super::publication_barrier_tests::{
    batch_from_snapshot, now_millis, publication_fence, registration, session_from_snapshot,
    snapshot, task_seed,
};

#[tokio::test]
async fn staged_fenced_task_keeps_partitioned_unfenced_targets_at_zero_rows() {
    let store = partitioned_store("unfenced-authority-active");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let staged = snapshot("scope-fenced-staged");
    let queued = store
        .queue_code_index_task(task_seed(&staged.source_scope))
        .await
        .expect("task should queue");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "partitioned-authority-worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("task claim should run")
        .expect("task should claim");
    store
        .begin_code_index_session_with_fence(
            session_from_snapshot(&staged),
            publication_fence(&running, "partitioned-authority-worker"),
        )
        .await
        .expect("fenced checkpoint should stage");

    let direct_snapshot = snapshot("scope-unfenced-snapshot");
    let direct_snapshot_scope = direct_snapshot.source_scope.clone();
    let snapshot_error = store
        .apply_code_index_snapshot(direct_snapshot)
        .await
        .expect_err("partitioned snapshot must require a fence");
    assert_partitioned_fence_error(snapshot_error, "snapshot");

    let direct_session_snapshot = snapshot("scope-unfenced-session");
    let direct_session_scope = direct_session_snapshot.source_scope.clone();
    let session_error = store
        .begin_code_index_session(session_from_snapshot(&direct_session_snapshot))
        .await
        .expect_err("partitioned session must require a fence");
    assert_partitioned_fence_error(session_error, "session start");

    assert!(
        store
            .catalog
            .repository_for_scope(direct_snapshot_scope.clone())
            .await
            .expect("snapshot route should query")
            .is_none()
    );
    assert!(
        store
            .catalog
            .repository_for_scope(direct_session_scope.clone())
            .await
            .expect("session route should query")
            .is_none()
    );
    let shard = store
        .catalog
        .checkpoint_repository_store("repo".to_owned())
        .await
        .expect("staged shard lookup should run")
        .expect("fenced staged shard should exist");
    let rejected_row_count = shard
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                       (SELECT COUNT(*) FROM code_repository_scopes WHERE source_scope = ?1) +
                       (SELECT COUNT(*) FROM code_repository_index_checkpoints WHERE source_scope = ?2) +
                       (SELECT COUNT(*) FROM code_repository_files WHERE source_scope IN (?1, ?2))",
                    rusqlite::params![direct_snapshot_scope, direct_session_scope],
                    |row| row.get::<_, usize>(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("rejected shard row count should load");
    assert_eq!(rejected_row_count, 0);
}

#[tokio::test]
async fn every_partitioned_checkpoint_mutation_requires_a_durable_fence() {
    let store = partitioned_store("unfenced-authority-all-entries");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let indexed = snapshot("scope-unfenced-all-entries");
    let session = session_from_snapshot(&indexed);

    let batch_error = store
        .apply_code_index_batch(batch_from_snapshot(indexed))
        .await
        .expect_err("partitioned batch must require a fence");
    assert_partitioned_fence_error(batch_error, "batch publication");
    let finalize_error = store
        .finalize_code_index_session(session.clone())
        .await
        .expect_err("partitioned finalization must require a fence");
    assert_partitioned_fence_error(finalize_error, "session finalization");
    let resume_error = store
        .begin_code_index_session_at_checkpoint(session, None)
        .await
        .expect_err("partitioned resume must require a fence");
    assert_partitioned_fence_error(resume_error, "session resume");
    assert!(
        store
            .code_index_checkpoint("scope-unfenced-all-entries".to_owned())
            .await
            .expect("rejected checkpoint should query")
            .is_none(),
        "rejection must happen before any checkpoint is written to the registered repository shard"
    );
}

fn assert_partitioned_fence_error(error: StorageError, operation: &str) {
    assert!(matches!(error, StorageError::InvalidInput(message)
            if message.contains("partitioned_sqlite")
                && message.contains(operation)
                && message.contains("publication fence")));
}
