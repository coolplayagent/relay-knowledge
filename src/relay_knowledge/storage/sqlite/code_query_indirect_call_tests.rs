use crate::{
    domain::{
        CodeCallRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:indirect-call:commit:tree";

#[tokio::test]
async fn callers_follow_designated_function_pointer_bindings() {
    let path = "src/generated_table.c";
    let mut caller_symbol = symbol("table-read-symbol", "table-file", path, "rk_table_read");
    caller_symbol.language_id = "c".to_owned();
    caller_symbol.signature =
        "int rk_table_read(struct rk_device *dev, char *buffer, size_t length)".to_owned();
    caller_symbol.line_range = range(20, 24);

    let mut read_call = call("table-read-call", "table-file", path);
    read_call.caller_symbol_snapshot_id = Some("table-read-symbol".to_owned());
    read_call.caller_name = Some("rk_table_read".to_owned());
    read_call.callee_name = "read".to_owned();
    read_call.target_hint = Some("read".to_owned());
    read_call.line_range = range(22, 22);

    let store = store_with_snapshot(CodeIndexSnapshot {
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
        files: vec![file("table-file", path, "c")],
        symbols: vec![caller_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![read_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "table-init-chunk",
                "table-file",
                path,
                "static const struct rk_table_row rk_rows[] = {\n\
    [RK_STAGE_READ] = {\n\
        .read = rk_driver_read,\n\
    },\n\
};",
                None,
                range(10, 16),
            ),
            chunk(
                "table-read-chunk",
                "table-file",
                path,
                "int rk_table_read(struct rk_device *dev, char *buffer, size_t length)\n\
{\n\
    return rk_rows[RK_STAGE_READ].read(dev, buffer, length);\n\
}",
                Some("table-read-symbol"),
                range(20, 24),
            ),
        ],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("rk_driver_read", CodeQueryKind::Callers))
        .await
        .expect("indirect caller query should succeed");

    assert_eq!(hits[0].path, path);
    assert_eq!(hits[0].edge_target_hint.as_deref(), Some("rk_driver_read"));
    assert!(hits[0].excerpt.contains("rk_rows[RK_STAGE_READ].read"));
}

#[tokio::test]
async fn indirect_callers_ignore_same_field_calls_in_other_files() {
    let binding_path = "src/generated_table.c";
    let unrelated_path = "src/unrelated_device.c";
    let mut caller_symbol = symbol(
        "table-read-symbol",
        "table-file",
        binding_path,
        "rk_table_read",
    );
    caller_symbol.language_id = "c".to_owned();
    caller_symbol.line_range = range(20, 24);

    let mut local_read_call = call("table-read-call", "table-file", binding_path);
    local_read_call.caller_symbol_snapshot_id = Some("table-read-symbol".to_owned());
    local_read_call.caller_name = Some("rk_table_read".to_owned());
    local_read_call.callee_name = "read".to_owned();
    local_read_call.line_range = range(22, 22);

    let mut unrelated_call = call("unrelated-read-call", "unrelated-file", unrelated_path);
    unrelated_call.caller_name = Some("poll_unrelated_device".to_owned());
    unrelated_call.callee_name = "read".to_owned();
    unrelated_call.line_range = range(42, 42);

    let store = store_with_snapshot(CodeIndexSnapshot {
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
            file("table-file", binding_path, "c"),
            file("unrelated-file", unrelated_path, "c"),
        ],
        symbols: vec![caller_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![local_read_call, unrelated_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "table-init-chunk",
                "table-file",
                binding_path,
                "static const struct rk_table_row rk_rows[] = {\n\
    [RK_STAGE_READ] = {\n\
        .read = rk_driver_read,\n\
    },\n\
};",
                None,
                range(10, 16),
            ),
            chunk(
                "table-read-chunk",
                "table-file",
                binding_path,
                "int rk_table_read(struct rk_device *dev, char *buffer, size_t length)\n\
{\n\
    return rk_rows[RK_STAGE_READ].read(dev, buffer, length);\n\
}",
                Some("table-read-symbol"),
                range(20, 24),
            ),
            chunk(
                "unrelated-read-chunk",
                "unrelated-file",
                unrelated_path,
                "int poll_unrelated_device(struct rk_device *dev)\n\
{\n\
    return dev->ops.read(dev);\n\
}",
                None,
                range(40, 44),
            ),
        ],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("rk_driver_read", CodeQueryKind::Callers))
        .await
        .expect("indirect caller query should succeed");

    assert!(hits.iter().any(|hit| hit.path == binding_path), "{hits:?}");
    assert!(
        hits.iter().all(|hit| hit.path != unrelated_path),
        "{hits:?}"
    );
}

fn request(query: &str, kind: CodeQueryKind) -> crate::domain::CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    crate::domain::CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
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
        line_count: 80,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
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
        language_id: "c".to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "function".to_owned(),
        signature: format!("int {name}(void)"),
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

fn chunk(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    symbol_snapshot_id: Option<&str>,
    line_range: RepositoryCodeRange,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "c".to_owned(),
        content: content.to_owned(),
        byte_range: range(0, content.len() as u32),
        line_range,
        symbol_snapshot_id: symbol_snapshot_id.map(str::to_owned),
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
