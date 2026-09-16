//! Source observations, not the wall clock, own durable retry identity.

use super::super::super::test_support::{claim_task_at_request_time, fail_task_at_request_time};
use super::{insert_compatible_base_scope, registered_store};
use crate::{
    domain::{CodeIndexTaskRecord, CodeIndexTaskState},
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskFailure, CodeIndexTaskSeed,
        CodeIndexTaskStore as _, SqliteGraphStore,
    },
};

fn source_io_seed(observation: u64, now_ms: u64) -> CodeIndexTaskSeed {
    let mut seed = super::pinned_worktree_seed(
        &format!("worktree_reconcile:repo:base:{observation:016x}"),
        "io-pending",
        now_ms,
        "base",
    );
    seed.payload_json = serde_json::json!({
        "watcher": {
            "kind": "periodic_worktree_reconcile",
            "observation_fingerprint": format!("{observation:016x}"),
            "source_io_recheck": true,
        }
    })
    .to_string();
    seed
}

async fn claim(store: &SqliteGraphStore, now_ms: u64) -> Option<CodeIndexTaskRecord> {
    store
        .run(move |connection| {
            claim_task_at_request_time(
                connection,
                CodeIndexTaskClaimRequest {
                    task_id: None,
                    lease_owner: "source-io-worker".into(),
                    lease_duration_ms: 1_000_000,
                    max_attempts: 3,
                    now_ms,
                },
            )
        })
        .await
        .unwrap()
}

async fn fail(
    store: &SqliteGraphStore,
    task: CodeIndexTaskRecord,
    now_ms: u64,
) -> CodeIndexTaskRecord {
    store
        .run(move |connection| {
            fail_task_at_request_time(
                connection,
                CodeIndexTaskFailure {
                    task_id: task.task_id,
                    lease_owner: task.lease_owner.unwrap(),
                    attempt_count: task.attempt_count,
                    publication_generation: task.publication_generation,
                    error_kind: "storage_unavailable".into(),
                    error_message: "index shard unavailable".into(),
                    retry_backoff_ms: 60_000,
                    max_attempts: 3,
                    now_ms,
                },
            )
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn source_io_minute_ticks_preserve_leases_backoff_and_dead_letter_history() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "base", "base-scope").await;
    let queued = store
        .queue_code_index_task(source_io_seed(10, 0))
        .await
        .unwrap();
    let running = claim(&store, 10).await.unwrap();
    let observed = store
        .queue_code_index_task(source_io_seed(10, 65_000))
        .await
        .unwrap();
    assert_eq!(observed, running);

    let retrying = fail(&store, running, 70_000).await;
    assert_eq!(retrying.next_retry_at_ms, 130_000);
    let observed = store
        .queue_code_index_task(source_io_seed(10, 120_000))
        .await
        .unwrap();
    assert_eq!(observed, retrying);
    assert!(claim(&store, 129_999).await.is_none());
    let second = claim(&store, 130_000).await.unwrap();
    assert_eq!(second.task_id, queued.task_id);
    assert_eq!(second.attempt_count, 2);
    let retrying = fail(&store, second, 130_001).await;
    let observed = store
        .queue_code_index_task(source_io_seed(10, 180_000))
        .await
        .unwrap();
    assert_eq!(observed, retrying);
    let third = claim(&store, 190_001).await.unwrap();
    assert_eq!(third.attempt_count, 3);
    let dead = fail(&store, third, 190_002).await;
    assert_eq!(dead.state, CodeIndexTaskState::DeadLetter);
    let observed = store
        .queue_code_index_task(source_io_seed(10, 600_000))
        .await
        .unwrap();
    assert_eq!(observed, dead);
    assert!(claim(&store, 600_000).await.is_none());
}

#[tokio::test]
async fn source_io_success_waits_a_full_minute_after_completion_before_recheck() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "base", "base-scope").await;
    let queued = store
        .queue_code_index_task(source_io_seed(10, 0))
        .await
        .unwrap();
    store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                connection.execute(
                    "UPDATE code_repository_index_tasks SET state = 'succeeded',
                 attempt_count = 1, updated_at_ms = 59000 WHERE task_id = ?1",
                    [task_id],
                )?;
                Ok(())
            }
        })
        .await
        .unwrap();
    for now_ms in [60_000, 118_999] {
        let existing = store
            .queue_code_index_task(source_io_seed(10, now_ms))
            .await
            .unwrap();
        assert_eq!(existing.state, CodeIndexTaskState::Succeeded);
        assert_eq!(existing.updated_at_ms, 59_000);
    }
    let recheck = store
        .queue_code_index_task(source_io_seed(10, 119_000))
        .await
        .unwrap();
    assert_eq!(recheck.task_id, queued.task_id);
    assert_eq!(recheck.state, CodeIndexTaskState::Queued);
    assert_eq!(recheck.attempt_count, 0);
}

#[tokio::test]
async fn source_io_changed_content_can_supersede_an_old_retry() {
    let store = registered_store().await;
    insert_compatible_base_scope(&store, "base", "base-scope").await;
    let first = store
        .queue_code_index_task(source_io_seed(10, 0))
        .await
        .unwrap();
    let running = claim(&store, 10).await.unwrap();
    fail(&store, running, 11).await;
    let changed = store
        .queue_code_index_task(source_io_seed(20, 20))
        .await
        .unwrap();
    assert_ne!(changed.task_id, first.task_id);
    assert_eq!(changed.state, CodeIndexTaskState::Queued);
    let old = store.code_index_task(first.task_id).await.unwrap().unwrap();
    assert_eq!(old.state, CodeIndexTaskState::Cancelled);
}
