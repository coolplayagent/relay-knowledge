use super::*;
use crate::{
    domain::{
        CodeExtractionMetadata, CodeFileFields, CodeFileRecord, CodeGraphBatch, CodeParseStatus,
        CodeRange, CodeReferenceFields, CodeReferenceKind, CodeReferenceRecord,
        CodeResolutionState, CodeSymbolKind, CodeSymbolRecord, EvidenceRecord, FactStatus,
        GraphMutationBatch, SourceScope,
    },
    storage::{CodeGraphStore, GraphCanvasSelection, GraphCanvasStorageRequest, GraphStore},
};

#[test]
fn code_file_projection_keeps_diagnostics_and_scope_relationship() {
    let mut builder = CanvasBuilder::new(8);
    insert_code_file_node(
        &mut builder,
        "repo",
        "src/lib.rs",
        Some("rust"),
        Some("partial"),
        Some("macro recovery"),
        GraphVersion::new(2),
    );
    let snapshot = builder.into_snapshot();

    let file = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "code-file:repo:src/lib.rs")
        .expect("code file should be projected");
    assert_eq!(file.status.as_deref(), Some("partial"));
    assert_eq!(
        file.details.get("diagnostic").map(String::as_str),
        Some("macro recovery")
    );
    assert!(
        snapshot
            .edges
            .iter()
            .any(|edge| edge.id == "scope-file:repo:src/lib.rs")
    );
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

    let after_file = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Mixed,
            source_scope: Some("repo".to_owned()),
            query: None,
            graph_version: GraphVersion::new(2),
            limit: 40,
        })
        .await
        .expect("canvas should load");
    let source_edge = after_file
        .edges
        .iter()
        .find(|edge| edge.id == "evidence-source-file:ev-future-file:repo:src/future.rs")
        .expect("source path edge should appear when the file exists");
    assert_eq!(source_edge.graph_version, GraphVersion::new(2));
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
