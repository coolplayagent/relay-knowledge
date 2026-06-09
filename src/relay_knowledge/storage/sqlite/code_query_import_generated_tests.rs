use crate::{
    domain::{
        CodeImportRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeFileRecord, RepositoryCodeRange,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:import-generated:commit:tree";

#[tokio::test]
async fn import_path_queries_filter_generated_direct_rows_before_early_return() {
    let mut generated_file = file("generated-file", "generated/importer.rs", "rust");
    generated_file.is_generated = true;
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
            generated_file,
            file("handwritten-file", "src/foo/bar.rs", "rust"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![
            import(
                "generated-import",
                "generated-file",
                "generated/importer.rs",
                "src/foo/bar.rs",
                None,
            ),
            import(
                "handwritten-import",
                "handwritten-file",
                "src/foo/bar.rs",
                "crate::runtime",
                None,
            ),
        ],
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let mut request = crate::domain::CodeRetrievalRequest::new(
        "src/foo/bar.rs",
        selector,
        CodeQueryKind::Imports,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    request.exclude_generated = true;

    let hits = store
        .search_code(request)
        .await
        .expect("import path query should fall through to handwritten FTS hits");

    assert!(hits.iter().any(|hit| hit.path == "src/foo/bar.rs"));
    assert!(!hits.iter().any(|hit| hit.path == "generated/importer.rs"));
}

#[tokio::test]
async fn import_queries_prefer_handwritten_fts_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut imports = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/importer_{index:03}.rs");
        let mut generated_file = file(&file_id, &path, "rust");
        generated_file.is_generated = true;
        files.push(generated_file);
        imports.push(import(
            &format!("generated-import-{index:03}"),
            &file_id,
            &path,
            "SharedPackage",
            None,
        ));
    }
    files.push(file("handwritten-file", "src/zz_handwritten.rs", "rust"));
    imports.push(import(
        "handwritten-import",
        "handwritten-file",
        "src/zz_handwritten.rs",
        "SharedPackage",
        None,
    ));
    let store = store_with_snapshot(CodeIndexSnapshot {
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
        symbols: Vec::new(),
        references: Vec::new(),
        imports,
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "SharedPackage",
        selector,
        CodeQueryKind::Imports,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("import query should keep handwritten FTS rows");

    assert_eq!(
        hits.first().map(|hit| hit.path.as_str()),
        Some("src/zz_handwritten.rs")
    );
}

#[tokio::test]
async fn import_path_queries_prefer_handwritten_direct_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut imports = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/importer_{index:03}.rs");
        let mut generated_file = file(&file_id, &path, "rust");
        generated_file.is_generated = true;
        files.push(generated_file);
        imports.push(import(
            &format!("generated-import-{index:03}"),
            &file_id,
            &path,
            "shared/module.rs",
            None,
        ));
    }
    files.push(file("handwritten-file", "src/zz_importer.rs", "rust"));
    imports.push(import(
        "handwritten-import",
        "handwritten-file",
        "src/zz_importer.rs",
        "shared/module.rs",
        None,
    ));
    let store = store_with_snapshot(CodeIndexSnapshot {
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
        symbols: Vec::new(),
        references: Vec::new(),
        imports,
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    delete_search_kind(&store, "import").await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "shared/module.rs",
        selector,
        CodeQueryKind::Imports,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("import direct query should keep handwritten rows");

    assert_eq!(
        hits.first().map(|hit| hit.path.as_str()),
        Some("src/zz_importer.rs")
    );
}

async fn delete_search_kind(store: &SqliteGraphStore, document_kind: &str) {
    store
        .run({
            let document_kind = document_kind.to_owned();
            move |connection| {
                connection.execute(
                    "
                    DELETE FROM code_repository_search
                    WHERE source_scope = ?1
                      AND document_kind = ?2
                    ",
                    (&TEST_SOURCE_SCOPE, &document_kind),
                )?;
                Ok(())
            }
        })
        .await
        .expect("test should remove FTS rows");
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
        is_generated: false,
        degraded_reason: None,
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
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 5_000,
        confidence_tier: "inferred".to_owned(),
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
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
