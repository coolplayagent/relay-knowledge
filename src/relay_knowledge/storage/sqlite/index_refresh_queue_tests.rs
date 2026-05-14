use super::*;
use crate::domain::{EvidenceRecord, GraphRelationRecord, IndexState, SourceScope};
use crate::storage::{
    IndexRefreshClaimRequest, IndexRefreshCompletion, IndexRefreshFailure,
    IndexRefreshQueueRequest, IndexRefreshTaskState,
};

#[test]
fn initialization_adds_task_timestamps_to_legacy_refresh_queue() {
    let connection = rusqlite::Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE graph_mutations (
                graph_version INTEGER PRIMARY KEY,
                evidence_count INTEGER NOT NULL,
                entity_count INTEGER NOT NULL,
                relation_count INTEGER NOT NULL DEFAULT 0,
                claim_count INTEGER NOT NULL DEFAULT 0,
                event_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE evidence (
                id TEXT PRIMARY KEY,
                source_scope TEXT NOT NULL
            );
            CREATE TABLE index_refresh_tasks (
                task_id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                modality TEXT NOT NULL,
                target_graph_version INTEGER NOT NULL,
                state TEXT NOT NULL,
                lease_owner TEXT,
                lease_expires_at_ms INTEGER,
                attempt_count INTEGER NOT NULL,
                next_retry_at_ms INTEGER NOT NULL,
                input_fingerprint TEXT NOT NULL,
                cursor_before INTEGER NOT NULL,
                cursor_after INTEGER,
                last_error_kind TEXT,
                last_error_message TEXT
            );
            INSERT INTO index_refresh_tasks (
                task_id, kind, source_scope, modality, target_graph_version, state,
                lease_owner, lease_expires_at_ms, attempt_count, next_retry_at_ms,
                input_fingerprint, cursor_before, cursor_after, last_error_kind,
                last_error_message
            )
            VALUES (
                'bm25:graph:text', 'bm25', 'graph', 'text', 1, 'queued',
                NULL, NULL, 0, 0, 'fingerprint', 0, NULL, NULL, NULL
            );
            ",
        )
        .expect("legacy schema should be created");

    indexing::initialize_schema(&connection).expect("schema should migrate");
    let columns = connection
        .prepare("PRAGMA table_info(index_refresh_tasks)")
        .expect("table info should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("columns should read")
        .collect::<Result<Vec<_>, _>>()
        .expect("columns should collect");

    assert!(columns.iter().any(|column| column == "created_at_ms"));
    assert!(columns.iter().any(|column| column == "updated_at_ms"));

    let (created_at_ms, updated_at_ms) = connection
        .query_row(
            "SELECT created_at_ms, updated_at_ms FROM index_refresh_tasks WHERE task_id = 'bm25:graph:text'",
            [],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
        )
        .expect("migrated timestamps should read");

    assert!(created_at_ms > 0);
    assert_eq!(updated_at_ms, created_at_ms);
}

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
async fn background_queue_rejects_zero_capacity_and_invalid_claims() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-invalid-queue", "docs", "Rust async storage").await;

    let capacity_error = store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 0,
            reset_dead_letter_tasks: false,
            now_ms: 10,
        })
        .await
        .expect_err("zero queue capacity should fail");
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
        capacity_error
            .to_string()
            .contains("queue capacity must be greater than zero")
    );
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
            task_id: first.task_id.clone(),
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
    let completed = store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: recovered.task_id,
            lease_owner: "worker-b".to_owned(),
            attempt_count: recovered.attempt_count,
            indexed_graph_version: GraphVersion::new(1),
            model_name: None,
            model_dimension: None,
            now_ms: 18,
        })
        .await
        .expect("current lease owner should complete");

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
    assert_eq!(completed.state, IndexRefreshTaskState::Succeeded);
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

async fn commit_evidence(store: &SqliteGraphStore, id: &str, source_scope: &str, content: &str) {
    let evidence = EvidenceRecord::new(
        id,
        SourceScope::parse(source_scope).expect("scope should parse"),
        content,
        Vec::new(),
    )
    .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");
}

async fn commit_relation(
    store: &SqliteGraphStore,
    relation_id: &str,
    source_scope: &str,
    evidence_id: &str,
) {
    let relation = GraphRelationRecord::new(
        relation_id,
        SourceScope::parse(source_scope).expect("scope should parse"),
        "relay-knowledge",
        "references",
        "cursor metadata preservation",
        vec![evidence_id.to_owned()],
    )
    .expect("relation should validate");
    let batch = GraphMutationBatch::with_facts(Vec::new(), vec![relation], Vec::new(), Vec::new())
        .expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("relation commit should succeed");
}
