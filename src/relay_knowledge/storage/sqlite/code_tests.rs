use super::*;
use crate::{
    domain::{
        CodeFileDiagnostic, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositorySelector, CodeRetrievalLayer, FreshnessPolicy,
    },
    storage::SqliteGraphStore,
};

#[path = "code_test_support.rs"]
mod code_test_support;

use code_test_support::{call, chunk, file, import, import_module, reference, symbol};

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
async fn repository_id_lookup_takes_precedence_over_alias_like_ids() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo:first",
                "first",
                "/tmp/first",
                Vec::new(),
                Vec::new(),
            )
            .expect("first registration should validate"),
        )
        .await
        .expect("first repository should persist");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo:second",
                "repo:first",
                "/tmp/second",
                Vec::new(),
                Vec::new(),
            )
            .expect("second registration should validate"),
        )
        .await
        .expect("second repository should persist");

    let status = store
        .code_repository_status("repo:first".to_owned())
        .await
        .expect("status should query")
        .expect("repository id should resolve");

    assert_eq!(status.repository_id, "repo:first");
    assert_eq!(status.alias, "first");
}

#[tokio::test]
async fn repo_prefixed_alias_resolves_when_repository_id_is_absent() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo:actual",
                "repo:team-a",
                "/tmp/actual",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");

    let status = store
        .code_repository_status("repo:team-a".to_owned())
        .await
        .expect("status should query")
        .expect("repo-prefixed alias should resolve");

    assert_eq!(status.repository_id, "repo:actual");
    assert_eq!(status.alias, "repo:team-a");
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
async fn hybrid_deduplication_preserves_retrieval_layers() {
    let store = store_with_repository_snapshot(snapshot_with_symbol_and_matching_chunk()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
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
            .contains(&CodeRetrievalLayer::Symbol)
    );
    assert!(
        hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::Lexical)
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
async fn rejects_impact_kind_for_plain_code_queries() {
    let store = store_with_repository_snapshot(snapshot_with_chunk(
        "repo",
        "src/lib.rs",
        "fn retry_policy() {}",
    ))
    .await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let error = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "retry_policy",
                selector,
                CodeQueryKind::Impact,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect_err("impact query kind should require repo impact");

    assert!(error.to_string().contains("repo impact"));
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
async fn impact_imports_use_rust_symbol_namespace_seeds() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_rust_symbol_importer(),
        Vec::new(),
        vec!["rust".to_owned()],
    )
    .await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["src/lib.rs".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
        .await
        .expect("impact should succeed");

    assert!(hits.iter().any(|hit| {
        hit.path == "src/main.rs"
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::ImportGraph)
    }));
}

#[tokio::test]
async fn impact_preserves_deleted_rust_paths_under_language_filters() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_deleted_rust_module_importer(),
        Vec::new(),
        vec!["rust".to_owned()],
    )
    .await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["src/deleted.rs".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
        .await
        .expect("impact should succeed");

    assert!(hits.iter().any(|hit| {
        hit.path == "src/caller.rs"
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::ImportGraph)
    }));
}

#[tokio::test]
async fn impact_preserves_deleted_go_paths_under_language_filters() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_deleted_go_module_importer(),
        Vec::new(),
        vec!["go".to_owned()],
    )
    .await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["go".to_owned()])
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["deleted.go".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
        .await
        .expect("impact should succeed");

    assert!(hits.iter().any(|hit| {
        hit.path == "caller.go"
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::ImportGraph)
    }));
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
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["README.md".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
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
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["src/a.rs".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
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
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: vec!["tests/out.rs".to_owned()],
                deleted_symbol_names: Vec::new(),
            },
        )
        .await
        .expect("impact should succeed");

    assert!(hits.is_empty());
}

