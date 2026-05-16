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

    assert_eq!(hits[0].path, "db/db_impl.cc");
    assert_eq!(
        hits[0].excerpt,
        "SanitizeOptions calls NewLRUCache: result.block_cache = NewLRUCache(8 << 20);"
    );
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
        chunks: Vec::new(),
        diagnostics,
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
        chunks: vec![chunk(
            "sanitize-options-chunk",
            "db-impl-source",
            "db/db_impl.cc",
            "Options SanitizeOptions(const Options& src) {\n    Options result;\n    result.block_cache = NewLRUCache(8 << 20);\n    return result;\n}",
            Some("sanitize-options"),
        )],
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
