use crate::{
    domain::{
        CodeFileFields, CodeFileRecord, CodeGraphBatch, CodeParseStatus, CodeSymbolKind,
        CodeSymbolRecord, GraphVersion, SourceScope,
    },
    storage::{CodeGraphStore, CodeSymbolSearchRequest},
};

use crate::storage::sqlite::code_graph::tests::support::{extraction, parsed_file, range};

#[tokio::test]
async fn symbol_queries_are_version_bounded() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .commit_code_graph_batch(
            CodeGraphBatch::new(vec![parsed_file("repo-a", "src/lib.rs", "sym-a")])
                .expect("batch should validate"),
        )
        .await
        .expect("first commit should succeed");
    store
        .commit_code_graph_batch(
            CodeGraphBatch::new(vec![parsed_file("repo-b", "src/lib.rs", "sym-b")])
                .expect("batch should validate"),
        )
        .await
        .expect("second commit should succeed");

    let first_snapshot = store
        .search_code_symbols(CodeSymbolSearchRequest {
            source_scope: None,
            path: None,
            name: Some("main".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 10,
        })
        .await
        .expect("symbol search should succeed");

    assert_eq!(first_snapshot.len(), 1);
    assert_eq!(first_snapshot[0].source_scope.as_str(), "repo-a");
}

#[tokio::test]
async fn symbol_search_returns_enum_members() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let source_scope = SourceScope::parse("repo").expect("scope should parse");
    let symbol = CodeSymbolRecord::new(
        "sym-color-red",
        source_scope.clone(),
        "src/lib.rs",
        "Color.Red",
        CodeSymbolKind::EnumMember,
        range(13, 16),
        extraction(),
    )
    .expect("enum member should validate");
    let file = CodeFileRecord::new(CodeFileFields {
        source_scope,
        path: "src/lib.rs".to_owned(),
        content_hash: "hash-enum-member".to_owned(),
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
        .expect("commit should succeed");
    let symbols = store
        .search_code_symbols(CodeSymbolSearchRequest {
            source_scope: Some("repo".to_owned()),
            path: None,
            name: Some("Color.Red".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
        })
        .await
        .expect("enum member symbol search should succeed");

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].kind, CodeSymbolKind::EnumMember);
    assert_eq!(symbols[0].name, "Color.Red");
}
