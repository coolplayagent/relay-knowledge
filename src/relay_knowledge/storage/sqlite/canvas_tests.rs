use super::*;
use crate::{
    domain::{
        ClaimRecord, CodeExtractionMetadata, CodeFileFields, CodeFileRecord, CodeGraphBatch,
        CodeParseStatus, CodeRange, CodeReferenceFields, CodeReferenceKind, CodeReferenceRecord,
        CodeResolutionState, CodeSymbolKind, CodeSymbolRecord, EventRecord, EvidenceRecord,
        FactStatus, GraphMutationBatch, GraphRelationRecord, GraphVersionRange, SourceScope,
    },
    storage::{CodeGraphStore, GraphStore},
};

#[tokio::test]
async fn canvas_projects_knowledge_nodes_and_edges() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-1",
        scope,
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
    assert!(!snapshot.truncated);
}

#[tokio::test]
async fn canvas_projects_code_nodes_and_truncation() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let extraction = CodeExtractionMetadata::new("rust", "symbols", "1", "function_item", "name")
        .expect("extraction should validate");
    let symbol = CodeSymbolRecord::new(
        "sym-main",
        scope.clone(),
        "src/main.rs",
        "main",
        CodeSymbolKind::Function,
        CodeRange::new(1, 10, 1, 1).expect("range"),
        extraction,
    )
    .expect("symbol should validate");
    let file = CodeFileRecord::new(CodeFileFields {
        source_scope: scope,
        path: "src/main.rs".to_owned(),
        content_hash: "hash".to_owned(),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Parsed,
        diagnostic: None,
        symbols: vec![symbol],
        references: Vec::new(),
        chunks: Vec::new(),
    })
    .expect("file should validate");
    store
        .commit_code_graph_batch(CodeGraphBatch::new(vec![file]).expect("batch"))
        .await
        .expect("code graph should commit");

    let snapshot = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Code,
            source_scope: Some("repo".to_owned()),
            query: Some("main".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 4,
        })
        .await
        .expect("canvas should load");

    assert!(snapshot.nodes.iter().any(|node| node.kind == "code_symbol"));
    assert!(snapshot.truncated);
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
async fn mixed_canvas_links_evidence_to_code_and_reference_targets() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-source-file",
        scope.clone(),
        "src/lib.rs documents relay graph canvas source file links",
        vec!["Graph Canvas".to_owned()],
    )
    .expect("evidence should validate")
    .with_metadata(
        Some("src/lib.rs".to_owned()),
        None,
        crate::domain::ConfidenceScore::CERTAIN,
        FactStatus::Accepted,
    )
    .expect("evidence metadata should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
        .await
        .expect("evidence commit should succeed");

    let extraction = CodeExtractionMetadata::new("rust", "symbols", "1", "function_item", "name")
        .expect("extraction should validate");
    let symbol = CodeSymbolRecord::new(
        "sym-canvas",
        scope.clone(),
        "src/lib.rs",
        "render_canvas",
        CodeSymbolKind::Function,
        CodeRange::new(4, 42, 2, 6).expect("range"),
        extraction.clone(),
    )
    .expect("symbol should validate");
    let resolved = CodeReferenceRecord::new(CodeReferenceFields {
        reference_id: "ref-resolved".to_owned(),
        source_scope: scope.clone(),
        path: "src/lib.rs".to_owned(),
        symbol_text: "render_canvas".to_owned(),
        kind: CodeReferenceKind::Call,
        range: CodeRange::new(50, 63, 8, 8).expect("range"),
        resolution_state: CodeResolutionState::Resolved,
        target_symbol_id: Some("sym-canvas".to_owned()),
        extraction: extraction.clone(),
    })
    .expect("resolved reference should validate");
    let unresolved = CodeReferenceRecord::new(CodeReferenceFields {
        reference_id: "ref-unresolved".to_owned(),
        source_scope: scope.clone(),
        path: "src/lib.rs".to_owned(),
        symbol_text: "missing_symbol".to_owned(),
        kind: CodeReferenceKind::Import,
        range: CodeRange::new(70, 84, 11, 11).expect("range"),
        resolution_state: CodeResolutionState::Unresolved,
        target_symbol_id: None,
        extraction: extraction.clone(),
    })
    .expect("unresolved reference should validate");
    let file = CodeFileRecord::new(CodeFileFields {
        source_scope: scope,
        path: "src/lib.rs".to_owned(),
        content_hash: "hash-canvas".to_owned(),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Partial,
        diagnostic: Some("macro expansion skipped".to_owned()),
        symbols: vec![symbol],
        references: vec![resolved, unresolved],
        chunks: Vec::new(),
    })
    .expect("file should validate");
    store
        .commit_code_graph_batch(CodeGraphBatch::new(vec![file]).expect("batch"))
        .await
        .expect("code graph should commit");

    let snapshot = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Mixed,
            source_scope: Some("repo".to_owned()),
            query: None,
            graph_version: GraphVersion::new(2),
            limit: 80,
        })
        .await
        .expect("canvas should load");

    let file = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "code-file:repo:src/lib.rs")
        .expect("code file node should be projected");
    assert_eq!(file.status.as_deref(), Some("partial"));
    assert_eq!(
        file.details.get("diagnostic").map(String::as_str),
        Some("macro expansion skipped")
    );
    assert!(snapshot.edges.iter().any(|edge| edge.id
        == "evidence-source-file:ev-source-file:repo:src/lib.rs"
        && edge.kind == "source_path"));
    assert!(
        snapshot
            .edges
            .iter()
            .any(|edge| edge.id == "reference:repo:src/lib.rs:ref-resolved" && edge.kind == "call")
    );
    let unresolved = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "symbol-ref:repo:missing_symbol")
        .expect("unresolved symbol node should be projected");
    assert_eq!(unresolved.status.as_deref(), Some("unresolved"));
    assert!(
        snapshot
            .edges
            .iter()
            .any(|edge| edge.id == "reference:repo:src/lib.rs:ref-unresolved"
                && edge.target == "symbol-ref:repo:missing_symbol")
    );
}

