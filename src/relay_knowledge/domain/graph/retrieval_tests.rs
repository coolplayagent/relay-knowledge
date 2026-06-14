use super::*;

#[test]
fn retriever_source_labels_match_wire_values() {
    assert_eq!(RetrieverSource::Bm25.as_str(), "bm25");
    assert_eq!(RetrieverSource::GraphEvidence.as_str(), "graph_evidence");
    assert_eq!(RetrieverSource::CodeGraph.as_str(), "code_graph");
    assert_eq!(RetrieverSource::Semantic.as_str(), "semantic");
    assert_eq!(RetrieverSource::Vector.as_str(), "vector");
    assert_eq!(RetrieverSource::GraphPath.as_str(), "graph_path");
    assert_eq!(RetrieverSource::Temporal.as_str(), "temporal");
    assert_eq!(
        RetrieverSource::CommunitySummary.as_str(),
        "community_summary"
    );
}

#[test]
fn rerank_mode_labels_match_wire_values() {
    assert_eq!(
        RerankMode::parse("local").expect("local"),
        RerankMode::Local
    );
    assert_eq!(
        RerankMode::parse("external").expect("external"),
        RerankMode::External
    );
    assert_eq!(
        RerankMode::parse("disabled").expect("disabled"),
        RerankMode::Disabled
    );
    assert_eq!(RerankMode::Local.as_str(), "local");
    assert_eq!(RerankMode::External.as_str(), "external");
    assert_eq!(RerankMode::Disabled.as_str(), "disabled");
}

#[test]
fn graph_path_preserves_fact_provenance() {
    let fact = ContextGraphFact {
        fact_id: "rel-1".to_owned(),
        kind: ContextGraphFactKind::Relation,
        subject: "relay-knowledge".to_owned(),
        predicate: "uses".to_owned(),
        object: Some("BM25".to_owned()),
        evidence_ids: vec!["ev-1".to_owned()],
        confidence: ConfidenceScore { basis_points: 9000 },
        status: FactStatus::Accepted,
        version_range: GraphVersionRange::open_from(GraphVersion::new(1)),
    };

    let path = ContextGraphPath::from_fact(&fact);

    assert_eq!(path.path_id, "path:rel-1");
    assert_eq!(path.nodes, ["relay-knowledge", "BM25"]);
    assert_eq!(path.edges[0].evidence_ids, ["ev-1"]);
    assert_eq!(path.edges[0].confidence.basis_points, 9000);
}

#[test]
fn traversal_trace_edge_ids_include_fact_kind() {
    let hit = RetrievalHit {
        evidence_id: "ev-1".to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        source_span: None,
        content: "shared fact id".to_owned(),
        entity_labels: Vec::new(),
        entities: Vec::new(),
        graph_facts: vec![
            test_fact(ContextGraphFactKind::Relation),
            test_fact(ContextGraphFactKind::Claim),
        ],
        code_artifact: None,
        retriever_sources: vec![RetrieverSource::GraphPath],
        ranking: Vec::new(),
        rerank: None,
        score: 1.0,
    };

    let mut trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        Some("docs".to_owned()),
        "direct_context_lookup".to_owned(),
        &[hit],
    );
    trace.apply_budget(8);

    let edge_ids = trace
        .visited_edges
        .iter()
        .map(|edge| edge.edge_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(edge_ids, ["claim:fact-1", "relation:fact-1"]);
}

#[test]
fn traversal_trace_budget_prioritizes_cited_paths() {
    let uncited_hit = test_hit(
        "ev-a",
        test_fact_for(
            ContextGraphFactKind::Claim,
            "fact-a",
            "a-uncited-subject",
            "a-uncited-object",
            "ev-a",
        ),
        1,
    );
    let cited_hit = test_hit(
        "ev-z",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-z",
            "z-cited-subject",
            "z-cited-object",
            "ev-z",
        ),
        99,
    );
    let mut trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        Some("docs".to_owned()),
        "direct_context_lookup".to_owned(),
        &[uncited_hit, cited_hit],
    );
    trace.mark_citations(["ev-z"]);

    trace.apply_budget(1);

    assert_eq!(trace.visited_edges[0].evidence_ids, ["ev-z"]);
    assert!(
        !trace.visited_nodes[0].node_id.contains("uncited"),
        "uncited node should not displace a cited path node"
    );
    assert_eq!(trace.ranking_contributions[0].result_id, "ev-z");
}

