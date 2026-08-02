//! Direct invariants for transactional code-graph batch replacement.

use crate::{
    domain::{CodeGraphBatch, GraphVersion},
    storage::{CodeGraphStore, CodeSymbolSearchRequest, GraphStore, IndexStore},
};

use crate::storage::sqlite::code_graph::tests::support::parsed_file;

#[tokio::test]
async fn commits_code_graph_batch_and_marks_indexes_stale() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let batch = CodeGraphBatch::new(vec![parsed_file("repo", "src/lib.rs", "sym-main")])
        .expect("batch should validate");

    let receipt = store
        .commit_code_graph_batch(batch)
        .await
        .expect("code graph commit should succeed");
    let graph = store.inspect_graph().await.expect("graph should inspect");
    let indexes = store.index_statuses().await.expect("indexes should load");

    assert_eq!(receipt.graph_version, GraphVersion::new(1));
    assert_eq!(receipt.file_count, 1);
    assert_eq!(receipt.symbol_count, 1);
    assert_eq!(graph.code_file_count, 1);
    assert_eq!(graph.code_symbol_count, 1);
    assert_eq!(graph.code_reference_count, 1);
    assert_eq!(graph.code_chunk_count, 1);
    assert_eq!(graph.code_parse_status_counts.parsed, 1);
    assert!(
        indexes
            .iter()
            .all(|status| status.is_stale_for(GraphVersion::new(1)))
    );
}

#[tokio::test]
async fn replacing_file_facts_removes_old_symbols() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let first = parsed_file("repo", "src/lib.rs", "sym-old");
    let second = parsed_file("repo", "src/lib.rs", "sym-new");
    store
        .commit_code_graph_batch(CodeGraphBatch::new(vec![first]).expect("batch"))
        .await
        .expect("first commit should succeed");
    store
        .commit_code_graph_batch(CodeGraphBatch::new(vec![second]).expect("batch"))
        .await
        .expect("second commit should succeed");

    let symbols = store
        .search_code_symbols(CodeSymbolSearchRequest {
            source_scope: Some("repo".to_owned()),
            path: Some("src/lib.rs".to_owned()),
            name: None,
            graph_version: GraphVersion::new(2),
            limit: 10,
        })
        .await
        .expect("symbol search should succeed");

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].symbol_id, "sym-new");
}
