use super::*;
use crate::{
    domain::{
        ClaimRecord, EventRecord, EvidenceRecord, FactStatus, GraphMutationBatch,
        GraphRelationRecord, GraphVersionRange, SourceScope,
    },
    storage::{GraphCanvasSelection, GraphCanvasStorageRequest, GraphStore},
};

#[test]
fn fact_evidence_ids_preserve_order_and_reject_invalid_json() {
    assert_eq!(
        evidence_ids(r#"["ev-2","ev-1"]"#).expect("evidence JSON should decode"),
        ["ev-2", "ev-1"]
    );
    assert!(matches!(
        evidence_ids("not-json"),
        Err(StorageError::InvalidInput(_))
    ));
}

#[tokio::test]
async fn canvas_projects_structured_fact_nodes_and_edges() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-structured",
        scope.clone(),
        "Relay Knowledge documents relation, claim, and event canvas rendering",
        vec!["Relay Knowledge".to_owned()],
    )
    .expect("evidence should validate");
    let relation = GraphRelationRecord::new(
        "rel-structured",
        scope.clone(),
        "Relay Knowledge",
        "renders",
        "graph canvas",
        vec!["ev-structured".to_owned()],
    )
    .expect("relation should validate");
    let claim = ClaimRecord::new(
        "claim-structured",
        scope.clone(),
        "Relay Knowledge",
        "canvas_mode",
        "keeps structured facts selectable",
        vec!["ev-structured".to_owned()],
    )
    .expect("claim should validate")
    .with_metadata(
        crate::domain::ConfidenceScore {
            basis_points: 8_750,
        },
        FactStatus::Proposed,
        crate::domain::GraphVersionRange::open_from(GraphVersion::ZERO),
    )
    .expect("claim metadata should validate");
    let event = EventRecord::new(
        "event-structured",
        scope,
        "canvas_refreshed",
        vec!["Relay Knowledge".to_owned()],
        Some("2026-05-15T10:00:00Z".to_owned()),
        vec!["ev-structured".to_owned()],
    )
    .expect("event should validate");
    store
        .commit_mutation_batch(
            GraphMutationBatch::with_facts(
                vec![evidence],
                vec![relation],
                vec![claim],
                vec![event],
            )
            .expect("batch should validate"),
        )
        .await
        .expect("commit should succeed");

    let snapshot = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Knowledge,
            source_scope: Some("docs".to_owned()),
            query: None,
            graph_version: GraphVersion::new(1),
            limit: 50,
        })
        .await
        .expect("canvas should load");

    let relation = snapshot
        .edges
        .iter()
        .find(|edge| edge.id == "relation:rel-structured")
        .expect("relation edge should be projected");
    assert_eq!(relation.kind, "relation");
    assert_eq!(relation.confidence_basis_points, Some(10_000));
    assert_eq!(relation.evidence_count, Some(1));
    assert_eq!(
        relation.details.get("relation_type").map(String::as_str),
        Some("renders")
    );
    let claim = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "claim:claim-structured")
        .expect("claim node should be projected");
    assert_eq!(claim.status.as_deref(), Some("proposed"));
    assert_eq!(
        claim.details.get("confidence").map(String::as_str),
        Some("8750")
    );
    let event = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "event:event-structured")
        .expect("event node should be projected");
    assert_eq!(event.kind, "event");
    assert!(
        event
            .label
            .contains("canvas_refreshed @ 2026-05-15T10:00:00Z")
    );
    assert!(
        snapshot
            .edges
            .iter()
            .any(|edge| edge.kind == "claim_subject" && edge.target == "claim:claim-structured")
    );
    assert!(
        snapshot
            .edges
            .iter()
            .any(|edge| edge.kind == "event_entity" && edge.source == "event:event-structured")
    );
    assert!(
        snapshot
            .available_kinds
            .iter()
            .any(|kind| kind == "evidence_link")
    );
}

