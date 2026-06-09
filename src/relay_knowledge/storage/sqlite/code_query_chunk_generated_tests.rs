use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration,
        CodeRepositorySelector, FreshnessPolicy, RepositoryCodeChunkRecord,
        RepositoryCodeFileRecord, RepositoryCodeRange,
    },
    storage::{SqliteGraphStore, code::CodeRepositoryStore},
};

const TEST_SOURCE_SCOPE: &str = "code:test:chunk-generated:commit:tree";

#[tokio::test]
async fn chunk_queries_prefer_handwritten_fts_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut chunks = Vec::new();
    for index in 0..320 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/chunk_{index:03}.rs");
        let mut generated_file = file(&file_id, &path);
        generated_file.is_generated = true;
        files.push(generated_file);
        chunks.push(chunk(
            &format!("generated-chunk-{index:03}"),
            &file_id,
            &path,
        ));
    }
    files.push(file("handwritten-file", "src/zz_handwritten.rs"));
    chunks.push(chunk(
        "handwritten-chunk",
        "handwritten-file",
        "src/zz_handwritten.rs",
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
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks,
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "render invoice ledger",
        selector,
        CodeQueryKind::Definition,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("chunk query should keep handwritten FTS rows");

    assert_eq!(
        hits.first().map(|hit| hit.path.as_str()),
        Some("src/zz_handwritten.rs")
    );
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

fn chunk(chunk_id: &str, file_id: &str, path: &str) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        content: "render invoice ledger".to_owned(),
        byte_range: RepositoryCodeRange { start: 1, end: 1 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: None,
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
