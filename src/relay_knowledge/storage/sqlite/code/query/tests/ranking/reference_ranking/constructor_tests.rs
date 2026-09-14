use super::*;

#[tokio::test]
async fn exact_reference_queries_rank_constructor_calls_before_passive_values() {
    let registry_path = "src/connector/registry.ts";
    let runtime_path = "src/connector/runtime.ts";
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("registry-file", registry_path, "typescript"),
            file("runtime-file", runtime_path, "typescript"),
        ],
        symbols: Vec::new(),
        references: vec![
            reference_on_line(
                "registry-value-reference",
                "registry-file",
                registry_path,
                "SaveRequest",
                None,
                8,
            ),
            reference_on_line(
                "runtime-constructor-reference",
                "runtime-file",
                runtime_path,
                "SaveRequest",
                None,
                20,
            ),
        ],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            chunk(
                "registry-chunk",
                "registry-file",
                registry_path,
                "export const handlers = {\n  save: SaveRequest,\n};",
                range(7, 9),
            ),
            chunk(
                "runtime-chunk",
                "runtime-file",
                runtime_path,
                "export function run(input: unknown) {\n  return new SaveRequest(input);\n}",
                range(19, 21),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("SaveRequest", CodeQueryKind::References))
        .await
        .expect("reference query should succeed");

    assert_eq!(hits[0].path, runtime_path);
    assert!(hits[0].excerpt.contains("new SaveRequest"));
    assert!(score_for_path(&hits, runtime_path) > score_for_path(&hits, registry_path));
}
