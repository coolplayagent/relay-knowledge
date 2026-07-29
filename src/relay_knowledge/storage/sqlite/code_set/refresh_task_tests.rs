use rusqlite::params;

use super::*;
use crate::{
    domain::CodeRepositorySetRefreshTaskState,
    storage::{
        CodeRepositorySetRefreshTaskClaimRequest, CodeRepositorySetRefreshTaskCompletion,
        CodeRepositorySetRefreshTaskFailure, CodeRepositorySetRefreshTaskSeed, SqliteGraphStore,
    },
};

#[tokio::test]
async fn repository_set_refresh_task_queue_claim_complete_and_requeue_round_trip() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(insert_set)
        .await
        .expect("repository set fixture should insert");
    let queued = store
        .run(|connection| code_set_tasks::queue_refresh_task(connection, seed("fp-a", 100)))
        .await
        .expect("task should queue");
    let duplicate = store
        .run(|connection| code_set_tasks::queue_refresh_task(connection, seed("fp-a", 101)))
        .await
        .expect("unfinished duplicate should reuse existing task");
    let distinct = store
        .run(|connection| code_set_tasks::queue_refresh_task(connection, seed("fp-b", 102)))
        .await
        .expect("distinct fingerprint should queue");

    assert_eq!(queued.task_id, duplicate.task_id);
    assert_ne!(queued.task_id, distinct.task_id);
    assert_eq!(queued.state, CodeRepositorySetRefreshTaskState::Queued);
    assert_eq!(queued.set_alias, "workspace");
    assert_eq!(queued.attempt_count, 0);

    let running = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                code_set_tasks::claim_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-a".to_owned(),
                        lease_duration_ms: 50,
                        max_attempts: 3,
                        now_ms: 110,
                    },
                )
            }
        })
        .await
        .expect("claim should query")
        .expect("queued task should claim");
    assert_eq!(running.state, CodeRepositorySetRefreshTaskState::Running);
    assert_eq!(running.lease_owner.as_deref(), Some("worker-a"));
    assert_eq!(running.lease_expires_at_ms, Some(160));
    assert_eq!(running.attempt_count, 1);

    let blocked = store
        .run({
            let task_id = running.task_id.clone();
            move |connection| {
                code_set_tasks::claim_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-b".to_owned(),
                        lease_duration_ms: 50,
                        max_attempts: 3,
                        now_ms: 120,
                    },
                )
            }
        })
        .await
        .expect("active lease should query");
    assert!(blocked.is_none());

    let invalid_complete = store
        .run({
            let task_id = running.task_id.clone();
            move |connection| {
                code_set_tasks::complete_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskCompletion {
                        task_id,
                        lease_owner: "other-worker".to_owned(),
                        attempt_count: 1,
                        now_ms: 125,
                    },
                )
            }
        })
        .await
        .expect_err("wrong lease owner should be rejected");
    assert!(invalid_complete.to_string().contains("lease"));

    let completed = store
        .run({
            let task_id = running.task_id.clone();
            move |connection| {
                code_set_tasks::complete_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskCompletion {
                        task_id,
                        lease_owner: "worker-a".to_owned(),
                        attempt_count: 1,
                        now_ms: 130,
                    },
                )
            }
        })
        .await
        .expect("completion should persist");
    assert_eq!(
        completed.state,
        CodeRepositorySetRefreshTaskState::Succeeded
    );
    assert!(completed.lease_owner.is_none());

    let next = store
        .run(|connection| {
            code_set_tasks::claim_refresh_task(
                connection,
                CodeRepositorySetRefreshTaskClaimRequest {
                    task_id: None,
                    lease_owner: "worker-next".to_owned(),
                    lease_duration_ms: 10,
                    max_attempts: 3,
                    now_ms: 140,
                },
            )
        })
        .await
        .expect("next queued task should query")
        .expect("distinct task should claim");
    assert_eq!(next.task_id, distinct.task_id);

    let requeued = store
        .run(|connection| code_set_tasks::queue_refresh_task(connection, seed("fp-a", 200)))
        .await
        .expect("terminal duplicate should reset");
    assert_eq!(requeued.task_id, queued.task_id);
    assert_eq!(requeued.state, CodeRepositorySetRefreshTaskState::Queued);
    assert_eq!(requeued.attempt_count, 0);
    assert!(requeued.last_error_message.is_none());
}

