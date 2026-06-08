use crate::{
    domain::{
        CodeCallRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeFileRecord, RepositoryCodeRange,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:call-generated:commit:tree";

#[tokio::test]
async fn callees_filter_generated_direct_call_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut calls = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/call_{index:03}.ts");
        let mut generated_file = file(&file_id, &path, "typescript");
        generated_file.is_generated = true;
        files.push(generated_file);
        let mut generated_call = call(&format!("generated-call-{index:03}"), &file_id, &path);
        generated_call.caller_name = Some("TargetCaller".to_owned());
        generated_call.callee_name = "GeneratedCallee".to_owned();
        calls.push(generated_call);
    }
    files.push(file("handwritten-file", "src/zz_call.ts", "typescript"));
    let mut handwritten_call = call("handwritten-call", "handwritten-file", "src/zz_call.ts");
    handwritten_call.caller_name = Some("TargetCaller".to_owned());
    handwritten_call.callee_name = "HandwrittenCallee".to_owned();
    calls.push(handwritten_call);
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
        calls,
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    delete_search_row(&store, "call", "src/zz_call.ts").await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["typescript".to_owned()])
            .expect("selector should validate");
    let mut request = crate::domain::CodeRetrievalRequest::new(
        "TargetCaller",
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
        .expect("callee direct query should find handwritten calls");

    assert!(hits.iter().any(|hit| hit.path == "src/zz_call.ts"));
    assert!(!hits.iter().any(|hit| hit.path.starts_with("generated/")));
}

#[tokio::test]
async fn callees_prefer_handwritten_fts_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut calls = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/call_{index:03}.ts");
        let mut generated_file = file(&file_id, &path, "typescript");
        generated_file.is_generated = true;
        files.push(generated_file);
        let mut generated_call = call(&format!("generated-call-{index:03}"), &file_id, &path);
        generated_call.caller_name = Some("TargetCaller".to_owned());
        generated_call.callee_name = "GeneratedCallee".to_owned();
        calls.push(generated_call);
    }
    files.push(file("handwritten-file", "src/zz_call.ts", "typescript"));
    let mut handwritten_call = call("handwritten-call", "handwritten-file", "src/zz_call.ts");
    handwritten_call.caller_name = Some("TargetCaller".to_owned());
    handwritten_call.callee_name = "HandwrittenCallee".to_owned();
    calls.push(handwritten_call);
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
        calls,
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["typescript".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "TargetCaller",
        selector,
        CodeQueryKind::Callees,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("callee FTS query should keep handwritten calls");

    assert_eq!(
        hits.first().map(|hit| hit.path.as_str()),
        Some("src/zz_call.ts")
    );
}

#[tokio::test]
async fn callees_prefer_handwritten_direct_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut calls = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/call_{index:03}.ts");
        let mut generated_file = file(&file_id, &path, "typescript");
        generated_file.is_generated = true;
        files.push(generated_file);
        let mut generated_call = call(&format!("generated-call-{index:03}"), &file_id, &path);
        generated_call.caller_name = Some("TargetCaller".to_owned());
        generated_call.callee_name = "GeneratedCallee".to_owned();
        calls.push(generated_call);
    }
    files.push(file("handwritten-file", "src/zz_call.ts", "typescript"));
    let mut handwritten_call = call("handwritten-call", "handwritten-file", "src/zz_call.ts");
    handwritten_call.caller_name = Some("TargetCaller".to_owned());
    handwritten_call.callee_name = "HandwrittenCallee".to_owned();
    calls.push(handwritten_call);
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
        calls,
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    delete_search_kind(&store, "call").await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["typescript".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "TargetCaller",
        selector,
        CodeQueryKind::Callees,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("callee direct query should keep handwritten calls");

    assert_eq!(
        hits.first().map(|hit| hit.path.as_str()),
        Some("src/zz_call.ts")
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
