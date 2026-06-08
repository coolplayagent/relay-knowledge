use super::*;
use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration,
        CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy, RepositoryCodeChunkRecord,
        RepositoryCodeFileRecord, RepositoryCodeRange,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

const TEST_SOURCE_SCOPE: &str = "git_snapshot:test";

#[tokio::test]
async fn candidate_paths_for_scope_apply_filters_before_limit() {
    let mut snapshot =
        snapshot_with_chunk_status("repo", "src/lib.rs", "body", CodeParseStatus::Parsed, None);
    snapshot.files.push(file(
        "doc",
        "docs/operations.md",
        "unknown",
        CodeParseStatus::TextOnly,
        None,
    ));
    snapshot.files.push(file(
        "notes",
        "docs/notes.txt",
        "unknown",
        CodeParseStatus::TextOnly,
        None,
    ));
    let store = store_with_repository_snapshot(snapshot).await;

    let paths = store
        .code_file_candidate_paths_for_scope(
            TEST_SOURCE_SCOPE.to_owned(),
            vec!["docs".to_owned()],
            vec!["unknown".to_owned()],
            1,
        )
        .await
        .expect("candidate paths should load");

    assert_eq!(paths, ["docs/notes.txt"]);
}

#[tokio::test]
async fn candidate_paths_for_query_scope_use_search_before_scope_budget() {
    let mut snapshot = snapshot_with_chunk_status(
        "repo",
        "zzz/target.rs",
        "fn late_budget_target() { /* RK_LATE_BUDGET_NOTE */ }",
        CodeParseStatus::Parsed,
        None,
    );
    for index in 0..300 {
        let file_id = format!("noise-{index:03}");
        let path = format!("src/noise_{index:03}.rs");
        snapshot
            .files
            .push(file(&file_id, &path, "rust", CodeParseStatus::Parsed, None));
        snapshot.chunks.push(chunk(
            &format!("noise-chunk-{index:03}"),
            &file_id,
            &path,
            &format!("fn noise_{index:03}() {{}}"),
            None,
        ));
    }
    let store = store_with_repository_snapshot(snapshot).await;

    let paths = store
        .code_file_candidate_paths_for_query_scope(
            TEST_SOURCE_SCOPE.to_owned(),
            "RK_LATE_BUDGET_NOTE".to_owned(),
            Vec::new(),
            vec!["rust".to_owned()],
            1,
        )
        .await
        .expect("candidate paths should load");
    let fallback_paths = store
        .code_file_candidate_paths_for_query_scope(
            TEST_SOURCE_SCOPE.to_owned(),
            "MISSING_BUDGET_NOTE".to_owned(),
            Vec::new(),
            vec!["rust".to_owned()],
            1,
        )
        .await
        .expect("fallback candidate paths should load");

    assert_eq!(paths, ["zzz/target.rs"]);
    assert_eq!(fallback_paths, ["src/noise_000.rs"]);
}

#[tokio::test]
async fn candidate_paths_for_query_scope_deduplicates_before_limit() {
    let mut snapshot = snapshot_with_chunk_status(
        "repo",
        "src/noisy.rs",
        "shared_signal appears in repeated candidate content",
        CodeParseStatus::Parsed,
        None,
    );
    for index in 0..12 {
        snapshot.chunks.push(chunk(
            &format!("noisy-chunk-{index:02}"),
            "file",
            "src/noisy.rs",
            &format!("shared_signal repeated candidate row {index}"),
            None,
        ));
    }
    snapshot.files.push(file(
        "target-file",
        "zzz/target.rs",
        "rust",
        CodeParseStatus::Parsed,
        None,
    ));
    snapshot.chunks.push(chunk(
        "target-chunk",
        "target-file",
        "zzz/target.rs",
        "shared_signal target candidate",
        None,
    ));
    let store = store_with_repository_snapshot(snapshot).await;

    let paths = store
        .code_file_candidate_paths_for_query_scope(
            TEST_SOURCE_SCOPE.to_owned(),
            "shared_signal".to_owned(),
            Vec::new(),
            vec!["rust".to_owned()],
            2,
        )
        .await
        .expect("candidate paths should load");

    assert_eq!(paths.len(), 2);
    assert!(paths.iter().any(|path| path == "src/noisy.rs"));
    assert!(paths.iter().any(|path| path == "zzz/target.rs"));
}

