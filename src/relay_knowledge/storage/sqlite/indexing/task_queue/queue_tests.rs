use crate::{
    domain::{GraphVersion, IndexKind},
    storage::{IndexRefreshQueueRequest, IndexStore, SqliteGraphStore},
};

use crate::storage::sqlite::indexing::task_queue::test_support::commit_evidence;

#[tokio::test]
async fn background_queue_rejects_when_capacity_is_exceeded() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-queue", "docs", "Rust async storage").await;

    let error = store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: IndexKind::ALL.to_vec(),
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 2,
            reset_dead_letter_tasks: false,
            now_ms: 100,
        })
        .await
        .expect_err("three index tasks should exceed capacity two");

    assert!(
        error
            .to_string()
            .contains("index refresh queue capacity exceeded")
    );
}

#[tokio::test]
async fn background_queue_rejects_zero_capacity() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-invalid-queue", "docs", "Rust async storage").await;

    let error = store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 0,
            reset_dead_letter_tasks: false,
            now_ms: 10,
        })
        .await
        .expect_err("zero queue capacity should fail");

    assert!(
        error
            .to_string()
            .contains("queue capacity must be greater than zero")
    );
}
