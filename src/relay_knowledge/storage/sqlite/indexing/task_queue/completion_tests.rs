use crate::{
    domain::{GraphVersion, IndexKind, IndexState},
    storage::{
        IndexRefreshClaimRequest, IndexRefreshCompletion, IndexRefreshQueueRequest,
        IndexRefreshTaskState, IndexStore, SqliteGraphStore,
    },
};

use crate::storage::sqlite::indexing::task_queue::test_support::{
    commit_evidence, commit_relation,
};

#[tokio::test]
async fn completing_refresh_task_advances_cursor_and_clears_queue() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-complete", "docs", "Rust async storage").await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 100,
        })
        .await
        .expect("task should queue");
    let task = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 110,
        })
        .await
        .expect("claim should load")
        .expect("task should be claimed");

    let completed = store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: task.task_id.clone(),
            lease_owner: "worker-a".to_owned(),
            attempt_count: task.attempt_count,
            indexed_graph_version: GraphVersion::new(1),
            model_name: None,
            model_dimension: None,
            now_ms: 120,
        })
        .await
        .expect("task should complete");
    let cursors = store.index_cursors().await.expect("cursors should load");
    let statuses = store.index_statuses().await.expect("statuses should load");
    let diagnostics = store
        .index_refresh_diagnostics(130)
        .await
        .expect("diagnostics should load");

    assert_eq!(completed.state, IndexRefreshTaskState::Succeeded);
    assert_eq!(completed.cursor_after, Some(GraphVersion::new(1)));
    assert_eq!(completed.lease_owner, None);
    let bm25_cursor = cursors
        .iter()
        .find(|cursor| cursor.kind == IndexKind::Bm25 && cursor.source_scope == "docs")
        .expect("bm25 cursor should exist");
    assert_eq!(bm25_cursor.state, IndexState::Fresh);
    assert_eq!(bm25_cursor.indexed_graph_version, GraphVersion::new(1));
    let bm25_status = statuses
        .iter()
        .find(|status| status.kind == IndexKind::Bm25)
        .expect("bm25 status should exist");
    assert_eq!(bm25_status.state, IndexState::Fresh);
    assert_eq!(bm25_status.indexed_graph_version, GraphVersion::new(1));
    assert_eq!(diagnostics.queue_depth, 0);
    assert_eq!(diagnostics.running_count, 0);

    let repeated = store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 140,
        })
        .await
        .expect("fresh completed work should remain out of the queue");
    assert_eq!(repeated.queue_depth, 0);

    commit_evidence(&store, "ev-complete-next", "docs", "Rust async indexing").await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(2),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 150,
        })
        .await
        .expect("newer graph version should reset completed task");
    let reset = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-b".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 160,
        })
        .await
        .expect("reset task should load")
        .expect("reset task should be claimed");

    assert_eq!(reset.task_id, task.task_id);
    assert_eq!(reset.target_graph_version, GraphVersion::new(2));
    assert_eq!(reset.cursor_before, GraphVersion::new(1));
    assert_eq!(reset.cursor_after, None);
    assert_eq!(reset.last_error_kind, None);
}

#[tokio::test]
async fn completing_refresh_task_prefers_indexed_model_metadata() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(
        &store,
        "ev-vector-metadata",
        "docs",
        "Vector index cursor metadata tracks source hashes",
    )
    .await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Vector],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 100,
        })
        .await
        .expect("vector task should queue");
    let task = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 110,
        })
        .await
        .expect("claim should load")
        .expect("task should be claimed");

    store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: task.task_id,
            lease_owner: "worker-a".to_owned(),
            attempt_count: task.attempt_count,
            indexed_graph_version: GraphVersion::new(1),
            model_name: Some("text-embedding-3-small".to_owned()),
            model_dimension: Some(1536),
            now_ms: 120,
        })
        .await
        .expect("model metadata should complete");
    let cursors = store.index_cursors().await.expect("cursors should load");
    let cursor = cursors
        .iter()
        .find(|cursor| cursor.kind == IndexKind::Vector && cursor.source_scope == "docs")
        .expect("vector cursor should exist");

    assert_eq!(cursor.source_hash.as_deref().map(str::len), Some(16));
    assert!(
        cursor
            .backend_cursor
            .as_deref()
            .is_some_and(|value| value.starts_with("vector:text:"))
    );
    assert_eq!(
        cursor.model_name.as_deref(),
        Some("relay-local-hash-ann-v1")
    );
    assert_eq!(cursor.model_dimension, Some(16));
}

