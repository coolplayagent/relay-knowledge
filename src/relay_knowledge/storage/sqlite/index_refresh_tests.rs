use super::*;
use crate::domain::{EvidenceRecord, GraphRelationRecord, IndexState, SourceScope};
use crate::storage::{IndexRefreshClaimRequest, IndexRefreshCompletion, IndexRefreshQueueRequest};

#[tokio::test]
async fn marks_index_refresh_complete_at_graph_version() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");

    let status = store
        .mark_refresh_complete(IndexKind::Vector, GraphVersion::new(7))
        .await
        .expect("refresh should update metadata");

    assert_eq!(status.kind, IndexKind::Vector);
    assert_eq!(status.index_version, 1);
    assert_eq!(status.indexed_graph_version, GraphVersion::new(7));
    assert_eq!(status.state, IndexState::Fresh);
}

#[tokio::test]
async fn default_index_completion_preserves_scoped_stale_cursors() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-default-stale", "docs", "Rust async storage").await;

    let status = store
        .mark_refresh_complete(IndexKind::Bm25, GraphVersion::new(1))
        .await
        .expect("completion should update default cursor");

    assert_eq!(status.kind, IndexKind::Bm25);
    assert_eq!(status.state, IndexState::Stale);
    assert!(status.is_stale_for(GraphVersion::new(1)));
}

#[tokio::test]
async fn moving_existing_evidence_marks_old_and_new_scopes_stale() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-move", "docs", "Rust async storage").await;
    complete_bm25_refresh(&store, GraphVersion::new(1)).await;

    let moved = EvidenceRecord::new(
        "ev-move",
        SourceScope::parse("repo").expect("scope should parse"),
        "Rust async storage moved",
        Vec::new(),
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![moved]).expect("batch"))
        .await
        .expect("move commit should succeed");
    let entries = store
        .read_after(GraphVersion::new(1), 10)
        .await
        .expect("mutation log should load");
    let cursors = store.index_cursors().await.expect("cursors should load");

    assert_eq!(entries[0].affected_scopes, ["docs", "repo"]);
    for scope in ["docs", "repo"] {
        let cursor = cursors
            .iter()
            .find(|cursor| cursor.kind == IndexKind::Bm25 && cursor.source_scope == scope)
            .expect("scope cursor should exist");
        assert_eq!(cursor.state, IndexState::Stale);
    }
}

#[tokio::test]
async fn structured_fact_evidence_references_mark_evidence_scopes_stale() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(
        &store,
        "ev-structured-scope",
        "docs",
        "Structured fact evidence scope",
    )
    .await;
    complete_bm25_refresh(&store, GraphVersion::new(1)).await;
    let source_scope = SourceScope::parse("docs").expect("scope should parse");
    let relation = GraphRelationRecord::new(
        "rel-scope",
        source_scope,
        "relay-knowledge",
        "uses",
        "scoped evidence",
        vec!["ev-structured-scope".to_owned()],
    )
    .expect("relation should validate");
    let batch = GraphMutationBatch::with_facts(Vec::new(), vec![relation], Vec::new(), Vec::new())
        .expect("structured graph facts should validate");

    store
        .commit_mutation_batch(batch)
        .await
        .expect("structured fact commit should succeed");
    let entries = store
        .read_after(GraphVersion::new(1), 10)
        .await
        .expect("mutation log should load");
    let cursors = store.index_cursors().await.expect("cursors should load");
    let bm25_cursor = cursors
        .iter()
        .find(|cursor| cursor.kind == IndexKind::Bm25 && cursor.source_scope == "docs")
        .expect("docs cursor should exist");

    assert_eq!(entries[0].affected_scopes, ["docs"]);
    assert_eq!(bm25_cursor.state, IndexState::Stale);
}

#[tokio::test]
async fn fallback_refresh_task_uses_status_cursor_when_scoped_cursors_are_missing() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(|connection| {
            connection.execute(
                "
                UPDATE index_status
                SET index_version = 3,
                    indexed_graph_version = 5,
                    state = 'stale'
                WHERE kind = 'bm25'
                ",
                [],
            )?;
            connection.execute("DELETE FROM index_cursors WHERE kind = 'bm25'", [])?;
            Ok(())
        })
        .await
        .expect("migration fixture should be applied");

    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(6),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 10,
        })
        .await
        .expect("fallback task should queue");
    let task = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 11,
        })
        .await
        .expect("claim should load")
        .expect("fallback task should be claimed");

    assert_eq!(task.source_scope, "graph");
    assert_eq!(task.cursor_before, GraphVersion::new(5));
    assert_eq!(task.target_graph_version, GraphVersion::new(6));
}

#[tokio::test]
async fn partial_cursor_loss_marks_status_stale_and_requeues_from_zero() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-missing-docs", "docs", "Rust async storage").await;
    commit_evidence(&store, "ev-missing-repo", "repo", "Rust graph indexing").await;
    store
        .run(|connection| {
            connection.execute(
                "
                UPDATE index_cursors
                SET state = 'fresh', indexed_graph_version = 2
                WHERE kind = 'bm25'
                ",
                [],
            )?;
            connection.execute(
                "
                UPDATE index_status
                SET index_version = 3,
                    indexed_graph_version = 2,
                    state = 'fresh',
                    last_error = NULL
                WHERE kind = 'bm25'
                ",
                [],
            )?;
            connection.execute(
                "
                DELETE FROM index_cursors
                WHERE kind = 'bm25' AND source_scope = 'docs'
                ",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("partial cursor loss fixture should be applied");

    let statuses = store.index_statuses().await.expect("statuses should load");
    let bm25 = statuses
        .iter()
        .find(|status| status.kind == IndexKind::Bm25)
        .expect("bm25 status should exist");
    assert_eq!(bm25.state, IndexState::Stale);
    assert_eq!(
        bm25.last_error.as_deref(),
        Some("1 scoped index cursor(s) missing")
    );

    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(2),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 10,
        })
        .await
        .expect("missing cursor should queue recovery work");
    let task = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 11,
        })
        .await
        .expect("claim should load")
        .expect("missing cursor task should be claimed");

    assert_eq!(task.source_scope, "docs");
    assert_eq!(task.cursor_before, GraphVersion::ZERO);
    assert_eq!(task.target_graph_version, GraphVersion::new(2));
}

#[tokio::test]
async fn queued_task_claim_uses_latest_target_after_extension() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-queued-v1", "docs", "Rust async storage").await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 10,
        })
        .await
        .expect("initial task should queue");

    commit_evidence(&store, "ev-queued-v2", "docs", "Rust graph indexing").await;
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(2),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 11,
        })
        .await
        .expect("queued task should extend to newer target");
    let task = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 12,
        })
        .await
        .expect("claim should load")
        .expect("task should be claimed");

    assert_eq!(task.source_scope, "docs");
    assert_eq!(task.cursor_before, GraphVersion::ZERO);
    assert_eq!(task.target_graph_version, GraphVersion::new(2));
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

async fn complete_bm25_refresh(store: &SqliteGraphStore, graph_version: GraphVersion) {
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: graph_version,
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 10,
        })
        .await
        .expect("bm25 task should queue");
    let task = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "test-worker".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 11,
        })
        .await
        .expect("claim should load")
        .expect("task should be claimed");
    store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: task.task_id,
            lease_owner: "test-worker".to_owned(),
            attempt_count: task.attempt_count,
            indexed_graph_version: graph_version,
            model_name: None,
            model_dimension: None,
            now_ms: 12,
        })
        .await
        .expect("bm25 task should complete");
}
