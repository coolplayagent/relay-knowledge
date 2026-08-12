//! Regression tests for durable repository-set refresh tasks.

use rusqlite::params;

use super as refresh_tasks;
use crate::{
    domain::CodeRepositorySetRefreshTaskState,
    storage::{
        CodeRepositorySetRefreshTaskClaimRequest, CodeRepositorySetRefreshTaskCompletion,
        CodeRepositorySetRefreshTaskFailure, CodeRepositorySetRefreshTaskSeed, SqliteGraphStore,
        StorageError,
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
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("fp-a", 100)))
        .await
        .expect("task should queue");
    let duplicate = store
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("fp-a", 101)))
        .await
        .expect("unfinished duplicate should reuse existing task");

    assert_eq!(queued.task_id, duplicate.task_id);
    assert_eq!(queued.state, CodeRepositorySetRefreshTaskState::Queued);
    assert_eq!(queued.set_alias, "workspace");
    assert_eq!(queued.attempt_count, 0);

    let running = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                refresh_tasks::claim_refresh_task(
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

    let distinct = store
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("fp-b", 112)))
        .await
        .expect("newest snapshot should queue beside running work");
    assert_ne!(queued.task_id, distinct.task_id);
    let same_set_blocked = store
        .run({
            let task_id = distinct.task_id.clone();
            move |connection| {
                refresh_tasks::claim_refresh_task(
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
        .expect("same-set writer exclusion should query");
    assert!(same_set_blocked.is_none());

    let blocked = store
        .run({
            let task_id = running.task_id.clone();
            move |connection| {
                refresh_tasks::claim_refresh_task(
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
                refresh_tasks::complete_refresh_task(
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
                refresh_tasks::complete_refresh_task(
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
            refresh_tasks::claim_refresh_task(
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
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("fp-a", 200)))
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
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("fp-retry", 10)))
        .await
        .expect("task should queue");
    let first_claim = store
        .run({
            let task_id = queued.task_id.clone();
            move |connection| {
                refresh_tasks::claim_refresh_task(
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
                refresh_tasks::claim_refresh_task(
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
                refresh_tasks::fail_refresh_task(
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
                refresh_tasks::claim_refresh_task(
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
                refresh_tasks::claim_refresh_task(
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
                refresh_tasks::fail_refresh_task(
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
                refresh_tasks::fail_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskFailure {
                        task_id,
                        lease_owner: "worker-c".to_owned(),
                        attempt_count: 3,
                        error_kind: "overlay_refresh".to_owned(),
                        error_message: "still failing".to_owned(),
                        retry_backoff_ms: 30,
                        max_attempts: 3,
                        now_ms: 79,
                    },
                )
            }
        })
        .await
        .expect("dead letter should persist");
    assert_eq!(dead.state, CodeRepositorySetRefreshTaskState::DeadLetter);

    let no_claim = store
        .run(|connection| {
            refresh_tasks::claim_refresh_task(
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
                refresh_tasks::queue_refresh_task(connection, seed("fp-retry", 1001))
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

#[tokio::test]
async fn repository_set_refresh_task_completion_and_failure_require_a_live_running_lease() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(insert_set)
        .await
        .expect("repository set fixture should insert");
    let queued = store
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("live-lease", 10)))
        .await
        .expect("task should queue");
    let running = store
        .run({
            let task_id = queued.task_id;
            move |connection| {
                refresh_tasks::claim_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: String::from("worker"),
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

    let completion_error = store
        .run({
            let task_id = running.task_id.clone();
            move |connection| {
                refresh_tasks::complete_refresh_task(
                    connection,
                    CodeRepositorySetRefreshTaskCompletion {
                        task_id,
                        lease_owner: String::from("worker"),
                        attempt_count: 1,
                        now_ms: 30,
                    },
                )
            }
        })
        .await
        .expect_err("completion at lease expiry must fail");
    assert!(completion_error.to_string().contains("lease"));

    let failure_error = store
        .run(move |connection| {
            refresh_tasks::fail_refresh_task(
                connection,
                CodeRepositorySetRefreshTaskFailure {
                    task_id: running.task_id,
                    lease_owner: String::from("worker"),
                    attempt_count: 1,
                    error_kind: String::from("overlay"),
                    error_message: String::from("expired"),
                    retry_backoff_ms: 10,
                    max_attempts: 3,
                    now_ms: 30,
                },
            )
        })
        .await
        .expect_err("failure at lease expiry must fail");
    assert!(failure_error.to_string().contains("lease"));
}

#[tokio::test]
async fn code_index_task_repository_set_queue_supersedes_pending_and_rejects_full_set() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(insert_set)
        .await
        .expect("repository set fixture should insert");
    let old = store
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("old", 10)))
        .await
        .expect("old snapshot should queue");
    let newest = store
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("new", 20)))
        .await
        .expect("new snapshot should supersede pending work");
    let (old_state, unfinished): (String, usize) = store
        .run(move |connection| {
            let old_state = connection.query_row(
                "SELECT state FROM code_repository_set_refresh_tasks WHERE task_id = ?1",
                params![old.task_id],
                |row| row.get(0),
            )?;
            let unfinished = connection.query_row(
                "SELECT COUNT(*) FROM code_repository_set_refresh_tasks
                 WHERE set_id = 'set-workspace' AND state IN ('queued', 'running', 'retrying')",
                [],
                |row| row.get(0),
            )?;
            Ok((old_state, unfinished))
        })
        .await
        .expect("queue state should query");
    assert_eq!(old_state, "cancelled");
    assert_eq!(unfinished, 1);
    assert_eq!(newest.state, CodeRepositorySetRefreshTaskState::Queued);

    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repository_set_refresh_tasks
                 SET state = 'running', lease_owner = 'worker-a', lease_expires_at_ms = 999
                 WHERE task_id = ?1",
                params![newest.task_id],
            )?;
            insert_refresh_task_row(connection, "running-b", "running-b", "running", 21)
        })
        .await
        .expect("two running task fixtures should persist");
    let error = store
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("overload", 30)))
        .await
        .expect_err("a set with two active tasks must apply backpressure");
    assert!(matches!(error, StorageError::CapacityExceeded(_)));
}

#[tokio::test]
async fn code_index_task_repository_set_terminal_history_is_bounded() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(|connection| {
            insert_set(connection)?;
            for index in 0..100_u64 {
                insert_refresh_task_row(
                    connection,
                    &format!("success-{index}"),
                    &format!("success-{index}"),
                    "succeeded",
                    index,
                )?;
            }
            for index in 0..70_u64 {
                insert_refresh_task_row(
                    connection,
                    &format!("failure-{index}"),
                    &format!("failure-{index}"),
                    "dead_letter",
                    1_000 + index,
                )?;
            }
            Ok(())
        })
        .await
        .expect("legacy terminal history should insert");
    store
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("current", 2_000)))
        .await
        .expect("queue admission should prune bounded terminal audit windows");
    let (succeeded, failure_class): (usize, usize) = store
        .run(|connection| {
            let succeeded = connection.query_row(
                "SELECT COUNT(*) FROM code_repository_set_refresh_tasks
                 WHERE set_id = 'set-workspace' AND state = 'succeeded'",
                [],
                |row| row.get(0),
            )?;
            let failure_class = connection.query_row(
                "SELECT COUNT(*) FROM code_repository_set_refresh_tasks
                 WHERE set_id = 'set-workspace'
                   AND state IN ('failed', 'dead_letter', 'cancelled')",
                [],
                |row| row.get(0),
            )?;
            Ok((succeeded, failure_class))
        })
        .await
        .expect("bounded audit counts should query");
    assert_eq!(
        succeeded,
        refresh_tasks::RETAIN_SUCCEEDED_REFRESH_TASKS_PER_SET
    );
    assert_eq!(
        failure_class,
        refresh_tasks::RETAIN_FAILURE_CLASS_REFRESH_TASKS_PER_STATE
    );
}

#[tokio::test]
async fn code_index_task_repository_set_queue_applies_global_backpressure() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(|connection| {
            insert_set(connection)?;
            for index in 0..refresh_tasks::MAX_UNFINISHED_REFRESH_TASKS_GLOBAL {
                let set_id = format!("global-set-{index}");
                connection.execute(
                    "INSERT INTO code_repository_sets (
                         set_id, alias, description, default_ref_policy_json,
                         created_at_ms, updated_at_ms
                     ) VALUES (?1, ?2, NULL, '{\"default_ref\":\"HEAD\"}', 1, 1)",
                    params![set_id, format!("global-{index}")],
                )?;
                connection.execute(
                    "INSERT INTO code_repository_set_refresh_tasks (
                         task_id, set_id, set_alias, state, lease_owner,
                         lease_expires_at_ms, attempt_count, next_retry_at_ms,
                         input_fingerprint, created_at_ms, updated_at_ms
                     ) VALUES (?1, ?2, ?3, 'running', 'worker', 999, 1, 0, ?4, 1, 1)",
                    params![
                        format!("global-task-{index}"),
                        set_id,
                        format!("global-{index}"),
                        format!("global-fingerprint-{index}")
                    ],
                )?;
            }
            Ok(())
        })
        .await
        .expect("global queue fixtures should insert");
    let error = store
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("overload", 2)))
        .await
        .expect_err("global unfinished capacity must reject admission");
    assert!(matches!(error, StorageError::CapacityExceeded(_)));
}