#[tokio::test]
async fn canvas_filters_structured_facts_by_validity_window() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-window",
        scope.clone(),
        "Canvas validity windows should match retrieval visibility",
        vec!["Windowed Graph".to_owned()],
    )
    .expect("evidence should validate");
    let future_relation = GraphRelationRecord::new(
        "rel-future",
        scope.clone(),
        "Windowed Graph",
        "appears_at",
        "future snapshot",
        vec!["ev-window".to_owned()],
    )
    .expect("relation should validate")
    .with_metadata(
        crate::domain::ConfidenceScore::CERTAIN,
        FactStatus::Accepted,
        GraphVersionRange::open_from(GraphVersion::new(3)),
    )
    .expect("relation metadata should validate");
    let expired_claim = ClaimRecord::new(
        "claim-expired",
        scope.clone(),
        "Windowed Graph",
        "visibility",
        "expired before snapshot",
        vec!["ev-window".to_owned()],
    )
    .expect("claim should validate")
    .with_metadata(
        crate::domain::ConfidenceScore::CERTAIN,
        FactStatus::Accepted,
        GraphVersionRange::new(GraphVersion::new(1), Some(GraphVersion::new(1)))
            .expect("range should validate"),
    )
    .expect("claim metadata should validate");
    let expired_event = EventRecord::new(
        "event-expired",
        scope,
        "window_closed",
        vec!["Windowed Graph".to_owned()],
        None,
        vec!["ev-window".to_owned()],
    )
    .expect("event should validate")
    .with_metadata(
        crate::domain::ConfidenceScore::CERTAIN,
        FactStatus::Accepted,
        GraphVersionRange::new(GraphVersion::new(1), Some(GraphVersion::new(1)))
            .expect("range should validate"),
    )
    .expect("event metadata should validate");
    store
        .commit_mutation_batch(
            GraphMutationBatch::with_facts(
                vec![evidence],
                vec![future_relation],
                vec![expired_claim],
                vec![expired_event],
            )
            .expect("batch should validate"),
        )
        .await
        .expect("commit should succeed");

    let before_future = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Knowledge,
            source_scope: Some("docs".to_owned()),
            query: None,
            graph_version: GraphVersion::new(2),
            limit: 50,
        })
        .await
        .expect("canvas should load");
    assert!(
        before_future
            .edges
            .iter()
            .all(|edge| edge.id != "relation:rel-future")
    );
    assert!(
        before_future
            .nodes
            .iter()
            .all(|node| node.id != "claim:claim-expired")
    );
    assert!(
        before_future
            .nodes
            .iter()
            .all(|node| node.id != "event:event-expired")
    );

    let at_future = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Knowledge,
            source_scope: Some("docs".to_owned()),
            query: None,
            graph_version: GraphVersion::new(3),
            limit: 50,
        })
        .await
        .expect("canvas should load");
    assert!(
        at_future
            .edges
            .iter()
            .any(|edge| edge.id == "relation:rel-future")
    );
    assert!(
        at_future
            .nodes
            .iter()
            .all(|node| node.id != "claim:claim-expired" && node.id != "event:event-expired")
    );
}

#[tokio::test]
async fn canvas_fact_scope_filters_ignore_future_evidence_scope_changes() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let docs_scope = SourceScope::parse("docs").expect("scope should parse");
    let repo_scope = SourceScope::parse("repo").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-scope-drift",
        docs_scope.clone(),
        "Scope Drift belongs to docs at the first snapshot",
        vec!["Scope Drift".to_owned(), "Relay".to_owned()],
    )
    .expect("evidence should validate");
    let relation = GraphRelationRecord::new(
        "rel-scope-drift",
        docs_scope.clone(),
        "Scope Drift",
        "documents",
        "Relay",
        vec!["ev-scope-drift".to_owned()],
    )
    .expect("relation should validate");
    let claim = ClaimRecord::new(
        "claim-scope-drift",
        docs_scope.clone(),
        "Scope Drift",
        "scope",
        "docs",
        vec!["ev-scope-drift".to_owned()],
    )
    .expect("claim should validate");
    let event = EventRecord::new(
        "event-scope-drift",
        docs_scope,
        "scope_recorded",
        vec!["Scope Drift".to_owned()],
        None,
        vec!["ev-scope-drift".to_owned()],
    )
    .expect("event should validate");
    store
        .commit_mutation_batch(
            GraphMutationBatch::with_facts(
                vec![evidence],
                vec![relation],
                vec![claim],
                vec![event],
            )
            .expect("batch should validate"),
        )
        .await
        .expect("initial facts should commit");

    let moved_evidence = EvidenceRecord::new(
        "ev-scope-drift",
        repo_scope,
        "Scope Drift is reingested under repo later",
        vec!["Scope Drift".to_owned(), "Relay".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![moved_evidence]).expect("batch"))
        .await
        .expect("moved evidence should commit");

    let old_repo_scope = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Knowledge,
            source_scope: Some("repo".to_owned()),
            query: None,
            graph_version: GraphVersion::new(1),
            limit: 80,
        })
        .await
        .expect("canvas should load");

    assert!(
        old_repo_scope
            .edges
            .iter()
            .all(|edge| edge.id != "relation:rel-scope-drift")
    );
    assert!(
        old_repo_scope
            .nodes
            .iter()
            .all(|node| node.id != "claim:claim-scope-drift")
    );
    assert!(
        old_repo_scope
            .nodes
            .iter()
            .all(|node| node.id != "event:event-scope-drift")
    );
}
