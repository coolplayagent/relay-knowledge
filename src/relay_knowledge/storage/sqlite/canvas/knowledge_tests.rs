use super::*;
use crate::{
    domain::{EvidenceRecord, GraphMutationBatch, SourceScope},
    storage::{GraphCanvasSelection, GraphCanvasStorageRequest, GraphStore},
};

#[tokio::test]
async fn knowledge_projection_materializes_entities_evidence_and_links() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let evidence = EvidenceRecord::new(
        "ev-knowledge-owner",
        SourceScope::parse("docs").expect("scope should parse"),
        "Relay knowledge graph canvas",
        vec!["Relay".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
        .await
        .expect("commit should succeed");

    let snapshot = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Knowledge,
            source_scope: Some("docs".to_owned()),
            query: Some("Relay".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 20,
        })
        .await
        .expect("canvas should load");

    assert!(snapshot.nodes.iter().any(|node| node.kind == "entity"));
    assert!(snapshot.nodes.iter().any(|node| node.kind == "evidence"));
    assert!(
        snapshot
            .edges
            .iter()
            .any(|edge| edge.kind == "evidence_link")
    );
}

#[tokio::test]
async fn canvas_entity_scope_filter_respects_snapshot_bounded_evidence() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let notes = EvidenceRecord::new(
        "ev-notes",
        SourceScope::parse("notes").expect("scope should parse"),
        "Shared Entity first appears in notes",
        vec!["Shared Entity".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![notes]).expect("batch"))
        .await
        .expect("first evidence should commit");
    let docs = EvidenceRecord::new(
        "ev-docs",
        SourceScope::parse("docs").expect("scope should parse"),
        "Shared Entity later appears in docs",
        vec!["Shared Entity".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![docs]).expect("batch"))
        .await
        .expect("second evidence should commit");

    let before_docs = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Knowledge,
            source_scope: Some("docs".to_owned()),
            query: Some("Shared Entity".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 20,
        })
        .await
        .expect("canvas should load");
    assert!(
        before_docs
            .nodes
            .iter()
            .all(|node| node.label != "Shared Entity")
    );

    let after_docs = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Knowledge,
            source_scope: Some("docs".to_owned()),
            query: Some("Shared Entity".to_owned()),
            graph_version: GraphVersion::new(2),
            limit: 20,
        })
        .await
        .expect("canvas should load");
    assert!(after_docs.nodes.iter().any(|node| {
        node.kind == "entity"
            && node.label == "Shared Entity"
            && node.source_scope.as_deref() == Some("docs")
    }));
}
