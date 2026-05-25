use super::*;
use crate::{
    domain::{
        CodeCallRecord, CodeFileDiagnostic, CodeImportRecord, CodeIndexSnapshot, CodeParseStatus,
        CodeQueryKind, CodeRepositorySelector, FreshnessPolicy, RepositoryCodeChunkRecord,
        RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:fixture:commit:tree";

#[tokio::test]
async fn full_scope_serves_narrower_query_filters() {
    let store = store_with_repository_snapshot(snapshot_with_target_symbol()).await;
    let path_selector = CodeRepositorySelector::new(
        "fixture",
        "commit",
        vec!["src/lib.rs".to_owned()],
        Vec::new(),
    )
    .expect("selector should validate");
    let language_selector =
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let no_match_selector =
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["python".to_owned()])
            .expect("selector should validate");

    let path_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                path_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("narrower path filter should use full scope");
    let language_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                language_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("narrower language filter should use full scope");
    let no_match_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                no_match_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("non-matching language filter should return no hits");

    assert_eq!(path_hits.len(), 1);
    assert_eq!(language_hits.len(), 1);
    assert!(no_match_hits.is_empty());
}

#[tokio::test]
async fn full_scope_path_filters_prune_fts_candidates_before_limit() {
    let store =
        store_with_repository_snapshot(snapshot_with_path_filtered_candidate_overflow()).await;
    let selector =
        CodeRepositorySelector::new("fixture", "commit", vec!["src".to_owned()], Vec::new())
            .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                selector,
                CodeQueryKind::Definition,
                1,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("path-filtered full-scope query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/target.rs");
    assert_eq!(hits[0].excerpt, "fn target()");
}

#[tokio::test]
async fn restrictive_scope_rejects_query_filters_outside_indexed_scope() {
    let store = store_with_repository_snapshot_and_filters(
        snapshot_with_target_symbol(),
        vec!["src".to_owned()],
        vec!["rust".to_owned()],
    )
    .await;
    let narrower_selector = CodeRepositorySelector::new(
        "fixture",
        "commit",
        vec!["src/lib.rs".to_owned()],
        Vec::new(),
    )
    .expect("selector should validate");
    let unsupported_path_selector =
        CodeRepositorySelector::new("fixture", "commit", vec!["tests".to_owned()], Vec::new())
            .expect("selector should validate");
    let unsupported_language_selector =
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["python".to_owned()])
            .expect("selector should validate");

    let narrower_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                narrower_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("narrower path filter should use the indexed base scope");
    let path_error = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                unsupported_path_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect_err("path outside indexed scope should be rejected");
    let language_error = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                unsupported_language_selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect_err("language outside indexed scope should be rejected");

    assert_eq!(narrower_hits.len(), 1);
    assert!(path_error.to_string().contains("requested filters"));
    assert!(language_error.to_string().contains("requested filters"));
}

