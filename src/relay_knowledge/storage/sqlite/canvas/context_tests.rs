use std::collections::BTreeMap;

use super::*;

fn node(id: &str, kind: &str) -> GraphCanvasStorageNode {
    GraphCanvasStorageNode {
        id: id.to_owned(),
        kind: kind.to_owned(),
        label: id.to_owned(),
        subtitle: None,
        source_scope: None,
        graph_version: GraphVersion::new(1),
        weight: 1,
        status: None,
        details: BTreeMap::new(),
    }
}

#[test]
fn canvas_filter_normalizes_text_and_overfetches_one_row() {
    let filter = CanvasFilter::new(
        Some(" scope ".to_owned()),
        Some(" ".to_owned()),
        GraphVersion::new(3),
        8,
    );

    assert_eq!(filter.source_scope.as_deref(), Some("scope"));
    assert_eq!(filter.query, None);
    assert_eq!(filter.sql_limit(), 9);
}

#[test]
fn canvas_builder_requires_edge_endpoints_and_tracks_kinds() {
    let mut builder = CanvasBuilder::new(4);
    builder.insert_node(node("first", "entity"));
    builder.insert_edge(GraphCanvasStorageEdge {
        id: "missing-edge".to_owned(),
        kind: "relation".to_owned(),
        source: "first".to_owned(),
        target: "missing".to_owned(),
        label: "relates".to_owned(),
        graph_version: GraphVersion::new(1),
        confidence_basis_points: None,
        evidence_count: None,
        details: BTreeMap::new(),
    });
    builder.insert_node(node("second", "claim"));
    builder.insert_edge(GraphCanvasStorageEdge {
        id: "edge".to_owned(),
        kind: "relation".to_owned(),
        source: "first".to_owned(),
        target: "second".to_owned(),
        label: "relates".to_owned(),
        graph_version: GraphVersion::new(1),
        confidence_basis_points: None,
        evidence_count: None,
        details: BTreeMap::new(),
    });
    let snapshot = builder.into_snapshot();

    assert_eq!(snapshot.edges.len(), 1);
    assert_eq!(snapshot.available_kinds, ["claim", "entity", "relation"]);
}

#[test]
fn canvas_builder_reports_query_and_item_budget_truncation() {
    let mut builder = CanvasBuilder::new(1);
    builder.observe_query_len(2);
    builder.insert_node(node("first", "entity"));
    builder.insert_node(node("second", "entity"));

    let snapshot = builder.into_snapshot();
    assert!(snapshot.truncated);
    assert_eq!(snapshot.nodes.len(), 1);
}