#[tokio::test]
async fn completing_refresh_task_preserves_model_metadata_without_new_documents() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-vector-preserve", "docs", "Rust async storage").await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Vector],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 100,
        })
        .await
        .expect("initial vector task should queue");
    let initial = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 110,
        })
        .await
        .expect("claim should load")
        .expect("initial task should be claimed");
    store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: initial.task_id,
            lease_owner: "worker-a".to_owned(),
            attempt_count: initial.attempt_count,
            indexed_graph_version: GraphVersion::new(1),
            model_name: None,
            model_dimension: None,
            now_ms: 120,
        })
        .await
        .expect("initial vector task should complete");

    commit_relation(&store, "rel-vector-preserve", "docs", "ev-vector-preserve").await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Vector],
            target_graph_version: GraphVersion::new(2),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 130,
        })
        .await
        .expect("relation-only vector task should queue");
    let relation_only = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-b".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 140,
        })
        .await
        .expect("claim should load")
        .expect("relation-only task should be claimed");
    store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: relation_only.task_id,
            lease_owner: "worker-b".to_owned(),
            attempt_count: relation_only.attempt_count,
            indexed_graph_version: GraphVersion::new(2),
            model_name: None,
            model_dimension: None,
            now_ms: 150,
        })
        .await
        .expect("relation-only vector task should preserve metadata");
    let cursors = store.index_cursors().await.expect("cursors should load");
    let cursor = cursors
        .iter()
        .find(|cursor| cursor.kind == IndexKind::Vector && cursor.source_scope == "docs")
        .expect("vector cursor should exist");

    assert_eq!(
        cursor.model_name.as_deref(),
        Some("relay-local-hash-ann-v1")
    );
    assert_eq!(cursor.model_dimension, Some(16));
}

#[tokio::test]
async fn completing_refresh_task_rejects_incomplete_backend_model_metadata() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-bad-model", "docs", "Rust async storage").await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Semantic],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 100,
        })
        .await
        .expect("semantic task should queue");
    let task = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 110,
        })
        .await
        .expect("claim should load")
        .expect("task should be claimed");

    let error = store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: task.task_id.clone(),
            lease_owner: "worker-a".to_owned(),
            attempt_count: task.attempt_count,
            indexed_graph_version: GraphVersion::new(1),
            model_name: Some("semantic-model".to_owned()),
            model_dimension: None,
            now_ms: 120,
        })
        .await
        .expect_err("model metadata must be complete");
    let still_running = store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: task.task_id,
            lease_owner: "worker-a".to_owned(),
            attempt_count: task.attempt_count,
            indexed_graph_version: GraphVersion::new(1),
            model_name: Some("semantic-model".to_owned()),
            model_dimension: Some(384),
            now_ms: 121,
        })
        .await
        .expect("valid metadata should still complete with active lease");

    assert!(
        error
            .to_string()
            .contains("model name and dimension must be supplied together")
    );
    assert_eq!(still_running.state, IndexRefreshTaskState::Succeeded);
}

#[tokio::test]
async fn completing_superseded_running_task_requeues_follow_up_refresh() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-running-v1", "docs", "Rust async storage").await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 100,
        })
        .await
        .expect("task should queue");
    let running = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 110,
        })
        .await
        .expect("claim should load")
        .expect("task should be claimed");

    commit_evidence(&store, "ev-running-v2", "docs", "Rust async indexing").await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(2),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 120,
        })
        .await
        .expect("running task should preserve claimed target");
    let partial = store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: running.task_id.clone(),
            lease_owner: "worker-a".to_owned(),
            attempt_count: running.attempt_count,
            indexed_graph_version: GraphVersion::new(1),
            model_name: None,
            model_dimension: None,
            now_ms: 130,
        })
        .await
        .expect("superseded completion should requeue follow-up");
    let follow_up = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-b".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 131,
        })
        .await
        .expect("follow-up claim should load")
        .expect("follow-up task should be claimed");

    assert_eq!(partial.state, IndexRefreshTaskState::Queued);
    assert_eq!(partial.attempt_count, 0);
    assert_eq!(partial.target_graph_version, GraphVersion::new(2));
    assert_eq!(partial.cursor_before, GraphVersion::new(1));
    assert_eq!(partial.cursor_after, None);
    assert_eq!(follow_up.task_id, running.task_id);
    assert_eq!(follow_up.attempt_count, 1);
    assert_eq!(follow_up.target_graph_version, GraphVersion::new(2));
    assert_eq!(follow_up.cursor_before, GraphVersion::new(1));

    store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: follow_up.task_id,
            lease_owner: "worker-b".to_owned(),
            attempt_count: follow_up.attempt_count,
            indexed_graph_version: GraphVersion::new(2),
            model_name: None,
            model_dimension: None,
            now_ms: 132,
        })
        .await
        .expect("follow-up completion should succeed");
    let cursors = store.index_cursors().await.expect("cursors should load");
    let diagnostics = store
        .index_refresh_diagnostics(133)
        .await
        .expect("diagnostics should load");
    let bm25_cursor = cursors
        .iter()
        .find(|cursor| cursor.kind == IndexKind::Bm25 && cursor.source_scope == "docs")
        .expect("bm25 cursor should exist");

    assert_eq!(bm25_cursor.state, IndexState::Fresh);
    assert_eq!(bm25_cursor.indexed_graph_version, GraphVersion::new(2));
    assert_eq!(diagnostics.queue_depth, 0);
}
