use super::*;
use crate::domain::{
    ClaimRecord, EventRecord, EvidenceRecord, GraphRelationRecord, RetrieverSource, SourceScope,
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
