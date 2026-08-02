use crate::{
    domain::{GraphVersion, WorkerKind, WorkerTaskState},
    storage::{
        IndexStore, SqliteGraphStore, WorkerTaskClaimRequest, WorkerTaskFailure, WorkerTaskSeed,
    },
};

#[tokio::test]
async fn sqlite_worker_queue_claim_failure_and_status_are_persistent() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let queued = store
        .queue_worker_tasks(vec![WorkerTaskSeed {
            kind: WorkerKind::Extractor,
            source_scope: "docs".to_owned(),
            evidence_id: Some("ev-worker".to_owned()),
            target_graph_version: GraphVersion::new(7),
            input_fingerprint: "extractor:ev-worker:7".to_owned(),
            payload_json: "{\"kind\":\"extractor\"}".to_owned(),
            now_ms: 10,
        }])
        .await
        .expect("task should queue");

    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].state, WorkerTaskState::Queued);

    let claimed = store
        .claim_worker_task(WorkerTaskClaimRequest {
            kind: Some(WorkerKind::Extractor),
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 500,
            max_attempts: 1,
            now_ms: 20,
        })
        .await
        .expect("claim should query")
        .expect("task should claim");

    assert_eq!(claimed.state, WorkerTaskState::Running);
    assert_eq!(claimed.attempt_count, 1);

    let failed = store
        .fail_worker_task(WorkerTaskFailure {
            task_id: claimed.task_id.clone(),
            lease_owner: "worker-a".to_owned(),
            attempt_count: claimed.attempt_count,
            error_kind: "extractor".to_owned(),
            error_message: "backend failed".to_owned(),
            retry_backoff_ms: 100,
            max_attempts: 1,
            now_ms: 30,
        })
        .await
        .expect("failure should persist");

    assert_eq!(failed.state, WorkerTaskState::DeadLetter);
    assert_eq!(failed.last_error_message.as_deref(), Some("backend failed"));

    let statuses = store.worker_statuses().await.expect("statuses should load");
    let extractor = statuses
        .iter()
        .find(|status| status.kind == WorkerKind::Extractor)
        .expect("extractor status should exist");

    assert_eq!(extractor.dead_letter_count, 1);
    assert_eq!(extractor.last_error.as_deref(), Some("backend failed"));
}
