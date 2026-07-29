use super::*;
use crate::{
    domain::{
        CodeIndexMode, CodeIndexResourceBudget, CodeIndexTaskState, CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskSeed, CodeRepositoryStore, SqliteGraphStore,
    },
};
use rusqlite::params;

#[tokio::test]
async fn code_index_task_reset_requeues_unfinished_tasks_without_terminal_history() {
    let store = registered_store().await;
    let running_seed = queue(&store, "fp-running", "scope-a", 100).await;
    let dead_seed = queue(&store, "fp-dead", "scope-b", 101).await;
    let succeeded_seed = queue(&store, "fp-done", "scope-c", 102).await;
    let queued_seed = queue(&store, "fp-queued", "scope-d", 103).await;
    let retry_seed = queue(&store, "fp-retry", "scope-e", 104).await;
    store
        .run({
            let running_task_id = running_seed.task_id.clone();
            let dead_task_id = dead_seed.task_id.clone();
            let succeeded_task_id = succeeded_seed.task_id.clone();
            let retry_task_id = retry_seed.task_id.clone();
            move |connection| {
                connection.execute(
                    "
                    UPDATE code_repository_index_tasks
                    SET state = 'running',
                        lease_owner = 'worker-a',
                        lease_expires_at_ms = 210,
                        attempt_count = 1,
                        updated_at_ms = 110
                    WHERE task_id = ?1
                    ",
                    params![&running_task_id],
                )?;
                connection.execute(
                    "
                    UPDATE code_repository_index_tasks
                    SET state = 'dead_letter',
                        attempt_count = 2,
                        next_retry_at_ms = 122,
                        last_error_kind = 'lease_expired',
                        last_error_message = 'lease expired',
                        updated_at_ms = 122
                    WHERE task_id = ?1
                    ",
                    params![&dead_task_id],
                )?;
                connection.execute(
                    "
                    UPDATE code_repository_index_tasks
                    SET state = 'succeeded',
                        updated_at_ms = 120
                    WHERE task_id = ?1
                    ",
                    params![&succeeded_task_id],
                )?;
                connection.execute(
                    "
                    UPDATE code_repository_index_tasks
                    SET state = 'retrying',
                        attempt_count = 1,
                        next_retry_at_ms = 190,
                        last_error_kind = 'code_index',
                        last_error_message = 'retryable',
                        updated_at_ms = 140
                    WHERE task_id = ?1
                    ",
                    params![&retry_task_id],
                )?;
                Ok(())
            }
        })
        .await
        .expect("task states should persist");

    let reset = store
        .run(|connection| code_tasks::reset_tasks(connection, "repo", 220))
        .await
        .expect("reset should persist");

    assert_eq!(reset.len(), 3);
    assert!(
        reset
            .iter()
            .any(|task| task.task_id == running_seed.task_id)
    );
    assert!(reset.iter().any(|task| task.task_id == queued_seed.task_id));
    assert!(reset.iter().any(|task| task.task_id == retry_seed.task_id));
    for task in reset {
        assert_eq!(task.state, CodeIndexTaskState::Queued);
        assert!(task.lease_owner.is_none());
        assert_eq!(task.lease_expires_at_ms, None);
        assert_eq!(task.attempt_count, 0);
        assert_eq!(task.next_retry_at_ms, 220);
        assert!(task.last_error_kind.is_none());
    }
    let dead = store
        .run({
            let task_id = dead_seed.task_id.clone();
            move |connection| code_tasks::task_by_id(connection, &task_id)
        })
        .await
        .expect("dead task should load")
        .expect("dead task should exist");
    assert_eq!(dead.state, CodeIndexTaskState::DeadLetter);
    let completed = store
        .run({
            let task_id = succeeded_seed.task_id.clone();
            move |connection| code_tasks::task_by_id(connection, &task_id)
        })
        .await
        .expect("completed task should load")
        .expect("completed task should exist");
    assert_eq!(completed.state, CodeIndexTaskState::Succeeded);
}

#[tokio::test]
async fn code_index_task_reset_does_not_requeue_while_repository_writer_is_live() {
    let store = registered_store().await;
    let live_seed = queue(&store, "fp-live", "scope-live", 100).await;
    let queued_seed = queue(&store, "fp-queued", "scope-queued", 101).await;
    let live = claim(&store, &live_seed.task_id, "worker-live", 1_000, 110).await;

    let reset = store
        .run(|connection| code_tasks::reset_tasks(connection, "repo", 220))
        .await
        .expect("reset should persist");

    assert!(reset.is_empty());
    let still_live = store
        .run({
            let task_id = live.task_id.clone();
            move |connection| code_tasks::task_by_id(connection, &task_id)
        })
        .await
        .expect("live task should load")
        .expect("live task should exist");
    assert_eq!(still_live.state, CodeIndexTaskState::Running);
    assert_eq!(still_live.lease_owner.as_deref(), Some("worker-live"));
    assert_eq!(still_live.lease_expires_at_ms, live.lease_expires_at_ms);
    let still_queued = store
        .run({
            let task_id = queued_seed.task_id.clone();
            move |connection| code_tasks::task_by_id(connection, &task_id)
        })
        .await
        .expect("queued task should load")
        .expect("queued task should exist");
    assert_eq!(still_queued.state, CodeIndexTaskState::Queued);
    assert_eq!(still_queued.next_retry_at_ms, queued_seed.next_retry_at_ms);
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo",
                "fixture",
                "/tmp/repo",
                vec!["src".to_owned()],
                vec!["rust".to_owned()],
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
}

async fn queue(
    store: &SqliteGraphStore,
    fingerprint: &str,
    scope: &str,
    now_ms: u64,
) -> crate::domain::CodeIndexTaskRecord {
    store
        .run({
            let seed = seed(fingerprint, scope, now_ms);
            move |connection| code_tasks::queue_task(connection, seed)
        })
        .await
        .expect("task should queue")
}

async fn claim(
    store: &SqliteGraphStore,
    task_id: &str,
    lease_owner: &str,
    lease_duration_ms: u64,
    now_ms: u64,
) -> crate::domain::CodeIndexTaskRecord {
    store
        .run({
            let task_id = task_id.to_owned();
            let lease_owner = lease_owner.to_owned();
            move |connection| {
                code_tasks::claim_task(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner,
                        lease_duration_ms,
                        max_attempts: 2,
                        now_ms,
                    },
                )
            }
        })
        .await
        .expect("task should claim")
        .expect("task should exist")
}

fn seed(fingerprint: &str, scope: &str, now_ms: u64) -> CodeIndexTaskSeed {
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
        payload_json: "{}".to_owned(),
        now_ms,
    }
}
