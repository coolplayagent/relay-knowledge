use super::*;
use crate::domain::{
    ClaimRecord, EventRecord, EvidenceRecord, GraphRelationRecord, IndexState, RetrieverSource,
    SourceScope,
};
use crate::storage::{
    IndexRefreshClaimRequest, IndexRefreshCompletion, IndexRefreshFailure,
    IndexRefreshQueueRequest, IndexRefreshTaskState,
};

#[tokio::test]
async fn commits_graph_batch_and_marks_indexes_stale() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-1",
        scope,
        "Rust uses ownership",
        vec!["Rust".to_owned()],
    )
    .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");

    let receipt = store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");
    let inspection = store.inspect_graph().await.expect("inspection should load");
    let statuses = store.index_statuses().await.expect("statuses should load");

    assert_eq!(receipt.graph_version, GraphVersion::new(1));
    assert_eq!(inspection.entity_count, 1);
    assert_eq!(inspection.evidence_count, 1);
    assert!(
        statuses
            .iter()
            .all(|status| status.is_stale_for(GraphVersion::new(1)))
    );
}

#[tokio::test]
async fn searches_evidence_by_query_token() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let evidence = EvidenceRecord::new("ev-1", scope, "Hybrid retrieval uses BM25", Vec::new())
        .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "BM25".to_owned(),
            source_scope: None,
            graph_version: GraphVersion::new(1),
            limit: 5,
        })
        .await
        .expect("search should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].evidence_id, "ev-1");
    assert!(hits[0].retriever_sources.contains(&RetrieverSource::Bm25));
}

#[tokio::test]
async fn reads_mutation_log_after_version() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-1", "docs", "Rust async storage").await;
    commit_evidence(&store, "ev-2", "docs", "SQLite graph storage").await;

    let entries = store
        .read_after(GraphVersion::new(1), 10)
        .await
        .expect("mutation log should load");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].graph_version, GraphVersion::new(2));
    assert_eq!(entries[0].evidence_count, 1);
    assert_eq!(entries[0].affected_scopes, ["docs"]);
    assert_eq!(entries[0].evidence_ids, ["ev-2"]);
    assert_eq!(entries[0].source_hashes.len(), 1);
    assert_eq!(entries[0].affected_entity_ids.len(), 1);
}

#[tokio::test]
async fn mutation_log_records_affected_entities_and_source_hashes() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-entity", "docs", "Rust async storage").await;

    let entries = store
        .read_after(GraphVersion::ZERO, 10)
        .await
        .expect("mutation log should load");

    assert_eq!(entries[0].affected_scopes, ["docs"]);
    assert_eq!(entries[0].evidence_ids, ["ev-entity"]);
    assert_eq!(entries[0].affected_entity_ids.len(), 1);
    assert_eq!(entries[0].source_hashes.len(), 1);
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

#[tokio::test]
async fn commit_receipt_counts_unique_affected_entities() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let first = EvidenceRecord::new(
        "ev-1",
        scope.clone(),
        "Rust async storage",
        vec!["Rust".to_owned()],
    )
    .expect("evidence should validate");
    let second = EvidenceRecord::new(
        "ev-2",
        scope,
        "Rust graph retrieval",
        vec!["rust".to_owned()],
    )
    .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![first, second]).expect("batch should validate");

    let receipt = store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");
    let entries = store
        .read_after(GraphVersion::ZERO, 10)
        .await
        .expect("mutation log should load");

    assert_eq!(receipt.entity_count, 1);
    assert_eq!(entries[0].entity_count, 1);
}

#[tokio::test]
async fn commits_structured_relation_claim_and_event_facts() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let relation = GraphRelationRecord::new(
        "rel-1",
        "relay-knowledge",
        "implements",
        "BM25 retrieval",
        Vec::new(),
    )
    .expect("relation should validate");
    let claim = ClaimRecord::new(
        "claim-1",
        "relay-knowledge",
        "retrieval",
        "uses reciprocal-rank fusion",
        Vec::new(),
    )
    .expect("claim should validate");
    let event = EventRecord::new(
        "event-1",
        "index_refreshed",
        vec!["relay-knowledge".to_owned()],
        Some("2026-05-12".to_owned()),
        Vec::new(),
    )
    .expect("event should validate");
    let batch =
        GraphMutationBatch::with_facts(Vec::new(), vec![relation], vec![claim], vec![event])
            .expect("structured graph facts should validate");

    let receipt = store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");
    let graph = store.inspect_graph().await.expect("graph should inspect");
    let entries = store
        .read_after(GraphVersion::ZERO, 10)
        .await
        .expect("mutation log should load");

    assert_eq!(receipt.relation_count, 1);
    assert_eq!(receipt.claim_count, 1);
    assert_eq!(receipt.event_count, 1);
    assert_eq!(graph.relation_count, 1);
    assert_eq!(graph.claim_count, 1);
    assert_eq!(graph.event_count, 1);
    assert_eq!(entries[0].relation_count, 1);
    assert_eq!(entries[0].claim_count, 1);
    assert_eq!(entries[0].event_count, 1);
}

