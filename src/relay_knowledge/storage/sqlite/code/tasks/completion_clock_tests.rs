use super::{complete_task_at, fail_task_at};
use crate::{
    domain::{
        CodeIndexMode, CodeIndexResourceBudget, CodeIndexTaskState, CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskCompletion, CodeIndexTaskFailure,
        CodeIndexTaskSeed, CodeRepositoryStore, SqliteGraphStore,
    },
};

use super::super::test_support::persist_published_task_target;
use super::super::{claim_task_at, queue_task};

#[tokio::test]
async fn code_index_task_authoritative_execution_time_rejects_stale_completion_and_failure() {
    let store = registered_store().await;
    let queued = store
        .run(|connection| queue_task(connection, seed()))
        .await
        .expect("task should queue");
    let running = store
        .run({
            let task_id = queued.task_id;
            move |connection| {
                claim_task_at(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "worker".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 1,
                    },
                    1,
                )
            }
        })
        .await
        .expect("claim should run")
        .expect("task should claim");
    store
        .run({
            let running = running.clone();
            move |connection| persist_published_task_target(connection, &running)
        })
        .await
        .expect("publication receipt should persist");

    let completion = CodeIndexTaskCompletion {
        task_id: running.task_id.clone(),
        lease_owner: "worker".to_owned(),
        attempt_count: running.attempt_count,
        publication_generation: running.publication_generation,
        now_ms: 5,
    };
    let completion_error = store
        .run(move |connection| complete_task_at(connection, completion, 11))
        .await
        .expect_err("execution at expiry must reject completion observed while live");
    assert!(completion_error.to_string().contains("active lease"));

    let failure = CodeIndexTaskFailure {
        task_id: running.task_id.clone(),
        lease_owner: "worker".to_owned(),
        attempt_count: running.attempt_count,
        publication_generation: running.publication_generation,
        error_kind: "fixture".to_owned(),
        error_message: "must not persist".to_owned(),
        retry_backoff_ms: 50,
        max_attempts: 3,
        now_ms: 5,
    };
    let failure_error = store
        .run(move |connection| fail_task_at(connection, failure, 11))
        .await
        .expect_err("execution at expiry must reject failure observed while live");
    assert!(failure_error.to_string().contains("active lease"));

    let unchanged = store
        .run({
            let task_id = running.task_id;
            move |connection| {
                connection
                    .query_row(
                        "SELECT state, lease_owner, lease_expires_at_ms,
                                next_retry_at_ms, last_error_kind, last_error_message,
                                (SELECT COUNT(*) FROM code_repository_publication_receipts
                                 WHERE task_id = ?1)
                         FROM code_repository_index_tasks WHERE task_id = ?1",
                        [&task_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, Option<String>>(1)?,
                                row.get::<_, Option<u64>>(2)?,
                                row.get::<_, u64>(3)?,
                                row.get::<_, Option<String>>(4)?,
                                row.get::<_, Option<String>>(5)?,
                                row.get::<_, usize>(6)?,
                            ))
                        },
                    )
                    .map_err(crate::storage::StorageError::from)
            }
        })
        .await
        .expect("unchanged task should load");
    assert_eq!(unchanged.0, CodeIndexTaskState::Running.as_str());
    assert_eq!(unchanged.1.as_deref(), Some("worker"));
    assert_eq!(unchanged.2, Some(11));
    assert_eq!(unchanged.3, 0);
    assert_eq!(unchanged.4, None);
    assert_eq!(unchanged.5, None);
    assert_eq!(unchanged.6, 1);
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
}

fn seed() -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        source_scope: "scope".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        mode: CodeIndexMode::Full,
        input_fingerprint: "clock".to_owned(),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        now_ms: 0,
    }
}