#[test]
fn traversal_trace_budget_prioritizes_cited_code_artifacts() {
    let mut uncited_hit = test_hit(
        "ev-a",
        test_fact_for(
            ContextGraphFactKind::Claim,
            "fact-a",
            "a-uncited-subject",
            "a-uncited-object",
            "ev-a",
        ),
        1,
    );
    uncited_hit.graph_facts.clear();
    uncited_hit.entities = vec![ContextEntity {
        id: "a-uncited-entity".to_owned(),
        label: "Uncited".to_owned(),
    }];
    let mut cited_hit = test_hit(
        "ev-z",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-z",
            "z-cited-subject",
            "z-cited-object",
            "ev-z",
        ),
        99,
    );
    cited_hit.graph_facts.clear();
    cited_hit.code_artifact = Some(CodeGraphArtifact {
        kind: CodeGraphArtifactKind::Chunk,
        artifact_id: "artifact-z".to_owned(),
        path: "src/lib.rs".to_owned(),
    });
    let mut trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        Some("docs".to_owned()),
        "direct_context_lookup".to_owned(),
        &[uncited_hit, cited_hit],
    );
    trace.mark_citations(["ev-z"]);

    trace.apply_budget(2);

    assert!(
        trace
            .visited_nodes
            .iter()
            .any(|node| node.node_id == "code:docs:src/lib.rs:chunk:artifact-z")
    );
    assert!(trace.visited_edges.iter().any(|edge| {
        edge.to_node_id.as_deref() == Some("code:docs:src/lib.rs:chunk:artifact-z")
            && edge.evidence_ids == ["ev-z"]
    }));
}

#[test]
fn traversal_trace_budget_prioritizes_child_edges_for_cited_parent() {
    let uncited_hit = test_hit(
        "ev-a",
        test_fact_for(
            ContextGraphFactKind::Claim,
            "fact-a",
            "a-uncited-subject",
            "a-uncited-object",
            "ev-a",
        ),
        1,
    );
    let cited_hit = test_hit(
        "ev-parent",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-z",
            "z-cited-subject",
            "z-cited-object",
            "ev-child",
        ),
        99,
    );
    let mut trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        Some("docs".to_owned()),
        "direct_context_lookup".to_owned(),
        &[uncited_hit, cited_hit],
    );
    trace.mark_citations(["ev-parent"]);

    trace.apply_budget(1);

    assert_eq!(trace.visited_edges[0].edge_id, "relation:fact-z");
    assert_eq!(
        trace.visited_edges[0].evidence_ids,
        ["ev-child", "ev-parent"]
    );
}

#[test]
fn traversal_trace_fact_edges_use_hit_retriever_source() {
    let mut hit = test_hit(
        "ev-bm25",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-bm25",
            "subject",
            "object",
            "ev-bm25",
        ),
        1,
    );
    hit.retriever_sources = vec![RetrieverSource::Bm25];
    hit.ranking[0].source = RetrieverSource::Bm25;

    let trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        Some("docs".to_owned()),
        "direct_context_lookup".to_owned(),
        &[hit],
    );

    assert_eq!(trace.visited_edges[0].source, RetrieverSource::Bm25);
}

#[test]
fn traversal_trace_cites_scoped_evidence_with_reused_ids() {
    let mut first_hit = test_hit(
        "shared-artifact",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-a",
            "subject-a",
            "object-a",
            "shared-artifact",
        ),
        1,
    );
    first_hit.source_scope = "repo-a".to_owned();
    first_hit.source_path = Some("src/a.rs".to_owned());
    let mut second_hit = test_hit(
        "shared-artifact",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-b",
            "subject-b",
            "object-b",
            "shared-artifact",
        ),
        2,
    );
    second_hit.source_scope = "repo-b".to_owned();
    second_hit.source_path = Some("src/b.rs".to_owned());
    let mut trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        None,
        "direct_context_lookup".to_owned(),
        &[first_hit, second_hit],
    );

    trace.mark_citations(["shared-artifact"]);

    let cited_scopes = trace
        .cited_evidence
        .iter()
        .map(|evidence| evidence.source_scope.as_str())
        .collect::<Vec<_>>();
    assert_eq!(cited_scopes, ["repo-a", "repo-b"]);
}

