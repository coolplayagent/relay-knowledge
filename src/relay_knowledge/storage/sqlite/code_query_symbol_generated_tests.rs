use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration,
        CodeRepositorySelector, FreshnessPolicy, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:symbol-generated:commit:tree";

#[tokio::test]
async fn exact_symbol_queries_filter_generated_direct_rows_before_candidate_limit() {
    let store = store_with_generated_symbol_fixture().await;
    delete_symbol_search_row(&store, "src/zz_handwritten.rs").await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let mut request = crate::domain::CodeRetrievalRequest::new(
        "Recover",
        selector,
        CodeQueryKind::Definition,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    request.exclude_generated = true;

    let hits = store
        .search_code(request)
        .await
        .expect("direct exact symbol query should find handwritten rows");

    assert!(hits.iter().any(|hit| hit.path == "src/zz_handwritten.rs"));
    assert!(!hits.iter().any(|hit| hit.path.starts_with("generated/")));
}

#[tokio::test]
async fn exact_symbol_queries_prefer_handwritten_direct_rows_before_candidate_limit() {
    let store = store_with_generated_symbol_fixture().await;
    delete_symbol_search_row(&store, "src/zz_handwritten.rs").await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "Recover",
        selector,
        CodeQueryKind::Definition,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("direct exact symbol query should keep handwritten rows");

    assert_eq!(
        hits.first().map(|hit| hit.path.as_str()),
        Some("src/zz_handwritten.rs")
    );
}

#[tokio::test]
async fn symbol_fts_queries_prefer_handwritten_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut symbols = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/recover_{index:03}.rs");
        let mut generated_file = file(&file_id, &path);
        generated_file.is_generated = true;
        files.push(generated_file);
        symbols.push(symbol_with_signature(
            &format!("generated-recover-{index:03}"),
            &file_id,
            &path,
            "fn recover_alpha_beta_gamma(alpha: Beta) -> Gamma",
        ));
    }
    files.push(file("handwritten-file", "src/zz_handwritten.rs"));
    symbols.push(symbol_with_signature(
        "handwritten-recover",
        "handwritten-file",
        "src/zz_handwritten.rs",
        "fn recover_alpha_beta_gamma(alpha: Beta) -> Gamma",
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
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
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
        "recover alpha beta gamma",
        selector,
        CodeQueryKind::Definition,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("symbol FTS query should keep handwritten rows");

    assert_eq!(
        hits.first().map(|hit| hit.path.as_str()),
        Some("src/zz_handwritten.rs")
    );
}

async fn store_with_generated_symbol_fixture() -> SqliteGraphStore {
    let mut files = Vec::new();
    let mut symbols = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/recover_{index:03}.rs");
        let mut generated_file = file(&file_id, &path);
        generated_file.is_generated = true;
        files.push(generated_file);
        symbols.push(symbol(
            &format!("generated-recover-{index:03}"),
            &file_id,
            &path,
        ));
    }
    files.push(file("handwritten-file", "src/zz_handwritten.rs"));
    symbols.push(symbol(
        "handwritten-recover",
        "handwritten-file",
        "src/zz_handwritten.rs",
    ));
    store_with_snapshot(CodeIndexSnapshot {
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
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await
}

async fn delete_symbol_search_row(store: &SqliteGraphStore, path: &str) {
    store
        .run({
            let path = path.to_owned();
            move |connection| {
                connection.execute(
                    "
                    DELETE FROM code_repository_search
                    WHERE source_scope = ?1
                      AND document_kind = 'symbol'
                      AND path = ?2
                    ",
                    (&TEST_SOURCE_SCOPE, &path),
                )?;
                Ok(())
            }
        })
        .await
        .expect("test should remove handwritten symbol FTS row");
}

fn file(file_id: &str, path: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    }
}

fn symbol(symbol_snapshot_id: &str, file_id: &str, path: &str) -> RepositoryCodeSymbolRecord {
    symbol_with_signature(symbol_snapshot_id, file_id, path, "fn Recover()")
}

fn symbol_with_signature(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    signature: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::Recover", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: "Recover".to_owned(),
        qualified_name: "Recover".to_owned(),
        kind: "function".to_owned(),
        signature: signature.to_owned(),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 1, end: 1 },
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
