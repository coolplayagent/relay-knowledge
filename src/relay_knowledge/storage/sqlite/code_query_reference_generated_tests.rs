use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration,
        CodeRepositorySelector, FreshnessPolicy, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeReferenceRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:reference-generated:commit:tree";

#[tokio::test]
async fn references_filter_generated_direct_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut references = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/ref_{index:03}.rs");
        let mut generated_file = file(&file_id, &path, "rust");
        generated_file.is_generated = true;
        files.push(generated_file);
        references.push(reference(
            &format!("generated-reference-{index:03}"),
            &file_id,
            &path,
        ));
    }
    files.push(file("handwritten-file", "src/zz_reference.rs", "rust"));
    references.push(reference(
        "handwritten-reference",
        "handwritten-file",
        "src/zz_reference.rs",
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
        references,
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    delete_search_row(&store, "reference", "src/zz_reference.rs").await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let mut request = crate::domain::CodeRetrievalRequest::new(
        "SharedThing",
        selector,
        CodeQueryKind::References,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    request.exclude_generated = true;

    let hits = store
        .search_code(request)
        .await
        .expect("reference direct query should find handwritten rows");

    assert!(hits.iter().any(|hit| hit.path == "src/zz_reference.rs"));
    assert!(!hits.iter().any(|hit| hit.path.starts_with("generated/")));
}

#[tokio::test]
async fn references_prefer_handwritten_fts_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut references = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/ref_{index:03}.rs");
        let mut generated_file = file(&file_id, &path, "rust");
        generated_file.is_generated = true;
        files.push(generated_file);
        references.push(reference(
            &format!("generated-reference-{index:03}"),
            &file_id,
            &path,
        ));
    }
    files.push(file("handwritten-file", "src/zz_reference.rs", "rust"));
    references.push(reference(
        "handwritten-reference",
        "handwritten-file",
        "src/zz_reference.rs",
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
        references,
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "SharedThing",
        selector,
        CodeQueryKind::References,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("reference FTS query should keep handwritten rows");

    assert_eq!(
        hits.first().map(|hit| hit.path.as_str()),
        Some("src/zz_reference.rs")
    );
}

#[tokio::test]
async fn references_prefer_handwritten_direct_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut references = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/ref_{index:03}.rs");
        let mut generated_file = file(&file_id, &path, "rust");
        generated_file.is_generated = true;
        files.push(generated_file);
        references.push(reference(
            &format!("generated-reference-{index:03}"),
            &file_id,
            &path,
        ));
    }
    files.push(file("handwritten-file", "src/zz_reference.rs", "rust"));
    references.push(reference(
        "handwritten-reference",
        "handwritten-file",
        "src/zz_reference.rs",
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
        references,
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    delete_search_kind(&store, "reference").await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "SharedThing",
        selector,
        CodeQueryKind::References,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("reference direct query should keep handwritten rows");

    assert_eq!(
        hits.first().map(|hit| hit.path.as_str()),
        Some("src/zz_reference.rs")
    );
}

async fn delete_search_row(store: &SqliteGraphStore, document_kind: &str, path: &str) {
    store
        .run({
            let document_kind = document_kind.to_owned();
            let path = path.to_owned();
            move |connection| {
                connection.execute(
                    "
                    DELETE FROM code_repository_search
                    WHERE source_scope = ?1
                      AND document_kind = ?2
                      AND path = ?3
                    ",
                    (&TEST_SOURCE_SCOPE, &document_kind, &path),
                )?;
                Ok(())
            }
        })
        .await
        .expect("test should remove FTS row");
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

fn reference(reference_id: &str, file_id: &str, path: &str) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        reference_id: reference_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        name: "SharedThing".to_owned(),
        kind: "call".to_owned(),
        target_symbol_snapshot_id: None,
        target_hint: Some("SharedThing".to_owned()),
        resolution_state: "resolved".to_owned(),
        confidence_basis_points: 8_000,
        confidence_tier: "inferred".to_owned(),
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