#[test]
fn traversal_trace_marks_citations_by_returned_hit_scope() {
    let mut first_hit = test_hit(
        "shared-artifact",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-a",
            "subject-a",
            "object-a",
            "shared-artifact",
        ),
        1,
    );
    first_hit.source_scope = "repo-a".to_owned();
    first_hit.source_path = Some("src/a.rs".to_owned());
    let mut second_hit = test_hit(
        "shared-artifact",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-b",
            "subject-b",
            "object-b",
            "shared-artifact",
        ),
        2,
    );
    second_hit.source_scope = "repo-b".to_owned();
    second_hit.source_path = Some("src/b.rs".to_owned());
    let mut trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        None,
        "direct_context_lookup".to_owned(),
        &[first_hit.clone(), second_hit],
    );

    trace.mark_citations_for_hits([&first_hit]);

    assert_eq!(trace.cited_evidence.len(), 1);
    assert_eq!(trace.cited_evidence[0].source_scope, "repo-a");
    assert_eq!(
        trace.cited_evidence[0].source_path.as_deref(),
        Some("src/a.rs")
    );
    assert!(
        trace
            .visited_but_uncited
            .iter()
            .any(|evidence| evidence.source_scope == "repo-b")
    );
}

#[test]
fn traversal_trace_budget_prioritizes_returned_hit_scope_for_reused_ids() {
    let mut first_hit = test_hit(
        "shared-artifact",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-a",
            "subject-a",
            "object-a",
            "shared-artifact",
        ),
        1,
    );
    first_hit.graph_facts.clear();
    first_hit.source_scope = "repo-a".to_owned();
    first_hit.source_path = Some("src/a.rs".to_owned());
    first_hit.code_artifact = Some(CodeGraphArtifact {
        kind: CodeGraphArtifactKind::Chunk,
        artifact_id: "chunk-1".to_owned(),
        path: "src/a.rs".to_owned(),
    });
    let mut second_hit = test_hit(
        "shared-artifact",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-b",
            "subject-b",
            "object-b",
            "shared-artifact",
        ),
        2,
    );
    second_hit.graph_facts.clear();
    second_hit.source_scope = "repo-b".to_owned();
    second_hit.source_path = Some("src/b.rs".to_owned());
    second_hit.code_artifact = Some(CodeGraphArtifact {
        kind: CodeGraphArtifactKind::Chunk,
        artifact_id: "chunk-1".to_owned(),
        path: "src/b.rs".to_owned(),
    });
    let mut trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        None,
        "direct_context_lookup".to_owned(),
        &[first_hit.clone(), second_hit],
    );
    trace.mark_citations_for_hits([&first_hit]);

    trace.apply_budget(1);

    assert!(
        trace
            .visited_nodes
            .iter()
            .all(|node| node.source_scope.as_deref() == Some("repo-a"))
    );
    assert_eq!(trace.visited_edges.len(), 1);
    assert_eq!(
        trace.visited_edges[0].source_scope.as_deref(),
        Some("repo-a")
    );
}

#[test]
fn traversal_trace_budget_scopes_cited_edge_endpoint_nodes() {
    let mut uncited_hit = test_hit(
        "shared-uncited",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-a",
            "shared-subject",
            "shared-object",
            "shared-uncited",
        ),
        1,
    );
    uncited_hit.source_scope = "repo-a".to_owned();
    uncited_hit.source_path = Some("src/shared.rs".to_owned());
    let mut cited_hit = test_hit(
        "shared-cited",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-b",
            "shared-subject",
            "shared-object",
            "shared-cited",
        ),
        2,
    );
    cited_hit.source_scope = "repo-b".to_owned();
    cited_hit.source_path = Some("src/shared.rs".to_owned());
    let mut trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        None,
        "direct_context_lookup".to_owned(),
        &[uncited_hit, cited_hit.clone()],
    );
    trace.mark_citations_for_hits([&cited_hit]);

    trace.apply_budget(1);

    assert_eq!(trace.visited_nodes.len(), 1);
    assert_eq!(
        trace.visited_nodes[0].source_scope.as_deref(),
        Some("repo-b")
    );
}

#[test]
fn traversal_trace_retains_edge_endpoint_nodes_by_scope() {
    let mut dropped_hit = test_hit(
        "shared-dropped",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-a",
            "shared-subject",
            "shared-object",
            "shared-dropped",
        ),
        1,
    );
    dropped_hit.source_scope = "repo-a".to_owned();
    dropped_hit.source_path = Some("src/shared.rs".to_owned());
    let mut retained_hit = test_hit(
        "shared-retained",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-b",
            "shared-subject",
            "shared-object",
            "shared-retained",
        ),
        2,
    );
    retained_hit.source_scope = "repo-b".to_owned();
    retained_hit.source_path = Some("src/shared.rs".to_owned());
    let mut trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        None,
        "direct_context_lookup".to_owned(),
        &[dropped_hit, retained_hit.clone()],
    );

    trace.retain_hits([&retained_hit]);

    assert!(
        trace
            .visited_nodes
            .iter()
            .all(|node| node.source_scope.as_deref() == Some("repo-b"))
    );
}

