use crate::{
    domain::{GraphVersion, IndexKind, IndexState},
    storage::{
        IndexRefreshClaimRequest, IndexRefreshCompletion, IndexRefreshFailure,
        IndexRefreshQueueRequest, IndexRefreshTaskState, IndexStore, SqliteGraphStore,
    },
};

use crate::storage::sqlite::indexing::task_queue::test_support::commit_evidence;

#[tokio::test]
async fn claim_rejects_invalid_lease_contracts() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let owner_error = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "  ".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 10,
        })
        .await
        .expect_err("blank lease owner should fail");
    let duration_error = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker".to_owned(),
            lease_duration_ms: 0,
            max_attempts: 3,
            now_ms: 10,
        })
        .await
        .expect_err("zero lease duration should fail");
    let attempts_error = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 0,
            now_ms: 10,
        })
        .await
        .expect_err("zero max attempts should fail");

    assert!(
        owner_error
            .to_string()
            .contains("lease owner must not be empty")
    );
    assert!(
        duration_error
            .to_string()
            .contains("lease duration must be greater than zero")
    );
    assert!(
        attempts_error
            .to_string()
            .contains("max attempts must be greater than zero")
    );
}

#[tokio::test]
async fn expired_task_lease_is_requeued_once() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-lease", "docs", "Rust async storage").await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 10,
        })
        .await
        .expect("task should queue");
    let first = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 5,
            max_attempts: 3,
            now_ms: 10,
        })
        .await
        .expect("claim should load")
        .expect("task should be claimed");

    let recovered = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-b".to_owned(),
            lease_duration_ms: 5,
            max_attempts: 3,
            now_ms: 16,
        })
        .await
        .expect("expired lease should recover")
        .expect("task should be reclaimed");

    assert_eq!(first.task_id, recovered.task_id);
    assert_eq!(recovered.state, IndexRefreshTaskState::Running);
    assert_eq!(recovered.lease_owner.as_deref(), Some("worker-b"));
    assert_eq!(recovered.attempt_count, 2);
    assert_eq!(recovered.last_error_kind.as_deref(), Some("lease_expired"));

    let stale_complete = store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: first.task_id.clone(),
            lease_owner: "worker-a".to_owned(),
            attempt_count: first.attempt_count,
            indexed_graph_version: GraphVersion::new(1),
            model_name: None,
            model_dimension: None,
            now_ms: 17,
        })
        .await
        .expect_err("stale lease completion should fail");
    let stale_failure = store
        .fail_index_refresh_task(IndexRefreshFailure {
            task_id: first.task_id,
            lease_owner: "worker-a".to_owned(),
            attempt_count: first.attempt_count,
            error_kind: "indexer".to_owned(),
            error_message: "stale worker failed late".to_owned(),
            retry_backoff_ms: 10,
            max_attempts: 3,
            now_ms: 17,
        })
        .await
        .expect_err("stale lease failure should fail");

    assert!(
        stale_complete
            .to_string()
            .contains("not held by an active lease")
    );
    assert!(
        stale_failure
            .to_string()
            .contains("not held by an active lease")
    );
}

#[tokio::test]
async fn expired_task_lease_dead_letters_after_attempt_budget() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(
        &store,
        "ev-expired-dead-letter",
        "docs",
        "Rust async storage",
    )
    .await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 10,
        })
        .await
        .expect("task should queue");
    store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 5,
            max_attempts: 1,
            now_ms: 10,
        })
        .await
        .expect("claim should load")
        .expect("task should be claimed");

    let reclaimed = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-b".to_owned(),
            lease_duration_ms: 5,
            max_attempts: 1,
            now_ms: 16,
        })
        .await
        .expect("expired lease recovery should load");
    let statuses = store.index_statuses().await.expect("statuses should load");
    let diagnostics = store
        .index_refresh_diagnostics(17)
        .await
        .expect("diagnostics should load");
    let bm25 = statuses
        .iter()
        .find(|status| status.kind == IndexKind::Bm25)
        .expect("bm25 status should exist");

    assert_eq!(reclaimed, None);
    assert_eq!(diagnostics.queue_depth, 0);
    assert_eq!(diagnostics.dead_letter_count, 1);
    assert_eq!(bm25.state, IndexState::Failed);
    assert_eq!(
        bm25.last_error.as_deref(),
        Some("index refresh task lease expired")
    );
}
