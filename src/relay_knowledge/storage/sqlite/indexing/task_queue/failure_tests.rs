use crate::{
    domain::{GraphVersion, IndexKind, IndexState},
    storage::{
        IndexRefreshClaimRequest, IndexRefreshFailure, IndexRefreshQueueRequest,
        IndexRefreshTaskState, IndexStore, SqliteGraphStore,
    },
};

use crate::storage::sqlite::indexing::task_queue::test_support::commit_evidence;

#[tokio::test]
async fn failed_refresh_task_retries_then_dead_letters() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-fail", "docs", "Rust async storage").await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Vector],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 100,
        })
        .await
        .expect("task should queue");
    let first = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 2,
            now_ms: 100,
        })
        .await
        .expect("claim should load")
        .expect("task should be claimed");

    let retrying = store
        .fail_index_refresh_task(IndexRefreshFailure {
            task_id: first.task_id.clone(),
            lease_owner: "worker-a".to_owned(),
            attempt_count: first.attempt_count,
            error_kind: "indexer".to_owned(),
            error_message: "embedding worker unavailable".to_owned(),
            retry_backoff_ms: 25,
            max_attempts: 2,
            now_ms: 105,
        })
        .await
        .expect("first failure should retry");
    let not_ready = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-b".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 2,
            now_ms: 129,
        })
        .await
        .expect("claim before retry time should load");
    let second = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-b".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 2,
            now_ms: 130,
        })
        .await
        .expect("retry claim should load")
        .expect("retry task should be claimed");
    let dead_letter = store
        .fail_index_refresh_task(IndexRefreshFailure {
            task_id: first.task_id.clone(),
            lease_owner: "worker-b".to_owned(),
            attempt_count: second.attempt_count,
            error_kind: "indexer".to_owned(),
            error_message: "embedding worker still unavailable".to_owned(),
            retry_backoff_ms: 25,
            max_attempts: 2,
            now_ms: 135,
        })
        .await
        .expect("second failure should dead-letter");
    let statuses = store.index_statuses().await.expect("statuses should load");
    let diagnostics = store
        .index_refresh_diagnostics(140)
        .await
        .expect("diagnostics should load");

    assert_eq!(retrying.state, IndexRefreshTaskState::Retrying);
    assert_eq!(retrying.next_retry_at_ms, 130);
    assert_eq!(retrying.last_error_kind.as_deref(), Some("indexer"));
    assert_eq!(not_ready, None);
    assert_eq!(second.attempt_count, 2);
    assert_eq!(dead_letter.state, IndexRefreshTaskState::DeadLetter);
    let vector_status = statuses
        .iter()
        .find(|status| status.kind == IndexKind::Vector)
        .expect("vector status should exist");
    assert_eq!(vector_status.state, IndexState::Failed);
    assert_eq!(
        vector_status.last_error.as_deref(),
        Some("embedding worker still unavailable")
    );
    assert_eq!(diagnostics.queue_depth, 0);
    assert_eq!(diagnostics.dead_letter_count, 1);
    assert!(diagnostics.stale_reasons.iter().any(|reason| {
        reason.kind == IndexKind::Vector
            && reason.source_scope.is_none()
            && reason.reason == "index family failed"
            && reason.last_error.as_deref() == Some("embedding worker still unavailable")
    }));
    assert!(diagnostics.stale_reasons.iter().any(|reason| {
        reason.kind == IndexKind::Vector
            && reason.source_scope.as_deref() == Some("docs")
            && reason.reason == "scoped cursor failed"
            && reason.last_error.as_deref() == Some("embedding worker still unavailable")
    }));

    let preserved = store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Vector],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 150,
        })
        .await
        .expect("diagnostic queue should preserve dead-lettered task");
    let skipped = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-c".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 2,
            now_ms: 151,
        })
        .await
        .expect("preserved dead-letter claim should load");
    let reset = store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Vector],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: true,
            now_ms: 160,
        })
        .await
        .expect("dead-lettered task should be reset by explicit requeue");
    let reclaimed = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-c".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 2,
            now_ms: 161,
        })
        .await
        .expect("reset task should load")
        .expect("reset task should be claimed");

    assert_eq!(preserved.queue_depth, 0);
    assert_eq!(preserved.dead_letter_count, 1);
    assert_eq!(skipped, None);
    assert_eq!(reset.queue_depth, 1);
    assert_eq!(reclaimed.task_id, first.task_id);
    assert_eq!(reclaimed.attempt_count, 1);
    assert_eq!(reclaimed.last_error_kind, None);
    assert_eq!(reclaimed.last_error_message, None);
}
