use crate::{
    domain::{CodeGraphBatch, GraphVersion},
    storage::{CodeGraphStore, CodeReferenceSearchRequest},
};

use crate::storage::sqlite::code_graph::tests::support::parsed_file;

#[tokio::test]
async fn reference_search_can_filter_by_target_symbol() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .commit_code_graph_batch(
            CodeGraphBatch::new(vec![parsed_file("repo", "src/lib.rs", "sym-main")])
                .expect("batch should validate"),
        )
        .await
        .expect("commit should succeed");

    let references = store
        .search_code_references(CodeReferenceSearchRequest {
            source_scope: Some("repo".to_owned()),
            path: None,
            symbol_text: Some("main".to_owned()),
            target_symbol_id: Some("sym-main".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
        })
        .await
        .expect("reference search should succeed");

    assert_eq!(references.len(), 1);
    assert_eq!(references[0].target_symbol_id.as_deref(), Some("sym-main"));
}
