use super::*;
use crate::domain::{EvidenceRecord, SourceScope};

#[tokio::test]
async fn older_index_refresh_does_not_regress_metadata() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .mark_refresh_complete(IndexKind::Semantic, GraphVersion::new(2))
        .await
        .expect("newer refresh should succeed");

    let stale_completion = store
        .mark_refresh_complete(IndexKind::Semantic, GraphVersion::new(1))
        .await
        .expect("older completion should not fail");

    assert_eq!(stale_completion.index_version, 1);
    assert_eq!(stale_completion.indexed_graph_version, GraphVersion::new(2));
}

#[tokio::test]
async fn missing_index_status_row_fails_refresh() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(|connection| {
            connection.execute("DELETE FROM index_status WHERE kind = 'vector'", [])?;
            Ok(())
        })
        .await
        .expect("fixture corruption should succeed");

    let error = store
        .mark_refresh_complete(IndexKind::Vector, GraphVersion::new(1))
        .await
        .expect_err("missing row should fail");

    assert_eq!(
        error.to_string(),
        "invalid storage input: index status row for 'vector' is missing"
    );
}

#[tokio::test]
async fn missing_required_index_status_row_fails_status_read() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(|connection| {
            connection.execute("DELETE FROM index_status WHERE kind = 'semantic'", [])?;
            Ok(())
        })
        .await
        .expect("fixture corruption should succeed");

    let error = store
        .index_statuses()
        .await
        .expect_err("missing row should fail");

    assert_eq!(
        error.to_string(),
        "invalid storage input: required index status row for 'semantic' is missing in storage metadata"
    );
}

#[tokio::test]
async fn unknown_index_kind_in_metadata_is_rejected() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(|connection| {
            connection.execute(
                "INSERT INTO index_status
                 (kind, index_version, indexed_graph_version, state, last_error)
                 VALUES ('future', 0, 0, 'fresh', NULL)",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("fixture corruption should succeed");

    let error = store
        .index_statuses()
        .await
        .expect_err("unknown kind should fail");

    assert_eq!(
        error.to_string(),
        "invalid storage input: unknown index kind 'future' in storage metadata"
    );
}

#[tokio::test]
async fn unknown_index_state_in_metadata_is_rejected() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(|connection| {
            connection.execute(
                "UPDATE index_status SET state = 'mystery' WHERE kind = 'bm25'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("fixture corruption should succeed");

    let error = store
        .index_statuses()
        .await
        .expect_err("unknown state should fail");

    assert_eq!(
        error.to_string(),
        "invalid storage input: unknown index state 'mystery' in storage metadata"
    );
}

#[tokio::test]
async fn update_removes_unreferenced_entities() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let rust = EvidenceRecord::new(
        "ev-1",
        scope.clone(),
        "Graph storage",
        vec!["Rust".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![rust]).expect("batch"))
        .await
        .expect("first commit should succeed");
    let sqlite = EvidenceRecord::new("ev-1", scope, "Graph storage", vec!["SQLite".to_owned()])
        .expect("evidence should validate");

    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![sqlite]).expect("batch"))
        .await
        .expect("second commit should succeed");
    let graph = store.inspect_graph().await.expect("graph should inspect");

    assert_eq!(graph.entity_count, 1);
}

#[tokio::test]
async fn search_preserves_entity_labels_with_unit_separator() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let label = "Alpha\u{1f}Beta".to_owned();
    let evidence = EvidenceRecord::new("ev-separator", scope, "Needle evidence", vec![label])
        .expect("evidence should validate");
    let receipt = store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
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

    assert_eq!(hits[0].entity_labels, ["Alpha\u{1f}Beta"]);
}

#[tokio::test]
async fn search_orders_equal_scores_by_evidence_id() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let evidence = ["ev-c", "ev-a", "ev-b"]
        .into_iter()
        .map(|id| EvidenceRecord::new(id, scope.clone(), "Tie token", Vec::new()))
        .collect::<Result<Vec<_>, _>>()
        .expect("evidence should validate");
    let receipt = store
        .commit_mutation_batch(GraphMutationBatch::new(evidence).expect("batch"))
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "Tie".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: receipt.graph_version,
            limit: 2,
        })
        .await
        .expect("search should succeed");
    let ids = hits
        .iter()
        .map(|hit| hit.evidence_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(ids, ["ev-a", "ev-b"]);
}
