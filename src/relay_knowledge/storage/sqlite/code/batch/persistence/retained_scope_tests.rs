use super::*;

#[tokio::test]
async fn retained_scope_reindex_keeps_intermediate_edge_search_rows() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:retained-edge-languages";
    let session = session_for_scope(source_scope);
    mark_scope_retained(&store, source_scope).await;
    let rust_file = file(source_scope, "rust-file", "src/lib.rs", "rust");
    let rust_reference = reference(
        source_scope,
        "rust-reference",
        "rust-file",
        "src/lib.rs",
        "target",
    );

    store
        .begin_code_index_session(session)
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 1,
            parsed_byte_count: 20,
            files: vec![rust_file],
            symbols: Vec::new(),
            references: vec![rust_reference],
            imports: Vec::new(),
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            routes: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("batch should persist");

    let languages = search_document_languages(&store, source_scope).await;

    assert_eq!(
        languages.get(&("reference".to_owned(), "src/lib.rs".to_owned())),
        Some(&"rust".to_owned())
    );
}