#[tokio::test]
async fn mixed_canvas_excludes_future_source_path_links() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-future-file",
        scope.clone(),
        "src/future.rs documents a file indexed later",
        vec!["Future File".to_owned()],
    )
    .expect("evidence should validate")
    .with_metadata(
        Some("src/future.rs".to_owned()),
        None,
        crate::domain::ConfidenceScore::CERTAIN,
        FactStatus::Accepted,
    )
    .expect("evidence metadata should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
        .await
        .expect("evidence commit should succeed");

    let file = CodeFileRecord::new(CodeFileFields {
        source_scope: scope,
        path: "src/future.rs".to_owned(),
        content_hash: "hash-future".to_owned(),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Parsed,
        diagnostic: None,
        symbols: Vec::new(),
        references: Vec::new(),
        chunks: Vec::new(),
    })
    .expect("file should validate");
    store
        .commit_code_graph_batch(CodeGraphBatch::new(vec![file]).expect("batch"))
        .await
        .expect("code graph should commit");

    let before_file = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Mixed,
            source_scope: Some("repo".to_owned()),
            query: None,
            graph_version: GraphVersion::new(1),
            limit: 40,
        })
        .await
        .expect("canvas should load");

    assert!(
        before_file
            .edges
            .iter()
            .all(|edge| edge.id != "evidence-source-file:ev-future-file:repo:src/future.rs")
    );
    assert!(
        !before_file
            .available_kinds
            .iter()
            .any(|kind| kind == "source_path")
    );
}

#[tokio::test]
async fn code_canvas_prefers_same_path_reference_target_when_symbol_ids_repeat() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let extraction = CodeExtractionMetadata::new("rust", "symbols", "1", "function_item", "name")
        .expect("extraction should validate");
    let same_path_symbol = CodeSymbolRecord::new(
        "shared-symbol",
        scope.clone(),
        "src/a.rs",
        "render_a",
        CodeSymbolKind::Function,
        CodeRange::new(1, 12, 1, 1).expect("range"),
        extraction.clone(),
    )
    .expect("symbol should validate");
    let other_path_symbol = CodeSymbolRecord::new(
        "shared-symbol",
        scope.clone(),
        "src/b.rs",
        "render_b",
        CodeSymbolKind::Function,
        CodeRange::new(1, 12, 1, 1).expect("range"),
        extraction.clone(),
    )
    .expect("symbol should validate");
    let reference = CodeReferenceRecord::new(CodeReferenceFields {
        reference_id: "ref-shared".to_owned(),
        source_scope: scope.clone(),
        path: "src/a.rs".to_owned(),
        symbol_text: "render_a".to_owned(),
        kind: CodeReferenceKind::Call,
        range: CodeRange::new(20, 28, 3, 3).expect("range"),
        resolution_state: CodeResolutionState::Resolved,
        target_symbol_id: Some("shared-symbol".to_owned()),
        extraction: extraction.clone(),
    })
    .expect("reference should validate");
    let first = CodeFileRecord::new(CodeFileFields {
        source_scope: scope.clone(),
        path: "src/a.rs".to_owned(),
        content_hash: "hash-a".to_owned(),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Parsed,
        diagnostic: None,
        symbols: vec![same_path_symbol],
        references: vec![reference],
        chunks: Vec::new(),
    })
    .expect("file should validate");
    let second = CodeFileRecord::new(CodeFileFields {
        source_scope: scope,
        path: "src/b.rs".to_owned(),
        content_hash: "hash-b".to_owned(),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Parsed,
        diagnostic: None,
        symbols: vec![other_path_symbol],
        references: Vec::new(),
        chunks: Vec::new(),
    })
    .expect("file should validate");
    store
        .commit_code_graph_batch(CodeGraphBatch::new(vec![first, second]).expect("batch"))
        .await
        .expect("code graph should commit");

    let snapshot = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Code,
            source_scope: Some("repo".to_owned()),
            query: None,
            graph_version: GraphVersion::new(1),
            limit: 80,
        })
        .await
        .expect("canvas should load");
    let reference_edges = snapshot
        .edges
        .iter()
        .filter(|edge| edge.id == "reference:repo:src/a.rs:ref-shared")
        .collect::<Vec<_>>();

    assert_eq!(reference_edges.len(), 1);
    assert_eq!(
        reference_edges[0].target,
        "code-symbol:repo:src/a.rs:shared-symbol"
    );
    assert_ne!(
        reference_edges[0].target,
        "code-symbol:repo:src/b.rs:shared-symbol"
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

#[tokio::test]
async fn canvas_rejects_limits_outside_storage_budget() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");

    let zero = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Knowledge,
            source_scope: None,
            query: None,
            graph_version: GraphVersion::ZERO,
            limit: 0,
        })
        .await
        .expect_err("zero limit should fail");
    assert!(zero.to_string().contains("limit must be positive"));

    let oversized = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Knowledge,
            source_scope: None,
            query: None,
            graph_version: GraphVersion::ZERO,
            limit: 1001,
        })
        .await
        .expect_err("oversized limit should fail");
    assert!(oversized.to_string().contains("limit must be at most 1000"));
}