#[tokio::test]
async fn exact_identifier_matches_rank_before_substring_matches() {
    let store = store_with_repository_snapshot(snapshot_with_exact_match_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let definition_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "_build_service",
                selector.clone(),
                CodeQueryKind::Definition,
                1,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("definition query should succeed");
    let caller_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "_summary",
                selector.clone(),
                CodeQueryKind::Callers,
                1,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller query should succeed");
    let callee_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "_summary",
                selector,
                CodeQueryKind::Callees,
                1,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("callee query should succeed");

    assert_eq!(definition_hits[0].excerpt, "fn _build_service()");
    assert_eq!(caller_hits[0].excerpt, "list_connectors calls _summary");
    assert_eq!(callee_hits[0].excerpt, "_summary calls ConnectorSummary");
}

#[tokio::test]
async fn definition_queries_rank_own_camel_case_symbol_name_before_signature_mentions() {
    let store = store_with_repository_snapshot(snapshot_with_type_name_signature_mentions()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "w3 save request",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("definition query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams/connector/w3_models.py");
    assert_eq!(hits[0].excerpt, "class W3ConnectorSaveRequest(BaseModel):");
}

#[tokio::test]
async fn exact_camel_case_definition_queries_rank_own_symbol_before_signature_mentions() {
    let store = store_with_repository_snapshot(snapshot_with_type_name_signature_mentions()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "W3ConnectorSaveRequest",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("definition query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams/connector/w3_models.py");
    assert_eq!(hits[0].excerpt, "class W3ConnectorSaveRequest(BaseModel):");
}

#[tokio::test]
async fn exact_definition_queries_rank_name_match_when_many_signatures_mention_it() {
    let store = store_with_repository_snapshot(snapshot_with_many_signature_mentions()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "W3ConnectorSaveRequest",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("definition query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams/connector/w3_models.py");
    assert_eq!(hits[0].excerpt, "class W3ConnectorSaveRequest(BaseModel):");
}

#[tokio::test]
async fn fuzzy_definition_queries_rank_multi_part_symbol_names_before_single_token_noise() {
    let store = store_with_repository_snapshot(snapshot_with_archive_output_dir_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "archive old eval output directory timestamp suffix",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams_evals/checkpoint.py");
    assert!(hits[0].excerpt.contains("fn archive_output_dir()"));
}

#[tokio::test]
async fn fuzzy_symbol_queries_recall_identifier_when_extra_terms_are_not_in_symbol_document() {
    let store =
        store_with_repository_snapshot(snapshot_with_checkpoint_version_constant_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "checkpoint metadata version constant",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams_evals/checkpoint.py");
    assert!(hits[0].excerpt.contains("_CHECKPOINT_VERSION"));
}

#[tokio::test]
async fn scoped_definition_queries_rank_scoped_member_before_token_permutations() {
    let store = store_with_repository_snapshot(snapshot_with_scoped_cpp_definition_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let db_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "DB::Open",
                selector.clone(),
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("DB::Open query should succeed");
    let write_batch_hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "WriteBatch::Put",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("WriteBatch::Put query should succeed");

    assert_eq!(db_hits[0].path, "db/db_impl.cc");
    assert_eq!(db_hits[0].line_range.start, 1503);
    assert_eq!(write_batch_hits[0].path, "db/write_batch.cc");
    assert_eq!(write_batch_hits[0].line_range.start, 98);
}

#[tokio::test]
async fn callee_queries_rank_resolved_edges_before_ambiguous_ties() {
    let store = store_with_repository_snapshot(snapshot_with_resolved_callee_tie()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "cma_debugfs_init",
                selector,
                CodeQueryKind::Callees,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("callee query should succeed");

    assert_eq!(
        hits.iter()
            .map(|hit| hit.excerpt.as_str())
            .collect::<Vec<_>>(),
        vec![
            "cma_debugfs_init calls cma_debugfs_add_one",
            "cma_debugfs_init calls debugfs_create_dir",
        ]
    );
}

#[tokio::test]
async fn caller_queries_keep_best_ranked_fts_candidates_before_bounded_scoring() {
    let store = store_with_repository_snapshot(snapshot_with_many_caller_candidate_ties()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "TargetCall exactOwner",
                selector,
                CodeQueryKind::Callers,
                1,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].excerpt, "exactOwner calls TargetCall");
    assert_eq!(hits[0].path, "src/exact_owner.py");
}

#[tokio::test]
async fn caller_queries_rank_matching_caller_context_before_same_callee_noise() {
    let store = store_with_repository_snapshot(snapshot_with_same_callee_context_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "TargetCall exactOwner",
                selector,
                CodeQueryKind::Callers,
                3,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].excerpt, "exactOwner calls TargetCall");
    assert_eq!(hits[0].path, "src/z_exact_owner.py");
}

#[tokio::test]
async fn callee_queries_prioritize_related_callee_identifier_parts() {
    let store = store_with_repository_snapshot(snapshot_with_related_callee_names()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "do_mmap",
                selector,
                CodeQueryKind::Callees,
                3,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("callee query should succeed");

    assert_eq!(hits[0].excerpt, "do_mmap calls mmap_region");
}

#[tokio::test]
async fn caller_queries_use_caller_chunk_excerpt_when_available() {
    let store = store_with_repository_snapshot(snapshot_with_call_site_chunk()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "NewLRUCache",
                selector,
                CodeQueryKind::Callers,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("caller query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "db/db_impl.cc");
    assert_eq!(
        hits[0].excerpt,
        "SanitizeOptions calls NewLRUCache: result.block_cache = NewLRUCache(8 << 20);"
    );
}

#[tokio::test]
async fn hybrid_chunk_queries_do_not_require_every_query_term_in_one_candidate() {
    let store = store_with_repository_snapshot(snapshot_with_eval_checkpoint_chunk()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "EvalCheckpointStore signature mismatch append result",
                selector,
                CodeQueryKind::Hybrid,
                3,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].path, "src/relay_teams_evals/checkpoint.py");
    assert!(hits[0].excerpt.contains("EvalCheckpointStore"));
}

#[tokio::test]
async fn hybrid_chunk_queries_prioritize_abstract_interfaces_over_usage_fixtures() {
    let store = store_with_repository_snapshot(snapshot_with_cache_interface_chunk_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "cache interface lookup insert total charge lru",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].path, "include/leveldb/cache.h");
    assert!(hits[0].excerpt.contains("class LEVELDB_EXPORT Cache"));
}

#[tokio::test]
async fn hybrid_chunk_queries_prioritize_header_declarations_for_api_context() {
    let store = store_with_repository_snapshot(snapshot_with_recovery_manifest_chunk_noise()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "Recover descriptor save_manifest VersionEdit",
                selector,
                CodeQueryKind::Hybrid,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].path, "db/db_impl.h");
    assert!(hits[0].excerpt.contains("RecoverLogFile"));
}

#[tokio::test]
async fn parsed_hits_do_not_inherit_repository_degraded_reason() {
    let mut snapshot = snapshot_with_degraded_files(1);
    snapshot.files.push(file(
        "target-file",
        "src/lib.rs",
        "rust",
        CodeParseStatus::Parsed,
        None,
    ));
    snapshot.symbols.push(symbol(
        "target-symbol",
        "target-file",
        "src/lib.rs",
        "target",
    ));
    snapshot.changed_path_count = snapshot.files.len();
    let store = store_with_repository_snapshot(snapshot).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "target",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/lib.rs");
    assert_eq!(hits[0].degraded_reason, None);
}

#[tokio::test]
async fn import_queries_match_include_targets_not_source_paths() {
    let store = store_with_repository_snapshot(snapshot_with_c_imports()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "linux/debugfs.h",
                selector,
                CodeQueryKind::Imports,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("import query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "mm/cma_debug.c");
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("include/linux/debugfs.h")
    );
}

#[tokio::test]
async fn import_queries_can_match_importing_source_paths() {
    let store = store_with_repository_snapshot(snapshot_with_c_imports()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "mm/cma_debug.c",
                selector,
                CodeQueryKind::Imports,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("import query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "mm/cma_debug.c");
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("include/linux/debugfs.h")
    );
}

#[tokio::test]
async fn import_queries_rank_earlier_matching_includes_before_later_ties() {
    let store = store_with_repository_snapshot(snapshot_with_repeated_c_imports()).await;
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "linux/debugfs.h",
                selector,
                CodeQueryKind::Imports,
                3,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("import query should succeed");

    assert_eq!(
        hits.iter().map(|hit| hit.path.as_str()).collect::<Vec<_>>(),
        vec!["mm/cma_debug.c", "fs/debugfs/file.c", "fs/debugfs/inode.c"]
    );
}

fn snapshot_with_target_symbol() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
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
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_path_filtered_candidate_overflow() -> CodeIndexSnapshot {
    let mut files = Vec::new();
    let mut symbols = Vec::new();
    for index in 0..600 {
        let file_id = format!("noise-file-{index:03}");
        let path = format!("vendor/noise_{index:03}.rs");
        files.push(file(&file_id, &path, "rust", CodeParseStatus::Parsed, None));
        symbols.push(symbol(
            &format!("noise-symbol-{index:03}"),
            &file_id,
            &path,
            "target",
        ));
    }

    files.push(file(
        "target-file",
        "src/target.rs",
        "rust",
        CodeParseStatus::Parsed,
        None,
    ));
    symbols.push(symbol(
        "target-symbol",
        "target-file",
        "src/target.rs",
        "target",
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
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_degraded_files(count: usize) -> CodeIndexSnapshot {
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    for index in 0..count {
        let file_id = format!("file-{index}");
        let path = format!("src/degraded_{index}.rs");
        let message = format!("parse degraded {index}");
        files.push(file(
            &file_id,
            &path,
            "rust",
            CodeParseStatus::Partial,
            Some(message.clone()),
        ));
        diagnostics.push(CodeFileDiagnostic {
            repository_id: "repo".to_owned(),
            source_scope: TEST_SOURCE_SCOPE.to_owned(),
            path,
            parse_status: CodeParseStatus::Partial,
            message,
        });
    }

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: count,
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
        chunks: Vec::new(),
        diagnostics,
    }
}

fn snapshot_with_archive_output_dir_noise() -> CodeIndexSnapshot {
    let mut target = symbol(
        "checkpoint-symbol",
        "checkpoint-file",
        "src/relay_teams_evals/checkpoint.py",
        "archive_output_dir",
    );
    let mut output_noise = symbol(
        "output-symbol",
        "output-file",
        "src/relay_teams/sessions/runs/background_tasks/projection.py",
        "_OUTPUT_TRUNCATED_SUFFIX",
    );
    let mut directory_noise = symbol(
        "directory-symbol",
        "directory-file",
        "src/relay_teams/workspace/directory_picker.py",
        "_pick_directory_macos",
    );
    let mut archive_noise = symbol(
        "archive-symbol",
        "archive-file",
        "tests/unit_tests/net/test_github_cli.py",
        "test_archive_output_dir_moves_existing_contents_to_timestamped_sibling",
    );
    for symbol in [
        &mut target,
        &mut output_noise,
        &mut directory_noise,
        &mut archive_noise,
    ] {
        symbol.doc_comment = Some("archive old eval output directory timestamp suffix".to_owned());
    }

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 4,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "checkpoint-file",
                "src/relay_teams_evals/checkpoint.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "output-file",
                "src/relay_teams/sessions/runs/background_tasks/projection.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "directory-file",
                "src/relay_teams/workspace/directory_picker.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "archive-file",
                "tests/unit_tests/net/test_github_cli.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![target, output_noise, directory_noise, archive_noise],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_checkpoint_version_constant_noise() -> CodeIndexSnapshot {
    let mut target = symbol(
        "checkpoint-version-symbol",
        "checkpoint-file",
        "src/relay_teams_evals/checkpoint.py",
        "_CHECKPOINT_VERSION",
    );
    target.kind = "constant".to_owned();
    target.signature = "_CHECKPOINT_VERSION = 1".to_owned();

    let mut checkpoint_noise = symbol(
        "checkpoint-noise",
        "checkpoint-noise-file",
        "src/relay_teams_evals/reporting.py",
        "checkpoint_metadata_report",
    );
    checkpoint_noise.signature = "def checkpoint_metadata_report() -> None:".to_owned();

    let mut version_noise = symbol(
        "version-noise",
        "version-noise-file",
        "src/relay_teams_evals/versioning.py",
        "metadata_version_report",
    );
    version_noise.signature = "def metadata_version_report() -> None:".to_owned();

    let mut constant_noise = symbol(
        "constant-noise",
        "constant-noise-file",
        "src/relay_teams_evals/constants.py",
        "metadata_constant_report",
    );
    constant_noise.signature = "def metadata_constant_report() -> None:".to_owned();

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 4,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "checkpoint-file",
                "src/relay_teams_evals/checkpoint.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "checkpoint-noise-file",
                "src/relay_teams_evals/reporting.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "version-noise-file",
                "src/relay_teams_evals/versioning.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "constant-noise-file",
                "src/relay_teams_evals/constants.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![target, checkpoint_noise, version_noise, constant_noise],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_resolved_callee_tie() -> CodeIndexSnapshot {
    let mut ambiguous = call("ambiguous-callee", "cma-source", "mm/cma_debug.c");
    ambiguous.caller_name = Some("cma_debugfs_init".to_owned());
    ambiguous.callee_name = "debugfs_create_dir".to_owned();
    ambiguous.target_hint = Some("debugfs_create_dir".to_owned());
    ambiguous.line_range = range(205, 205);

    let mut resolved = call("resolved-callee", "cma-source", "mm/cma_debug.c");
    resolved.caller_name = Some("cma_debugfs_init".to_owned());
    resolved.callee_symbol_snapshot_id = Some("cma-debugfs-add-one".to_owned());
    resolved.callee_name = "cma_debugfs_add_one".to_owned();
    resolved.target_hint = Some("cma_debugfs_add_one".to_owned());
    resolved.resolution_state = "resolved".to_owned();
    resolved.confidence_basis_points = 8_000;
    resolved.confidence_tier = "inferred".to_owned();
    resolved.line_range = range(208, 208);

    CodeIndexSnapshot {
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
        files: vec![file(
            "cma-source",
            "mm/cma_debug.c",
            "c",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: vec![symbol(
            "cma-debugfs-add-one",
            "cma-source",
            "mm/cma_debug.c",
            "cma_debugfs_add_one",
        )],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![ambiguous, resolved],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_many_caller_candidate_ties() -> CodeIndexSnapshot {
    let mut files = Vec::new();
    let mut calls = Vec::new();
    for index in 0..550 {
        let file_id = format!("noise-file-{index}");
        let path = format!("src/exactOwner/noise_{index}.py");
        files.push(file(
            &file_id,
            &path,
            "python",
            CodeParseStatus::Parsed,
            None,
        ));
        let mut call = call(&format!("noise-call-{index}"), &file_id, &path);
        call.caller_name = Some(format!("noiseCaller{index}"));
        call.callee_name = "TargetCall".to_owned();
        call.target_hint = Some("TargetCall".to_owned());
        calls.push(call);
    }

    files.push(file(
        "exact-file",
        "src/exact_owner.py",
        "python",
        CodeParseStatus::Parsed,
        None,
    ));
    let mut exact = call("exact-call", "exact-file", "src/exact_owner.py");
    exact.caller_name = Some("exactOwner".to_owned());
    exact.callee_name = "TargetCall".to_owned();
    exact.target_hint = Some("TargetCall".to_owned());
    exact.resolution_state = "resolved".to_owned();
    exact.confidence_basis_points = 8_000;
    exact.confidence_tier = "inferred".to_owned();
    calls.push(exact);

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
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls,
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_same_callee_context_noise() -> CodeIndexSnapshot {
    let mut first_noise = call("first-noise-call", "first-noise-file", "src/a_noise.py");
    first_noise.caller_name = Some("otherOwner".to_owned());
    first_noise.callee_name = "TargetCall".to_owned();
    first_noise.target_hint = Some("TargetCall".to_owned());

    let mut second_noise = call("second-noise-call", "second-noise-file", "src/b_noise.py");
    second_noise.caller_name = Some("anotherOwner".to_owned());
    second_noise.callee_name = "TargetCall".to_owned();
    second_noise.target_hint = Some("TargetCall".to_owned());

    let mut exact = call("exact-call", "exact-file", "src/z_exact_owner.py");
    exact.caller_name = Some("exactOwner".to_owned());
    exact.callee_name = "TargetCall".to_owned();
    exact.target_hint = Some("TargetCall".to_owned());

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 3,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "first-noise-file",
                "src/a_noise.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "second-noise-file",
                "src/b_noise.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "exact-file",
                "src/z_exact_owner.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![first_noise, second_noise, exact],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_call_site_chunk() -> CodeIndexSnapshot {
    let mut caller = symbol(
        "sanitize-options",
        "db-impl-source",
        "db/db_impl.cc",
        "SanitizeOptions",
    );
    caller.language_id = "cpp".to_owned();
    caller.line_range = range(110, 124);

    let mut call = call("new-lru-cache-call", "db-impl-source", "db/db_impl.cc");
    call.caller_symbol_snapshot_id = Some("sanitize-options".to_owned());
    call.caller_name = Some("SanitizeOptions".to_owned());
    call.callee_name = "NewLRUCache".to_owned();
    call.target_hint = Some("NewLRUCache".to_owned());
    call.resolution_state = "resolved".to_owned();
    call.confidence_basis_points = 8_000;
    call.confidence_tier = "inferred".to_owned();
    call.line_range = range(116, 116);

    CodeIndexSnapshot {
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
        files: vec![file(
            "db-impl-source",
            "db/db_impl.cc",
            "cpp",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: vec![caller],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            RepositoryCodeChunkRecord {
                line_range: range(110, 115),
                ..chunk(
                    "sanitize-options-prologue",
                    "db-impl-source",
                    "db/db_impl.cc",
                    "Options SanitizeOptions(const Options& src) {\n    Options result;",
                    Some("sanitize-options"),
                )
            },
            RepositoryCodeChunkRecord {
                line_range: range(116, 124),
                ..chunk(
                    "sanitize-options-call-site",
                    "db-impl-source",
                    "db/db_impl.cc",
                    "    result.block_cache = NewLRUCache(8 << 20);\n    return result;\n}",
                    Some("sanitize-options"),
                )
            },
        ],
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_eval_checkpoint_chunk() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
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
        files: vec![file(
            "checkpoint-source",
            "src/relay_teams_evals/checkpoint.py",
            "python",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![chunk(
            "checkpoint-chunk",
            "checkpoint-source",
            "src/relay_teams_evals/checkpoint.py",
            "class EvalCheckpointStore:\n    def ensure_initialized(self, signature):\n        raise ValueError(\"Checkpoint signature does not match\")\n\n    def append_result(self, result):\n        self._results_path.write_text(result.model_dump_json())",
            None,
        )],
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_cache_interface_chunk_noise() -> CodeIndexSnapshot {
    let target = chunk(
        "cache-interface-chunk",
        "cache-header",
        "include/leveldb/cache.h",
        "class LEVELDB_EXPORT Cache {\n public:\n  virtual Handle* Insert(const Slice& key, void* value, size_t charge,\n                         void (*deleter)(const Slice& key, void* value)) = 0;\n  virtual Handle* Lookup(const Slice& key) = 0;\n  virtual size_t TotalCharge() const = 0;\n};",
        None,
    );
    let noise = chunk(
        "cache-fixture-chunk",
        "cache-fixture",
        "benchmarks/cache_lru_fixture.cc",
        "class CacheFixture {\n public:\n  CacheFixture() : cache_(NewLRUCache(kCacheSize)) {}\n  int Lookup(int key) { return cache_->Lookup(EncodeKey(key)) == nullptr ? -1 : 0; }\n  void Insert(int key, int value, int charge = 1) { cache_->Insert(EncodeKey(key), EncodeValue(value), charge, nullptr); }\n  size_t TotalCharge() const { return cache_->TotalCharge(); }\n};",
        None,
    );

    CodeIndexSnapshot {
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
            file(
                "cache-header",
                "include/leveldb/cache.h",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "cache-fixture",
                "benchmarks/cache_lru_fixture.cc",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![target, noise],
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_recovery_manifest_chunk_noise() -> CodeIndexSnapshot {
    let target = chunk(
        "recover-header-chunk",
        "db-impl-header",
        "db/db_impl.h",
        "class DBImpl {\n  // Switches to a new log-file/memtable and writes a new descriptor iff successful.\n  Status RecoverLogFile(uint64_t log_number, bool last_log, bool* save_manifest,\n                        VersionEdit* edit, SequenceNumber* max_sequence)\n      EXCLUSIVE_LOCKS_REQUIRED(mutex_);\n  Status WriteLevel0Table(MemTable* mem, VersionEdit* edit, Version* base)\n      EXCLUSIVE_LOCKS_REQUIRED(mutex_);\n};",
        None,
    );
    let noise = chunk(
        "recover-implementation-chunk",
        "db-impl-source",
        "db/db_impl.cc",
        "Status DBImpl::RecoverLogFile(uint64_t log_number, bool last_log, bool* save_manifest,\n                              VersionEdit* edit, SequenceNumber* max_sequence) {\n  if (*save_manifest) {\n    descriptor_log_->AddRecord(edit->Encode());\n  }\n  return Status::OK();\n}",
        None,
    );

    CodeIndexSnapshot {
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
            file(
                "db-impl-header",
                "db/db_impl.h",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "db-impl-source",
                "db/db_impl.cc",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![target, noise],
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_related_callee_names() -> CodeIndexSnapshot {
    let mut unrelated = call("unmapped-area", "mmap-source", "mm/mmap.c");
    unrelated.caller_name = Some("do_mmap".to_owned());
    unrelated.callee_name = "__get_unmapped_area".to_owned();
    unrelated.target_hint = Some("__get_unmapped_area".to_owned());
    unrelated.resolution_state = "resolved".to_owned();
    unrelated.confidence_basis_points = 8_000;
    unrelated.confidence_tier = "inferred".to_owned();
    unrelated.line_range = range(408, 408);

    let mut related = call("mmap-region", "mmap-source", "mm/mmap.c");
    related.caller_name = Some("do_mmap".to_owned());
    related.callee_name = "mmap_region".to_owned();
    related.target_hint = Some("mmap_region".to_owned());
    related.resolution_state = "resolved".to_owned();
    related.confidence_basis_points = 8_000;
    related.confidence_tier = "inferred".to_owned();
    related.line_range = range(560, 560);

    CodeIndexSnapshot {
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
        files: vec![file(
            "mmap-source",
            "mm/mmap.c",
            "c",
            CodeParseStatus::Parsed,
            None,
        )],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![unrelated, related],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_exact_match_noise() -> CodeIndexSnapshot {
    let mut exact_callee = call("exact-callee", "connector-file", "src/connector.py");
    exact_callee.caller_name = Some("list_connectors".to_owned());
    exact_callee.callee_name = "_summary".to_owned();
    exact_callee.target_hint = Some("_summary".to_owned());
    exact_callee.line_range = range(10, 10);

    let mut noisy_callee = call("noisy-callee", "agent-file", "src/agent.py");
    noisy_callee.caller_name = Some("agent_runtimes_list".to_owned());
    noisy_callee.callee_name = "_render_agent_summary_table".to_owned();
    noisy_callee.target_hint = Some("_render_agent_summary_table".to_owned());
    noisy_callee.line_range = range(5, 5);

    let mut exact_caller = call("exact-caller", "summary-file", "src/summary.py");
    exact_caller.caller_name = Some("_summary".to_owned());
    exact_caller.callee_name = "ConnectorSummary".to_owned();
    exact_caller.target_hint = Some("ConnectorSummary".to_owned());
    exact_caller.line_range = range(20, 20);

    let mut noisy_caller = call("noisy-caller", "agent-file", "src/agent.py");
    noisy_caller.caller_name = Some("_render_agent_summary_table".to_owned());
    noisy_caller.callee_name = "echo".to_owned();
    noisy_caller.target_hint = Some("echo".to_owned());
    noisy_caller.line_range = range(6, 6);

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 4,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "connector-file",
                "src/connector.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "agent-file",
                "src/agent.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "summary-file",
                "src/summary.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "builder-file",
                "tests/builder.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![
            symbol(
                "exact-builder",
                "builder-file",
                "tests/builder.py",
                "_build_service",
            ),
            symbol(
                "noisy-builder",
                "builder-file",
                "tests/builder.py",
                "_build_service_with_control",
            ),
        ],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![exact_callee, noisy_callee, exact_caller, noisy_caller],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_type_name_signature_mentions() -> CodeIndexSnapshot {
    let mut request_type = symbol(
        "w3-save-request",
        "w3-models-file",
        "src/relay_teams/connector/w3_models.py",
        "W3ConnectorSaveRequest",
    );
    request_type.language_id = "python".to_owned();
    request_type.kind = "class".to_owned();
    request_type.signature = "class W3ConnectorSaveRequest(BaseModel):".to_owned();

    let mut save_method = symbol(
        "save-w3-connector",
        "service-file",
        "src/relay_teams/connector/service.py",
        "save_w3_connector",
    );
    save_method.language_id = "python".to_owned();
    save_method.kind = "method".to_owned();
    save_method.signature =
        "async def save_w3_connector(self, request: W3ConnectorSaveRequest)".to_owned();

    CodeIndexSnapshot {
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
            file(
                "w3-models-file",
                "src/relay_teams/connector/w3_models.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "service-file",
                "src/relay_teams/connector/service.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: vec![save_method, request_type],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_many_signature_mentions() -> CodeIndexSnapshot {
    let mut request_type = symbol(
        "w3-save-request",
        "w3-models-file",
        "src/relay_teams/connector/w3_models.py",
        "W3ConnectorSaveRequest",
    );
    request_type.language_id = "python".to_owned();
    request_type.kind = "class".to_owned();
    request_type.signature = "class W3ConnectorSaveRequest(BaseModel):".to_owned();
    request_type.line_range = range(1_000, 1_000);

    let mut symbols = Vec::new();
    for index in 0..550 {
        let mut save_method = symbol(
            &format!("save-w3-connector-{index}"),
            "service-file",
            "src/relay_teams/connector/service.py",
            &format!("save_w3_connector_{index}"),
        );
        save_method.language_id = "python".to_owned();
        save_method.kind = "method".to_owned();
        save_method.signature =
            format!("async def save_w3_connector_{index}(self, request: W3ConnectorSaveRequest)");
        save_method.line_range = range(index + 1, index + 1);
        symbols.push(save_method);
    }
    symbols.push(request_type);

    CodeIndexSnapshot {
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
            file(
                "w3-models-file",
                "src/relay_teams/connector/w3_models.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "service-file",
                "src/relay_teams/connector/service.py",
                "python",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_scoped_cpp_definition_noise() -> CodeIndexSnapshot {
    let mut db_open = symbol("db-open", "db-impl-source", "db/db_impl.cc", "Open");
    db_open.language_id = "cpp".to_owned();
    db_open.qualified_name = "leveldb.DB.Open".to_owned();
    db_open.signature =
        "Status DB::Open(const Options& options, const std::string& dbname, DB** dbptr)".to_owned();
    db_open.line_range = range(1503, 1503);

    let mut open_db = symbol(
        "open-db-helper",
        "fault-injection-source",
        "db/fault_injection_test.cc",
        "OpenDB",
    );
    open_db.language_id = "cpp".to_owned();
    open_db.qualified_name = "leveldb.FaultInjectionTest.OpenDB".to_owned();
    open_db.signature = "Status OpenDB()".to_owned();
    open_db.line_range = range(453, 458);

    let mut write_batch_put = symbol(
        "write-batch-put",
        "write-batch-source",
        "db/write_batch.cc",
        "Put",
    );
    write_batch_put.language_id = "cpp".to_owned();
    write_batch_put.qualified_name = "leveldb.WriteBatch.Put".to_owned();
    write_batch_put.signature =
        "void WriteBatch::Put(const Slice& key, const Slice& value)".to_owned();
    write_batch_put.line_range = range(98, 98);

    let mut c_wrapper = symbol(
        "c-wrapper-put",
        "c-source",
        "db/c.cc",
        "leveldb_writebatch_put",
    );
    c_wrapper.language_id = "cpp".to_owned();
    c_wrapper.signature =
        "void leveldb_writebatch_put(leveldb_writebatch_t* b, const char* key, size_t klen)"
            .to_owned();
    c_wrapper.line_range = range(332, 335);

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 4,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file(
                "db-impl-source",
                "db/db_impl.cc",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "fault-injection-source",
                "db/fault_injection_test.cc",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "write-batch-source",
                "db/write_batch.cc",
                "cpp",
                CodeParseStatus::Parsed,
                None,
            ),
            file("c-source", "db/c.cc", "cpp", CodeParseStatus::Parsed, None),
        ],
        symbols: vec![open_db, db_open, c_wrapper, write_batch_put],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_c_imports() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
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
            file(
                "debugfs-header",
                "include/linux/debugfs.h",
                "c",
                CodeParseStatus::Parsed,
                None,
            ),
            file(
                "cma-source",
                "mm/cma_debug.c",
                "c",
                CodeParseStatus::Parsed,
                None,
            ),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![
            import(
                "debugfs-internal-include",
                "debugfs-header",
                "include/linux/debugfs.h",
                "#include <linux/fs.h>",
                Some("include/linux/fs.h"),
                "unresolved",
            ),
            import(
                "cma-debugfs-include",
                "cma-source",
                "mm/cma_debug.c",
                "#include <linux/debugfs.h>",
                Some("include/linux/debugfs.h"),
                "resolved",
            ),
        ],
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_repeated_c_imports() -> CodeIndexSnapshot {
    let mut snapshot = snapshot_with_c_imports();
    snapshot.changed_path_count = 4;
    snapshot.files.extend([
        file(
            "debugfs-file-source",
            "fs/debugfs/file.c",
            "c",
            CodeParseStatus::Parsed,
            None,
        ),
        file(
            "debugfs-inode-source",
            "fs/debugfs/inode.c",
            "c",
            CodeParseStatus::Parsed,
            None,
        ),
    ]);
    let mut file_import = import(
        "debugfs-file-include",
        "debugfs-file-source",
        "fs/debugfs/file.c",
        "#include <linux/debugfs.h>",
        Some("include/linux/debugfs.h"),
        "resolved",
    );
    file_import.line_range = range(16, 16);
    let mut inode_import = import(
        "debugfs-inode-include",
        "debugfs-inode-source",
        "fs/debugfs/inode.c",
        "#include <linux/debugfs.h>",
        Some("include/linux/debugfs.h"),
        "resolved",
    );
    inode_import.line_range = range(23, 23);
    snapshot.imports[1].line_range = range(9, 9);
    snapshot.imports.extend([file_import, inode_import]);

    snapshot
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
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 1,
        parse_status,
        degraded_reason,
    }
}

fn symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "function".to_owned(),
        signature: format!("fn {name}()"),
        doc_comment: None,
        byte_range: range(0, 1),
        line_range: range(1, 1),
    }
}

fn chunk(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    symbol_snapshot_id: Option<&str>,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "cpp".to_owned(),
        content: content.to_owned(),
        byte_range: range(0, content.len() as u32),
        line_range: range(110, 124),
        symbol_snapshot_id: symbol_snapshot_id.map(str::to_owned),
    }
}

fn call(call_id: &str, file_id: &str, path: &str) -> CodeCallRecord {
    CodeCallRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        call_id: call_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        caller_symbol_snapshot_id: None,
        caller_name: None,
        callee_symbol_snapshot_id: None,
        callee_name: "target".to_owned(),
        target_hint: None,
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 2_500,
        confidence_tier: "ambiguous".to_owned(),
        line_range: range(1, 1),
    }
}

fn import(
    import_id: &str,
    file_id: &str,
    path: &str,
    module: &str,
    target_hint: Option<&str>,
    resolution_state: &str,
) -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        import_id: import_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        module: module.to_owned(),
        target_hint: target_hint.map(str::to_owned),
        resolution_state: resolution_state.to_owned(),
        confidence_basis_points: if resolution_state == "resolved" {
            8_000
        } else {
            2_500
        },
        confidence_tier: if resolution_state == "resolved" {
            "inferred".to_owned()
        } else {
            "ambiguous".to_owned()
        },
        line_range: range(1, 1),
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

async fn store_with_repository_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
    store_with_repository_snapshot_and_filters(snapshot, Vec::new(), Vec::new()).await
}

async fn store_with_repository_snapshot_and_filters(
    mut snapshot: CodeIndexSnapshot,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        "/tmp/repo",
        path_filters.clone(),
        language_filters.clone(),
    )
    .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    snapshot.path_filters = path_filters;
    snapshot.language_filters = language_filters;
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");

    store
}