#[tokio::test]
async fn code_index_task_repository_set_expired_final_attempt_releases_capacity() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(|connection| {
            insert_set(connection)?;
            insert_refresh_task_row(connection, "expired", "expired", "running", 1)?;
            connection.execute(
                "UPDATE code_repository_set_refresh_tasks
                 SET attempt_count = 3, lease_owner = 'lost-worker', lease_expires_at_ms = 10
                 WHERE task_id = 'expired'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("expired final attempt fixture should insert");
    let claimed = store
        .run(|connection| {
            refresh_tasks::claim_refresh_task(
                connection,
                CodeRepositorySetRefreshTaskClaimRequest {
                    task_id: None,
                    lease_owner: "recovery-worker".to_owned(),
                    lease_duration_ms: 10,
                    max_attempts: 3,
                    now_ms: 20,
                },
            )
        })
        .await
        .expect("expired task recovery should run");
    assert!(claimed.is_none());
    store
        .run(|connection| refresh_tasks::queue_refresh_task(connection, seed("replacement", 21)))
        .await
        .expect("terminal lease recovery must release queue capacity");
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

fn insert_refresh_task_row(
    connection: &rusqlite::Connection,
    task_id: &str,
    fingerprint: &str,
    state: &str,
    now_ms: u64,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO code_repository_set_refresh_tasks (
             task_id, set_id, set_alias, state, attempt_count, next_retry_at_ms,
             input_fingerprint, created_at_ms, updated_at_ms
         ) VALUES (?1, 'set-workspace', 'workspace', ?2, 0, ?3, ?4, ?3, ?3)",
        params![task_id, state, now_ms, fingerprint],
    )?;
    Ok(())
}
