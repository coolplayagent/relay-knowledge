use crate::{
    domain::{
        CodeCallRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:ambiguous-generated:commit:tree";

#[tokio::test]
async fn callees_filter_generated_ambiguous_implementations_before_candidate_limit() {
    let store = store_with_snapshot(snapshot_with_generated_ambiguous_callee_noise()).await;
    let selector =
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["python".to_owned()])
            .expect("selector should validate");
    let mut request = crate::domain::CodeRetrievalRequest::new(
        "dispatch",
        selector,
        CodeQueryKind::Callees,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    request.exclude_generated = true;

    let hits = store
        .search_code(request)
        .await
        .expect("generated callee candidates should be filtered before limit");

    assert!(
        hits.iter()
            .any(|hit| hit.path == "src/service/zz_handwritten.py")
    );
    assert!(
        !hits
            .iter()
            .any(|hit| hit.path.starts_with("src/service/aa_generated_"))
    );
}

#[tokio::test]
async fn callees_prefer_handwritten_ambiguous_implementations_before_candidate_limit() {
    let store = store_with_snapshot(snapshot_with_generated_ambiguous_callee_noise()).await;
    let selector =
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["python".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "dispatch",
        selector,
        CodeQueryKind::Callees,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("handwritten callee candidates should survive generated noise");
    let hit_paths = hits
        .iter()
        .map(|hit| format!("{}:{:.3}", hit.path, hit.score))
        .collect::<Vec<_>>();

    assert!(
        hits.iter()
            .any(|hit| hit.path == "src/service/zz_handwritten.py"),
        "hits: {hit_paths:?}"
    );
}

fn snapshot_with_generated_ambiguous_callee_noise() -> CodeIndexSnapshot {
    let mut files = vec![file(
        "caller-file",
        "src/service/caller.py",
        "python",
        false,
    )];
    let mut symbols = Vec::new();
    for index in 0..140 {
        let file_id = format!("generated-impl-file-{index:03}");
        let path = format!("src/service/aa_generated_{index:03}.py");
        files.push(file(&file_id, &path, "python", true));
        symbols.push(symbol(
            &format!("generated-impl-{index:03}"),
            &file_id,
            &path,
            "handle",
            "python",
        ));
    }
    files.push(file(
        "handwritten-impl-file",
        "src/service/zz_handwritten.py",
        "python",
        false,
    ));
    symbols.push(symbol(
        "handwritten-impl",
        "handwritten-impl-file",
        "src/service/zz_handwritten.py",
        "handle",
        "python",
    ));
    let mut ambiguous_call = call("ambiguous-call", "caller-file", "src/service/caller.py");
    ambiguous_call.caller_name = Some("dispatch".to_owned());
    ambiguous_call.callee_name = "handle".to_owned();
    ambiguous_call.target_hint = Some("handle".to_owned());

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
        calls: vec![ambiguous_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn file(
    file_id: &str,
    path: &str,
    language_id: &str,
    is_generated: bool,
) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 20,
        parse_status: CodeParseStatus::Parsed,
        is_generated,
        degraded_reason: None,
    }
}

fn symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    language_id: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "function".to_owned(),
        signature: format!("def {name}(payload): ..."),
        doc_comment: None,
        byte_range: range(0, 1),
        line_range: range(1, 1),
        symbol_role: None,
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
        resolution_state: "ambiguous".to_owned(),
        confidence_basis_points: 5_000,
        confidence_tier: "ambiguous".to_owned(),
        line_range: range(10, 10),
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
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