#[tokio::test]
async fn repository_set_refresh_task_retry_dead_letter_and_invalid_rows_are_explicit() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(insert_set)
        .await
        .expect("repository set fixture should insert");
    let queued = store
        .run(|connection| code_set_tasks::queue_refresh_task(connection, seed("fp-retry", 10)))
        .await
        .expect("task should queue");
    let first_claim = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                code_set_tasks::claim_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-a".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 20,
                    },
                )
            }
        })
        .await
        .expect("claim should query")
        .expect("task should claim");
    assert_eq!(first_claim.attempt_count, 1);

    let reclaimed = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                code_set_tasks::claim_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-b".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 31,
                    },
                )
            }
        })
        .await
        .expect("expired lease should query")
        .expect("expired lease should reclaim");
    assert_eq!(reclaimed.attempt_count, 2);
    assert_eq!(reclaimed.lease_owner.as_deref(), Some("worker-b"));

    let retrying = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                code_set_tasks::fail_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskFailure {
                        task_id,
                        lease_owner: "worker-b".to_owned(),
                        attempt_count: 2,
                        error_kind: "overlay_refresh".to_owned(),
                        error_message: "ambiguous import graph".to_owned(),
                        retry_backoff_ms: 30,
                        max_attempts: 3,
                        now_ms: 40,
                    },
                )
            }
        })
        .await
        .expect("failure should persist");
    assert_eq!(retrying.state, CodeRepositorySetRefreshTaskState::Retrying);
    assert_eq!(retrying.next_retry_at_ms, 70);
    assert_eq!(
        retrying.last_error_message.as_deref(),
        Some("ambiguous import graph")
    );

    let too_early = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                code_set_tasks::claim_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-c".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 69,
                    },
                )
            }
        })
        .await
        .expect("retry claim should query");
    assert!(too_early.is_none());

    let final_claim = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                code_set_tasks::claim_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker-c".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 70,
                    },
                )
            }
        })
        .await
        .expect("retry should query")
        .expect("retry should claim");
    assert_eq!(final_claim.attempt_count, 3);

    let invalid_failure = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                code_set_tasks::fail_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskFailure {
                        task_id,
                        lease_owner: "worker-c".to_owned(),
                        attempt_count: 2,
                        error_kind: "overlay_refresh".to_owned(),
                        error_message: "stale attempt".to_owned(),
                        retry_backoff_ms: 10,
                        max_attempts: 3,
                        now_ms: 75,
                    },
                )
            }
        })
        .await
        .expect_err("stale attempt should be rejected");
    assert!(invalid_failure.to_string().contains("lease"));

    let dead = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                code_set_tasks::fail_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskFailure {
                        task_id,
                        lease_owner: "worker-c".to_owned(),
                        attempt_count: 3,
                        error_kind: "overlay_refresh".to_owned(),
                        error_message: "still failing".to_owned(),
                        retry_backoff_ms: 30,
                        max_attempts: 3,
                        now_ms: 80,
                    },
                )
            }
        })
        .await
        .expect("dead letter should persist");
    assert_eq!(dead.state, CodeRepositorySetRefreshTaskState::DeadLetter);

    let no_claim = store
        .run(|connection| {
            code_set_tasks::claim_refresh_task(
                connection,
                CodeRepositorySetRefreshTaskClaimRequest {
                    task_id: None,
                    lease_owner: "worker-d".to_owned(),
                    lease_duration_ms: 10,
                    max_attempts: 3,
                    now_ms: 1000,
                },
            )
        })
        .await
        .expect("dead task should not claim");
    assert!(no_claim.is_none());

    let invalid_state_error = store
        .run({
            let task_id = queued.task_id;
            move |connection| {
                connection.execute(
                    "UPDATE code_repository_set_refresh_tasks SET state = 'mystery' WHERE task_id = ?1",
                    params![&task_id],
                )?;
                code_set_tasks::queue_refresh_task(connection, seed("fp-retry", 1001))
            }
        })
        .await
        .expect_err("unknown task state should fail decoding");
    assert!(
        invalid_state_error
            .to_string()
            .contains("unknown repository set refresh task state")
    );
}

fn seed(fingerprint: &str, now_ms: u64) -> CodeRepositorySetRefreshTaskSeed {
    CodeRepositorySetRefreshTaskSeed {
        set_id: "set-workspace".to_owned(),
        set_alias: "workspace".to_owned(),
        input_fingerprint: fingerprint.to_owned(),
        now_ms,
    }
}

fn insert_set(connection: &mut rusqlite::Connection) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_sets (
            set_id, alias, description, default_ref_policy_json, created_at_ms, updated_at_ms
        )
        VALUES ('set-workspace', 'workspace', NULL, '{\"default_ref\":\"HEAD\"}', 1, 1)
        ",
        [],
    )?;
    Ok(())
}
