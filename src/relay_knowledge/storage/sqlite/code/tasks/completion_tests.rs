use super::super::retention::{RETAIN_FAILED_TASK_AUDIT_ROWS, RETAIN_SUCCEEDED_TASK_AUDIT_ROWS};
use super::super::{claim_task, queue_task};
use super::{complete_task, fail_task};
use crate::{
    domain::{
        CodeIndexMode, CodeIndexResourceBudget, CodeIndexTaskRecord, CodeIndexTaskState,
        CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskCompletion, CodeIndexTaskFailure,
        CodeIndexTaskSeed, CodeRepositoryStore, SqliteGraphStore,
    },
};

#[tokio::test]
async fn completion_transitions_require_active_lease_and_bound_retry_state() {
    let store = registered_store().await;

    let succeeded = claim(&store, "fp-success", "scope-success", 10).await;
    let wrong_owner = store
        .run({
            let task_id = succeeded.task_id.clone();
            move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id,
                        lease_owner: "other-worker".to_owned(),
                        attempt_count: 1,
                        now_ms: 30,
                    },
                )
            }
        })
        .await
        .expect_err("wrong lease owner should fail");
    assert!(wrong_owner.to_string().contains("active lease"));

    let succeeded = store
        .run({
            let task_id = succeeded.task_id;
            move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id,
                        lease_owner: "worker".to_owned(),
                        attempt_count: 1,
                        now_ms: 30,
                    },
                )
            }
        })
        .await
        .expect("active lease should complete");
    assert_eq!(succeeded.state, CodeIndexTaskState::Succeeded);
    assert!(succeeded.lease_owner.is_none());

    let retrying = claim(&store, "fp-retry", "scope-retry", 40).await;
    let retrying = fail(&store, retrying, 3, 50).await;
    assert_eq!(retrying.state, CodeIndexTaskState::Retrying);
    assert_eq!(retrying.next_retry_at_ms, 60);
    assert_eq!(retrying.last_error_kind.as_deref(), Some("fixture"));

    // A retrying predecessor intentionally holds this repository's FIFO lane.
    // Exercise the independent dead-letter transition in an isolated store
    // instead of bypassing that ordering invariant in the fixture.
    let dead_letter_store = registered_store().await;
    let dead_letter = claim(&dead_letter_store, "fp-dead", "scope-dead", 60).await;
    let dead_letter = fail(&dead_letter_store, dead_letter, 1, 70).await;
    assert_eq!(dead_letter.state, CodeIndexTaskState::DeadLetter);
    assert!(dead_letter.lease_owner.is_none());
}

#[tokio::test]
async fn code_index_task_completion_and_dead_letter_keep_audit_history_bounded() {
    let store = registered_store().await;
    for index in 0..RETAIN_SUCCEEDED_TASK_AUDIT_ROWS + 20 {
        let task = claim(
            &store,
            &format!("success-{index}"),
            &format!("success-scope-{index}"),
            100 + index as u64 * 3,
        )
        .await;
        store
            .run(move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id: task.task_id,
                        lease_owner: task.lease_owner.expect("task should have lease owner"),
                        attempt_count: task.attempt_count,
                        now_ms: task.updated_at_ms.saturating_add(1),
                    },
                )
            })
            .await
            .expect("task should complete");
    }
    for index in 0..RETAIN_FAILED_TASK_AUDIT_ROWS + 20 {
        let task = claim(
            &store,
            &format!("dead-{index}"),
            &format!("dead-scope-{index}"),
            10_000 + index as u64 * 3,
        )
        .await;
        fail(&store, task, 1, 10_002 + index as u64 * 3).await;
    }

    let (succeeded, dead_letter) = store
        .run(|connection| {
            let count = |state: &str| {
                connection.query_row(
                    "SELECT COUNT(*) FROM code_repository_index_tasks
                     WHERE repository_id = 'repo' AND state = ?1",
                    [state],
                    |row| row.get::<_, usize>(0),
                )
            };
            Ok((count("succeeded")?, count("dead_letter")?))
        })
        .await
        .expect("audit counts should load");

    assert!(succeeded <= RETAIN_SUCCEEDED_TASK_AUDIT_ROWS);
    assert!(dead_letter <= RETAIN_FAILED_TASK_AUDIT_ROWS);
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

async fn claim(
    store: &SqliteGraphStore,
    fingerprint: &str,
    scope: &str,
    now_ms: u64,
) -> CodeIndexTaskRecord {
    let seed = seed(fingerprint, scope, now_ms);
    let queued = store
        .run(move |connection| queue_task(connection, seed))
        .await
        .expect("task should queue");
    store
        .run(move |connection| {
            claim_task(
                connection,
                CodeIndexTaskClaimRequest {
                    task_id: Some(queued.task_id),
                    lease_owner: "worker".to_owned(),
                    lease_duration_ms: 100,
                    max_attempts: 3,
                    now_ms: now_ms.saturating_add(1),
                },
            )
        })
        .await
        .expect("task should claim")
        .expect("queued task should be claimable")
}

async fn fail(
    store: &SqliteGraphStore,
    task: CodeIndexTaskRecord,
    max_attempts: u32,
    now_ms: u64,
) -> CodeIndexTaskRecord {
    store
        .run(move |connection| {
            fail_task(
                connection,
                CodeIndexTaskFailure {
                    task_id: task.task_id,
                    lease_owner: task.lease_owner.expect("task should have lease owner"),
                    attempt_count: task.attempt_count,
                    error_kind: "fixture".to_owned(),
                    error_message: "fixture failure".to_owned(),
                    retry_backoff_ms: 10,
                    max_attempts,
                    now_ms,
                },
            )
        })
        .await
        .expect("task failure should persist")
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
