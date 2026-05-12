use super::*;
use crate::{
    domain::{
        CodeCallRecord, CodeImportRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositorySelector, CodeRetrievalLayer, FreshnessPolicy, RepositoryCodeChunkRecord,
        RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeReferenceRecord,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn stores_code_repository_and_queries_fallback_chunks() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    let snapshot = snapshot_with_chunk("repo", "src/lib.rs", "fn retry_policy() {}");
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");
    let selector =
        CodeRepositorySelector::new("fixture", "commit", vec!["src/".to_owned()], Vec::new())
            .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "retry_policy",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/lib.rs");
    assert_eq!(hits[0].resolved_commit_sha, "commit");
    assert!(
        !hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
}

#[tokio::test]
async fn text_only_chunk_hits_are_marked_as_text_fallback() {
    let store = store_with_repository_snapshot(snapshot_with_chunk_status(
        "repo",
        "README.txt",
        "RetryPolicy appears in docs",
        CodeParseStatus::TextOnly,
        Some("tree-sitter grammar is not configured".to_owned()),
    ))
    .await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "RetryPolicy",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("query should succeed");

    assert_eq!(hits.len(), 1);
    assert!(
        hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
}

#[tokio::test]
async fn rejects_code_queries_for_unindexed_refs() {
    let store = store_with_repository_snapshot(snapshot_with_chunk(
        "repo",
        "src/lib.rs",
        "fn retry_policy() {}",
    ))
    .await;
    let selector = CodeRepositorySelector::new("fixture", "other", Vec::new(), Vec::new())
        .expect("selector should validate");

    let error = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "retry_policy",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect_err("stale ref should fail");

    assert!(error.to_string().contains("not requested ref other"));
}

#[tokio::test]
async fn language_filters_apply_to_references_calls_and_imports() {
    let store = store_with_repository_snapshot(snapshot_with_language_edges()).await;
    let selector = CodeRepositorySelector::new(
        "fixture",
        "commit",
        vec!["src/".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("selector should validate");

    for kind in [
        CodeQueryKind::References,
        CodeQueryKind::Callers,
        CodeQueryKind::Imports,
    ] {
        let query = if kind == CodeQueryKind::Imports {
            "module"
        } else {
            "target"
        };
        let hits = store
            .search_code(
                crate::domain::CodeRetrievalRequest::new(
                    query,
                    selector.clone(),
                    kind,
                    10,
                    FreshnessPolicy::AllowStale,
                )
                .expect("request should validate"),
            )
            .await
            .expect("query should succeed");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].language_id, "rust");
        assert_eq!(hits[0].path, "src/lib.rs");
    }
}

#[tokio::test]
async fn impact_does_not_fall_back_to_all_symbols_for_non_symbol_paths() {
    let store = store_with_repository_snapshot(snapshot_with_language_edges()).await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(request, vec!["README.md".to_owned()])
        .await
        .expect("impact should succeed");

    assert!(hits.is_empty());
}

#[tokio::test]
async fn impact_callers_match_resolved_symbol_identity() {
    let store = store_with_repository_snapshot(snapshot_with_duplicate_callee_names()).await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(request, vec!["src/a.rs".to_owned()])
        .await
        .expect("impact should succeed");

    assert!(hits.iter().any(|hit| hit.path == "src/caller_a.rs"));
    assert!(!hits.iter().any(|hit| hit.path == "src/caller_b.rs"));
}

#[tokio::test]
async fn impact_seeds_respect_request_path_filters() {
    let store = store_with_repository_snapshot(snapshot_with_out_of_scope_seed()).await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", vec!["src".to_owned()], Vec::new())
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(request, vec!["tests/out.rs".to_owned()])
        .await
        .expect("impact should succeed");

    assert!(hits.is_empty());
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

fn snapshot_with_chunk(repository_id: &str, path: &str, content: &str) -> CodeIndexSnapshot {
    snapshot_with_chunk_status(repository_id, path, content, CodeParseStatus::Parsed, None)
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
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file("file", path, "rust", parse_status, degraded_reason)],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: vec![chunk("chunk", "file", path, content, None)],
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_language_edges() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "rust-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "python-file",
                "py/app.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: Vec::new(),
        references: vec![
            reference("rust-reference", "rust-file", "src/lib.rs", None),
            reference("python-reference", "python-file", "py/app.py", None),
        ],
        imports: vec![
            import("rust-import", "rust-file", "src/lib.rs"),
            import("python-import", "python-file", "py/app.py"),
        ],
        calls: vec![
            call("rust-call", "rust-file", "src/lib.rs", None),
            call("python-call", "python-file", "py/app.py", None),
        ],
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_duplicate_callee_names() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        full_replace: true,
        changed_path_count: 4,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("a-file", "src/a.rs", "rust", CodeParseStatus::Parsed, None),
            file("b-file", "src/b.rs", "rust", CodeParseStatus::Parsed, None),
            file(
                "caller-a-file",
                "src/caller_a.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "caller-b-file",
                "src/caller_b.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![
            symbol("target-a", "a-file", "src/a.rs", "target"),
            symbol("target-b", "b-file", "src/b.rs", "target"),
        ],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![
            call(
                "call-a",
                "caller-a-file",
                "src/caller_a.rs",
                Some("target-a"),
            ),
            call(
                "call-b",
                "caller-b-file",
                "src/caller_b.rs",
                Some("target-b"),
            ),
        ],
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_out_of_scope_seed() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "out-file",
                "tests/out.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "caller-file",
                "src/caller.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![symbol("out-target", "out-file", "tests/out.rs", "target")],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![call(
            "out-call",
            "caller-file",
            "src/caller.rs",
            Some("out-target"),
        )],
        chunks: Vec::new(),
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

fn symbol(id: &str, file_id: &str, path: &str, name: &str) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        symbol_snapshot_id: id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: name.to_owned(),
        qualified_name: format!("{}::{name}", path.replace('/', "::")),
        kind: "function".to_owned(),
        signature: format!("fn {name}()"),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 0, end: 8 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn reference(
    id: &str,
    file_id: &str,
    path: &str,
    target_symbol_snapshot_id: Option<&str>,
) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        reference_id: id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        name: "target".to_owned(),
        kind: "call".to_owned(),
        target_symbol_snapshot_id: target_symbol_snapshot_id.map(str::to_owned),
        byte_range: RepositoryCodeRange { start: 0, end: 6 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn import(id: &str, file_id: &str, path: &str) -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        import_id: id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        module: "module::target".to_owned(),
        line_range: RepositoryCodeRange { start: 1, end: 1 },
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

fn call(
    id: &str,
    file_id: &str,
    path: &str,
    callee_symbol_snapshot_id: Option<&str>,
) -> CodeCallRecord {
    CodeCallRecord {
        repository_id: "repo".to_owned(),
        call_id: id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        caller_symbol_snapshot_id: None,
        caller_name: Some("caller".to_owned()),
        callee_symbol_snapshot_id: callee_symbol_snapshot_id.map(str::to_owned),
        callee_name: "target".to_owned(),
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}
