use super::*;
use crate::domain::{
    CodeImportRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
    CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy, RepositoryCodeFileRecord,
    RepositoryCodeRange, RepositoryCodeSymbolRecord,
};

const TEST_SOURCE_SCOPE: &str = "code:test:import-target:commit:tree";

#[tokio::test]
async fn target_symbol_import_queries_filter_importing_paths_before_limit() {
    let store = store_with_snapshot(snapshot_with_path_filtered_target_symbol_imports()).await;
    let selector = CodeRepositorySelector::new(
        "fixture",
        "commit",
        vec!["src".to_owned()],
        vec!["go".to_owned()],
    )
    .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "TargetSymbol",
                selector,
                CodeQueryKind::Imports,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("path-filtered target-symbol import query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/target_importer.go");
    assert_eq!(hits[0].edge_target_hint.as_deref(), Some("lib"));
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
}

fn snapshot_with_path_filtered_target_symbol_imports() -> CodeIndexSnapshot {
    let mut files = vec![file("target-symbol-file", "lib/target.go", "go")];
    let mut imports = Vec::new();
    for index in 0..550 {
        let file_id = format!("noise-importer-file-{index:03}");
        let path = format!("src/aaa/noise_importer_{index:03}.py");
        files.push(file(&file_id, &path, "python"));
        imports.push(import(
            &format!("noise-import-{index:03}"),
            &file_id,
            &path,
            "example.com/lib",
            Some("lib"),
        ));
    }
    files.push(file("target-importer-file", "src/target_importer.go", "go"));
    imports.push(import(
        "target-import",
        "target-importer-file",
        "src/target_importer.go",
        "example.com/lib",
        Some("lib"),
    ));

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: files.len(),
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files,
        symbols: vec![symbol(
            "target-symbol",
            "target-symbol-file",
            "lib/target.go",
            "TargetSymbol",
            "go",
        )],
        references: Vec::new(),
        imports,
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn file(file_id: &str, path: &str, language_id: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    }
}

fn symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    language_id: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "type".to_owned(),
        signature: format!("type {name} struct {{}}"),
        doc_comment: None,
        byte_range: range(0, 1),
        line_range: range(1, 1),
    }
}

fn import(
    import_id: &str,
    file_id: &str,
    path: &str,
    module: &str,
    target_hint: Option<&str>,
) -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        import_id: import_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        module: module.to_owned(),
        target_hint: target_hint.map(str::to_owned),
        resolution_state: "resolved".to_owned(),
        confidence_basis_points: 8_000,
        confidence_tier: "inferred".to_owned(),
        line_range: range(1, 1),
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

async fn store_with_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");

    store
}
