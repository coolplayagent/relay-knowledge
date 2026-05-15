use super::*;
use crate::{
    domain::{
        CodeCallRecord, CodeFileDiagnostic, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositorySelector, FreshnessPolicy, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
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
