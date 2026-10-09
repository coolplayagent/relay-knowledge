use super::super::{
    ResearchService,
    test_support::{TEST_LOCK, fixture, save},
};
use super::*;

#[tokio::test]
async fn keeps_author_claims_separate_and_limits_hash_impact_to_dependent_relations() {
    let _guard = TEST_LOCK.lock().await;
    let (root, bundle, _) = fixture().await;
    let research = ResearchService::new(root.clone());
    let report = research
        .validate_bundle("bundle.json".into(), "research".into())
        .await
        .unwrap();
    assert!(report.valid);
    assert_eq!((report.node_count, report.edge_count), (27, 38));
    assert!(
        report
            .relations
            .iter()
            .all(|relation| relation.state == "proposed")
    );
    assert_eq!(
        report.relations[0].interpretations,
        [EvidenceInterpretation::UserScopeConfirmation]
    );
    assert!(
        research
            .validate_bundle("bundle.json".into(), "foreign".into())
            .await
            .is_err()
    );
    tokio::fs::write(root.join("a.txt"), b"changed")
        .await
        .unwrap();
    let changed = research
        .validate_bundle("bundle.json".into(), "research".into())
        .await
        .unwrap();
    assert!(!changed.valid);
    for (index, relation) in changed.relations.iter().enumerate() {
        assert_eq!(
            relation.state,
            if index % 2 == 0 {
                "needs_review"
            } else {
                "proposed"
            }
        );
    }
    assert_eq!(bundle.graph.edges[0].metadata["status"], "user-confirmed");
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn rejects_dangling_edges_duplicate_ids_missing_evidence_and_scope_crossing() {
    let _guard = TEST_LOCK.lock().await;
    let (root, mut bundle, _) = fixture().await;
    bundle.graph.nodes[1].id = bundle.graph.nodes[0].id.clone();
    bundle.graph.edges[0].source = "missing".into();
    bundle.graph.edges[1].id = bundle.graph.edges[0].id.clone();
    bundle.graph.edges[2].evidence.clear();
    bundle.evidence[0].source_scope = "foreign".into();
    bundle.evidence[1].id = "a".into();
    save(&root, &bundle).await;
    let report = ResearchService::new(root.clone())
        .validate_bundle("bundle.json".into(), "research".into())
        .await
        .unwrap();
    for code in [
        "duplicate_node",
        "dangling_edge",
        "duplicate_relation",
        "missing_or_changed_evidence",
        "scope_violation",
        "duplicate_evidence",
    ] {
        assert!(report.diagnostics.iter().any(|d| d.code == code), "{code}");
    }
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn validates_exact_byte_and_line_spans_and_returns_lossless_neighborhood() {
    let _guard = TEST_LOCK.lock().await;
    let (root, mut bundle, _) = fixture().await;
    let research = ResearchService::new(root.clone());
    let view = research
        .bundle_view("bundle.json".into(), "research".into(), None)
        .await
        .unwrap();
    assert_eq!(
        view["view"]["graph"],
        serde_json::to_value(&bundle.graph).unwrap()
    );
    let neighborhood = research
        .bundle_view(
            "bundle.json".into(),
            "research".into(),
            Some("concept-0".into()),
        )
        .await
        .unwrap();
    assert_eq!(neighborhood["view"]["hops"], 1);
    assert!(
        neighborhood["view"]["graph"]["nodes"]
            .as_array()
            .unwrap()
            .len()
            < 27
    );
    assert!(
        research
            .bundle_view(
                "bundle.json".into(),
                "research".into(),
                Some("missing".into())
            )
            .await
            .is_err()
    );
    bundle.evidence[0].span.as_mut().unwrap().end_byte = 200;
    bundle.evidence[1].span.as_mut().unwrap().start_line = 2;
    bundle.evidence[1].span.as_mut().unwrap().end_line = 2;
    save(&root, &bundle).await;
    let report = research
        .validate_bundle("bundle.json".into(), "research".into())
        .await
        .unwrap();
    assert!(report.diagnostics.iter().any(|d| d.code == "invalid_span"));
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "invalid_span_lines")
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}