#[tokio::test]
async fn rejects_zero_limits_for_search_and_mutation_log() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");

    let search_error = store
        .search(GraphSearchRequest {
            query: "Rust".to_owned(),
            source_scope: None,
            graph_version: GraphVersion::ZERO,
            limit: 0,
        })
        .await
        .expect_err("zero search limit should fail");
    let log_error = store
        .read_after(GraphVersion::ZERO, 0)
        .await
        .expect_err("zero log limit should fail");

    assert_eq!(
        search_error.to_string(),
        "invalid storage input: search limit must be greater than zero"
    );
    assert_eq!(
        log_error.to_string(),
        "invalid storage input: mutation log limit must be greater than zero"
    );
}

#[tokio::test]
async fn search_filters_by_source_scope_and_sorts_by_score() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-1", "docs", "Rust Rust SQLite").await;
    commit_evidence(&store, "ev-2", "repo", "Rust").await;

    let docs = store
        .search(GraphSearchRequest {
            query: "Rust SQLite".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(2),
            limit: 5,
        })
        .await
        .expect("search should succeed");
    let all = store
        .search(GraphSearchRequest {
            query: "Rust".to_owned(),
            source_scope: None,
            graph_version: GraphVersion::new(2),
            limit: 5,
        })
        .await
        .expect("search should succeed");

    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].source_scope, "docs");
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn search_considers_matches_beyond_newest_candidates() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(
        &store,
        "ev-old",
        "docs",
        "Needle evidence remains searchable after newer writes",
    )
    .await;

    let scope = SourceScope::parse("docs").expect("scope should parse");
    let newer = (0..500)
        .map(|index| {
            EvidenceRecord::new(
                format!("ev-new-{index}"),
                scope.clone(),
                format!("Unrelated graph maintenance record {index}"),
                Vec::new(),
            )
            .expect("evidence should validate")
        })
        .collect::<Vec<_>>();
    let batch = GraphMutationBatch::new(newer).expect("batch should validate");
    let receipt = store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "Needle".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: receipt.graph_version,
            limit: 5,
        })
        .await
        .expect("search should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].evidence_id, "ev-old");
}

#[tokio::test]
async fn search_respects_graph_version_snapshot() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-1", "docs", "Snapshot only sees Rust").await;
    commit_evidence(&store, "ev-2", "docs", "Future vector token").await;

    let before_future = store
        .search(GraphSearchRequest {
            query: "Future".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
        })
        .await
        .expect("search should succeed");
    let after_future = store
        .search(GraphSearchRequest {
            query: "Future".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(2),
            limit: 5,
        })
        .await
        .expect("search should succeed");

    assert!(before_future.is_empty());
    assert_eq!(after_future.len(), 1);
    assert_eq!(after_future[0].evidence_id, "ev-2");
}

#[tokio::test]
async fn search_snapshot_excludes_updated_evidence_from_future_version() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-1", "docs", "Original graph token").await;
    commit_evidence(&store, "ev-1", "docs", "Future graph token").await;

    let before_update = store
        .search(GraphSearchRequest {
            query: "Future".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
        })
        .await
        .expect("search should succeed");
    let after_update = store
        .search(GraphSearchRequest {
            query: "Future".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(2),
            limit: 5,
        })
        .await
        .expect("search should succeed");

    assert!(before_update.is_empty());
    assert_eq!(after_update.len(), 1);
    assert_eq!(after_update[0].evidence_id, "ev-1");
}

#[test]
fn stable_entity_ids_are_deterministic() {
    assert_eq!(stable_id("entity", "Rust"), "entity:bffedf1f6f66c727");
    assert_eq!(stable_id("entity", "Rust"), stable_id("entity", "rust"));
}

#[tokio::test]
async fn open_creates_parent_database_directory() {
    let path = std::env::temp_dir()
        .join(format!("relay-knowledge-storage-{}", std::process::id()))
        .join("nested")
        .join("graph.sqlite");
    let _ = std::fs::remove_file(&path);

    let store = SqliteGraphStore::open(&path).expect("store should open");
    let version = store
        .current_graph_version()
        .await
        .expect("version should load");

    assert_eq!(version, GraphVersion::ZERO);
    assert!(path.exists());
}

async fn commit_evidence(store: &SqliteGraphStore, id: &str, source_scope: &str, content: &str) {
    let evidence = EvidenceRecord::new(
        id,
        SourceScope::parse(source_scope).expect("scope should parse"),
        content,
        vec!["Rust".to_owned()],
    )
    .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");

    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");
}
