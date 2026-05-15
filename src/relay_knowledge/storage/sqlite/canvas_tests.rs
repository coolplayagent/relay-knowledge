use super::*;
use crate::{
    domain::{
        CodeExtractionMetadata, CodeFileFields, CodeFileRecord, CodeGraphBatch, CodeParseStatus,
        CodeRange, CodeSymbolKind, CodeSymbolRecord, EvidenceRecord, GraphMutationBatch,
        SourceScope,
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
