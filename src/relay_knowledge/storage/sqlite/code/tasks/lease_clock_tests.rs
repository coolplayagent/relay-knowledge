use super::{claim_task_at, recover_expired_task_leases_at};
use crate::{
    domain::{
        CodeIndexMode, CodeIndexResourceBudget, CodeIndexTaskRecord, CodeIndexTaskState,
        CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskSeed, RepositoryCatalogStore as _, SqliteGraphStore,
    },
};

use super::super::{queue_task, reset_tasks_at};

#[tokio::test]
async fn code_index_task_future_observation_cannot_claim_recover_or_reset_a_live_attempt() {
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
        .expect("initial claim should run")
        .expect("task should claim");

    let future_claim = store
        .run({
            let task_id = running.task_id.clone();
            move |connection| {
                claim_task_at(
                    connection,
                    CodeIndexTaskClaimRequest {
                        task_id: Some(task_id),
                        lease_owner: "future-worker".to_owned(),
                        lease_duration_ms: 10,
                        max_attempts: 3,
                        now_ms: 12,
                    },
                    5,
                )
            }
        })
        .await
        .expect_err("future claim observation must fail closed");
    assert!(
        future_claim
            .to_string()
            .contains("later than authoritative")
    );
    assert_live_attempt_unchanged(&store, &running).await;

    let future_recovery = store
        .run(|connection| recover_expired_task_leases_at(connection, 12, 3, 5))
        .await
        .expect_err("future recovery observation must fail closed");
    assert!(
        future_recovery
            .to_string()
            .contains("later than authoritative")
    );
    assert_live_attempt_unchanged(&store, &running).await;

    let future_reset = store
        .run(|connection| reset_tasks_at(connection, "repo", 12, 5))
        .await
        .expect_err("future reset observation must fail closed");
    assert!(
        future_reset
            .to_string()
            .contains("later than authoritative")
    );
    assert_live_attempt_unchanged(&store, &running).await;
}

async fn assert_live_attempt_unchanged(store: &SqliteGraphStore, expected: &CodeIndexTaskRecord) {
    let actual = store
        .run({
            let task_id = expected.task_id.clone();
            move |connection| super::super::task_by_id(connection, &task_id)
        })
        .await
        .expect("task lookup should run")
        .expect("task should exist");
    assert_eq!(actual.state, CodeIndexTaskState::Running);
    assert_eq!(actual.lease_owner, expected.lease_owner);
    assert_eq!(actual.lease_expires_at_ms, expected.lease_expires_at_ms);
    assert_eq!(actual.attempt_count, expected.attempt_count);
    assert_eq!(
        actual.publication_generation,
        expected.publication_generation
    );
    assert_eq!(actual.updated_at_ms, expected.updated_at_ms);
    assert_eq!(actual.last_error_kind, expected.last_error_kind);
    assert_eq!(actual.last_error_message, expected.last_error_message);
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