#[test]
fn traversal_trace_deduplicates_code_artifact_edges_by_scoped_identity() {
    let mut first_hit = test_hit(
        "shared-artifact",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-a",
            "subject-a",
            "object-a",
            "shared-artifact",
        ),
        1,
    );
    first_hit.graph_facts.clear();
    first_hit.source_scope = "repo-a".to_owned();
    first_hit.code_artifact = Some(CodeGraphArtifact {
        kind: CodeGraphArtifactKind::Chunk,
        artifact_id: "chunk-1".to_owned(),
        path: "src/lib.rs".to_owned(),
    });
    let mut second_hit = test_hit(
        "shared-artifact",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-b",
            "subject-b",
            "object-b",
            "shared-artifact",
        ),
        2,
    );
    second_hit.graph_facts.clear();
    second_hit.source_scope = "repo-b".to_owned();
    second_hit.code_artifact = Some(CodeGraphArtifact {
        kind: CodeGraphArtifactKind::Chunk,
        artifact_id: "chunk-1".to_owned(),
        path: "src/lib.rs".to_owned(),
    });
    let mut trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        None,
        "direct_context_lookup".to_owned(),
        &[first_hit, second_hit],
    );
    trace.mark_citations(["shared-artifact"]);

    trace.apply_budget(8);

    let artifact_edges = trace
        .visited_edges
        .iter()
        .filter(|edge| edge.predicate.as_deref() == Some("code_artifact"))
        .collect::<Vec<_>>();
    assert_eq!(artifact_edges.len(), 2);
    assert_ne!(artifact_edges[0].edge_id, artifact_edges[1].edge_id);
}

#[test]
fn traversal_trace_retains_direct_entity_nodes_for_retained_evidence() {
    let mut retained_hit = test_hit(
        "ev-retained",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-retained",
            "subject-retained",
            "object-retained",
            "ev-retained",
        ),
        1,
    );
    retained_hit.graph_facts.clear();
    retained_hit.entities = vec![ContextEntity {
        id: "entity-retained".to_owned(),
        label: "Retained".to_owned(),
    }];
    let mut dropped_hit = test_hit(
        "ev-dropped",
        test_fact_for(
            ContextGraphFactKind::Relation,
            "fact-dropped",
            "subject-dropped",
            "object-dropped",
            "ev-dropped",
        ),
        2,
    );
    dropped_hit.graph_facts.clear();
    dropped_hit.entities = vec![ContextEntity {
        id: "entity-dropped".to_owned(),
        label: "Dropped".to_owned(),
    }];
    let mut trace = TraversalProvenanceTrace::from_hits(
        GraphVersion::new(1),
        Some("docs".to_owned()),
        "direct_context_lookup".to_owned(),
        &[retained_hit.clone(), dropped_hit],
    );

    trace.retain_hits([&retained_hit]);

    assert!(
        trace
            .visited_nodes
            .iter()
            .any(|node| node.node_id == "entity-retained")
    );
    assert!(
        !trace
            .visited_nodes
            .iter()
            .any(|node| node.node_id == "entity-dropped")
    );
}

fn test_hit(evidence_id: &str, fact: ContextGraphFact, rank: usize) -> RetrievalHit {
    RetrievalHit {
        evidence_id: evidence_id.to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        source_span: None,
        content: format!("{evidence_id} content"),
        entity_labels: Vec::new(),
        entities: Vec::new(),
        graph_facts: vec![fact],
        code_artifact: None,
        retriever_sources: vec![RetrieverSource::GraphPath],
        ranking: vec![RankingSignal {
            source: RetrieverSource::GraphPath,
            rank,
            score: 1.0 / rank as f64,
            explanation: "test ranking".to_owned(),
        }],
        rerank: None,
        score: 1.0,
    }
}

fn test_fact(kind: ContextGraphFactKind) -> ContextGraphFact {
    test_fact_for(kind, "fact-1", "relay-knowledge", "GraphRAG", "ev-1")
}

fn test_fact_for(
    kind: ContextGraphFactKind,
    fact_id: &str,
    subject: &str,
    object: &str,
    evidence_id: &str,
) -> ContextGraphFact {
    ContextGraphFact {
        fact_id: fact_id.to_owned(),
        kind,
        subject: subject.to_owned(),
        predicate: "supports".to_owned(),
        object: Some(object.to_owned()),
        evidence_ids: vec![evidence_id.to_owned()],
        confidence: ConfidenceScore { basis_points: 9000 },
        status: FactStatus::Accepted,
        version_range: GraphVersionRange::open_from(GraphVersion::new(1)),
    }
}