#[tokio::test]
async fn candidate_paths_for_query_scope_falls_back_when_search_table_unavailable() {
    let mut snapshot = snapshot_with_chunk_status(
        "repo",
        "zzz/target.rs",
        "fn late_budget_target() { /* RK_LATE_BUDGET_NOTE */ }",
        CodeParseStatus::Parsed,
        None,
    );
    for index in 0..300 {
        let file_id = format!("noise-{index:03}");
        let path = format!("src/noise_{index:03}.rs");
        snapshot
            .files
            .push(file(&file_id, &path, "rust", CodeParseStatus::Parsed, None));
        snapshot.chunks.push(chunk(
            &format!("noise-chunk-{index:03}"),
            &file_id,
            &path,
            &format!("fn noise_{index:03}() {{}}"),
            None,
        ));
    }
    let store = store_with_repository_snapshot(snapshot).await;
    store
        .run(|connection| {
            connection.execute_batch("DROP TABLE code_repository_search")?;
            Ok(())
        })
        .await
        .expect("search table should be removable");

    let paths = store
        .code_file_candidate_paths_for_query_scope(
            TEST_SOURCE_SCOPE.to_owned(),
            "RK_LATE_BUDGET_NOTE".to_owned(),
            Vec::new(),
            vec!["rust".to_owned()],
            1,
        )
        .await
        .expect("query-aware content fallback should load candidate paths");

    assert_eq!(paths, ["zzz/target.rs"]);
}

#[tokio::test]
async fn candidate_paths_for_query_scope_reports_unavailable_search_without_query_candidates() {
    let snapshot = snapshot_with_chunk_status(
        "repo",
        "zzz/target.rs",
        "fn unrelated_target() {}",
        CodeParseStatus::Parsed,
        None,
    );
    let store = store_with_repository_snapshot(snapshot).await;
    store
        .run(|connection| {
            connection.execute_batch("DROP TABLE code_repository_search")?;
            Ok(())
        })
        .await
        .expect("search table should be removable");

    let error = store
        .code_file_candidate_paths_for_query_scope(
            TEST_SOURCE_SCOPE.to_owned(),
            "RK_LATE_BUDGET_NOTE".to_owned(),
            Vec::new(),
            vec!["rust".to_owned()],
            1,
        )
        .await
        .expect_err("missing query-aware candidates should report unavailable search");
    let message = error.to_string();

    assert!(
        message.contains("code_repository_search"),
        "unexpected error: {message}"
    );
}

#[tokio::test]
async fn code_search_returns_empty_for_plannable_fallback_when_search_read_model_unavailable() {
    let snapshot = snapshot_with_chunk_status(
        "repo",
        "src/lib.rs",
        "fn rk_search_unavailable_note() {}",
        CodeParseStatus::Parsed,
        None,
    );
    let store = store_with_repository_snapshot(snapshot).await;
    store
        .run(|connection| {
            connection.execute_batch("DROP TABLE code_repository_search")?;
            Ok(())
        })
        .await
        .expect("search table should be removable");
    let request = code_search_request("rk_search_unavailable_note", CodeQueryKind::Hybrid);

    let hits = store
        .search_code_scope(TEST_SOURCE_SCOPE.to_owned(), request)
        .await
        .expect("unavailable FTS read model should not fail the code query");

    assert!(
        hits.is_empty(),
        "structured FTS layer should empty out so application fallback can continue: {hits:?}"
    );
}

#[tokio::test]
async fn code_search_reports_import_search_read_model_unavailable() {
    let snapshot = snapshot_with_chunk_status(
        "repo",
        "src/lib.rs",
        "fn rk_search_unavailable_note() {}",
        CodeParseStatus::Parsed,
        None,
    );
    let store = store_with_repository_snapshot(snapshot).await;
    store
        .run(|connection| {
            connection.execute_batch("DROP TABLE code_repository_search")?;
            Ok(())
        })
        .await
        .expect("search table should be removable");
    let request = code_search_request("rk_search_unavailable_note", CodeQueryKind::Imports);

    let error = store
        .search_code_scope(TEST_SOURCE_SCOPE.to_owned(), request)
        .await
        .expect_err("import query should report unavailable FTS read model");
    let message = error.to_string();

    assert!(
        message.contains("code_repository_search"),
        "unexpected error: {message}"
    );
}

#[test]
fn candidate_path_query_keeps_discriminative_suffix_terms() {
    let fts_query =
        code_snapshot::candidate_path_fts_query("a b c d e f g h pkg module VerySpecificHandler")
            .expect("query terms should produce FTS");
    let terms = fts_query.split(" OR ").collect::<Vec<_>>();

    assert_eq!(terms.len(), 8);
    assert!(terms.contains(&"\"VerySpecificHandler\""));
    assert!(!terms.contains(&"\"a\""));
}

async fn store_with_repository_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
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

fn snapshot_with_chunk_status(
    repository_id: &str,
    path: &str,
    content: &str,
    parse_status: CodeParseStatus,
    degraded_reason: Option<String>,
) -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: repository_id.to_owned(),
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
        files: vec![file(
            "file",
            path,
            "rust",
            parse_status,
            degraded_reason.clone(),
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![chunk("chunk", "file", path, content, None)],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn file(
    file_id: &str,
    path: &str,
    language_id: &str,
    parse_status: CodeParseStatus,
    degraded_reason: Option<String>,
) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("{file_id}-hash"),
        byte_len: 20,
        line_count: 1,
        parse_status,
        degraded_reason,
    }
}

fn chunk(
    id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    symbol_snapshot_id: Option<&str>,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        content: content.to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 20 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: symbol_snapshot_id.map(str::to_owned),
    }
}

fn code_search_request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate"),
        kind,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}
