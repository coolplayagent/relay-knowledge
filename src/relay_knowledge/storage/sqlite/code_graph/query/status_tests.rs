use crate::{
    domain::{CodeFileFields, CodeFileRecord, CodeGraphBatch, CodeParseStatus, SourceScope},
    storage::{CodeGraphStore, GraphStore},
};

#[tokio::test]
async fn failed_and_partial_files_are_visible_in_parse_counts() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let failed = code_file(
        "src/broken.rs",
        CodeParseStatus::Failed,
        "parser panic isolated",
    );
    let partial = code_file(
        "src/partial.rs",
        CodeParseStatus::Partial,
        "syntax error node",
    );

    store
        .commit_code_graph_batch(CodeGraphBatch::new(vec![failed, partial]).expect("batch"))
        .await
        .expect("commit should succeed");
    let graph = store.inspect_graph().await.expect("graph should inspect");

    assert_eq!(graph.code_file_count, 2);
    assert_eq!(graph.code_parse_status_counts.failed, 1);
    assert_eq!(graph.code_parse_status_counts.partial, 1);
}

fn code_file(path: &str, parse_status: CodeParseStatus, diagnostic: &str) -> CodeFileRecord {
    CodeFileRecord::new(CodeFileFields {
        source_scope: SourceScope::parse("repo").expect("scope should parse"),
        path: path.to_owned(),
        content_hash: format!("hash-{path}"),
        language_id: "rust".to_owned(),
        parse_status,
        diagnostic: Some(diagnostic.to_owned()),
        symbols: Vec::new(),
        references: Vec::new(),
        chunks: Vec::new(),
    })
    .expect("file should validate")
}