#[tokio::test]
async fn impact_callers_can_use_deleted_symbol_names() {
    let store = store_with_repository_snapshot(snapshot_with_unresolved_caller()).await;
    let request = crate::domain::CodeImpactRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        "base",
        "commit",
        10,
    )
    .expect("impact request should validate");

    let hits = store
        .analyze_code_impact(
            request,
            CodeImpactChanges {
                paths: Vec::new(),
                deleted_symbol_names: vec!["target".to_owned()],
            },
        )
        .await
        .expect("impact should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/caller.rs");
}

#[tokio::test]
async fn incremental_updates_retain_existing_degraded_status() {
    let store = store_with_repository_snapshot(snapshot_with_degraded_and_parsed_files()).await;
    store
        .apply_code_index_snapshot(incremental_snapshot_for_parsed_file())
        .await
        .expect("incremental snapshot should apply");

    let status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("status should load")
        .expect("status should exist");

    assert!(status.degraded_reason.is_some());
}

async fn store_with_repository_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
    store_with_repository_snapshot_and_filters(snapshot, Vec::new(), Vec::new()).await
}

async fn store_with_repository_snapshot_and_filters(
    snapshot: CodeIndexSnapshot,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        "/tmp/repo",
        path_filters,
        language_filters,
    )
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
        chunks: vec![chunk("chunk", "file", path, content, None)],
        diagnostics: degraded_reason
            .map(|message| CodeFileDiagnostic {
                repository_id: repository_id.to_owned(),
                path: path.to_owned(),
                parse_status,
                message,
            })
            .into_iter()
            .collect(),
    }
}

fn snapshot_with_symbol_and_matching_chunk() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(
            "target-file",
            "src/lib.rs",
            "rust",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: vec![symbol(
            "target-symbol",
            "target-file",
            "src/lib.rs",
            "target",
        )],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: vec![chunk(
            "target-chunk",
            "target-file",
            "src/lib.rs",
            "fn target()",
            Some("target-symbol"),
        )],
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

fn snapshot_with_rust_symbol_importer() -> CodeIndexSnapshot {
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
                "lib-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "main-file",
                "src/main.rs",
                "rust",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![symbol(
            "retry-symbol",
            "lib-file",
            "src/lib.rs",
            "retry_policy",
        )],
        references: Vec::new(),
        imports: vec![import_module(
            "main-import",
            "main-file",
            "src/main.rs",
            "use crate::retry_policy;",
        )],
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_deleted_rust_module_importer() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(
            "caller-file",
            "src/caller.rs",
            "rust",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![import_module(
            "caller-import",
            "caller-file",
            "src/caller.rs",
            "use crate::deleted;",
        )],
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_deleted_go_module_importer() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(
            "caller-file",
            "caller.go",
            "go",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![import_module(
            "caller-import",
            "caller-file",
            "caller.go",
            "import \"deleted\"",
        )],
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_unresolved_caller() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(
            "caller-file",
            "src/caller.rs",
            "rust",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![call("call", "caller-file", "src/caller.rs", None)],
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_degraded_and_parsed_files() -> CodeIndexSnapshot {
    let mut snapshot = snapshot_with_chunk_status(
        "repo",
        "README.txt",
        "RetryPolicy appears in docs",
        CodeParseStatus::TextOnly,
        Some("tree-sitter grammar is not configured".to_owned()),
    );
    snapshot.files.push(file(
        "src-file",
        "src/lib.rs",
        "rust",
        CodeParseStatus::Parsed,
        None,
    ));
    snapshot.chunks.push(chunk(
        "src-chunk",
        "src-file",
        "src/lib.rs",
        "fn kept() {}",
        None,
    ));
    snapshot
}

fn incremental_snapshot_for_parsed_file() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree-2".to_owned(),
        full_replace: false,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(
            "src-file-2",
            "src/lib.rs",
            "rust",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: vec![chunk(
            "src-chunk-2",
            "src-file-2",
            "src/lib.rs",
            "fn kept() -> u32 { 1 }",
            None,
        )],
        diagnostics: Vec::new(),
    }
}
