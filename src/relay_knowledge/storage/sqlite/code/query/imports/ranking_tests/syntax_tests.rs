use super::*;

#[tokio::test]
async fn import_syntax_queries_rank_import_expression_rows_before_static_declarations() {
    let provider_path = "src/provider.ts";
    let mut type_import = import(
        "protocol-type-import",
        "provider-file",
        provider_path,
        "import type { StreamEnvelope } from \"./protocol\";",
    );
    type_import.line_range = range(2, 2);
    let mut runtime_import = import(
        "protocol-runtime-import",
        "provider-file",
        provider_path,
        "import { sendEnvelope } from \"./protocol\";",
    );
    runtime_import.line_range = range(3, 3);
    let mut dynamic_import = import(
        "protocol-dynamic-import",
        "provider-file",
        provider_path,
        "await import(\"./protocol\")",
    );
    dynamic_import.line_range = range(8, 8);
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file("provider-file", provider_path, "typescript")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![type_import, runtime_import, dynamic_import],
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let import_syntax_hits = store
        .search_code(request("import \"./protocol\"", CodeQueryKind::Imports))
        .await
        .expect("import syntax query should succeed");
    assert!(import_syntax_hits[0].excerpt.contains("await import"));

    let path_hits = store
        .search_code(request("./protocol", CodeQueryKind::Imports))
        .await
        .expect("plain path import query should succeed");
    assert!(path_hits[0].excerpt.starts_with("import "));
}
