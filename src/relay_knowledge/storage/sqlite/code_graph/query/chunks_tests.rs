use crate::{
    domain::{CodeGraphBatch, GraphVersion},
    storage::{CodeChunkSearchRequest, CodeGraphStore},
};

use crate::storage::sqlite::code_graph::tests::support::parsed_file;

#[tokio::test]
async fn chunk_queries_are_scope_and_version_bounded() {
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

    let scoped = store
        .search_code_chunks(CodeChunkSearchRequest {
            source_scope: Some("repo-b".to_owned()),
            path: Some("src/lib.rs".to_owned()),
            query: Some("main".to_owned()),
            graph_version: GraphVersion::new(2),
            limit: 10,
        })
        .await
        .expect("chunk search should succeed");
    let first_snapshot = store
        .search_code_chunks(CodeChunkSearchRequest {
            source_scope: None,
            path: None,
            query: Some("main".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 10,
        })
        .await
        .expect("version-bounded chunk search should succeed");

    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].source_scope.as_str(), "repo-b");
    assert_eq!(scoped[0].linked_symbol_ids, ["sym-b"]);
    assert_eq!(first_snapshot.len(), 1);
    assert_eq!(first_snapshot[0].source_scope.as_str(), "repo-a");
}
