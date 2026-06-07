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
        workspaces: Vec::new(),
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
async fn callers_merge_indirect_bindings_when_direct_calls_exist() {
    let direct_path = "src/direct_driver.c";
    let table_path = "src/generated_table.c";

    let mut direct_symbol = symbol(
        "direct-read-symbol",
        "direct-file",
        direct_path,
        "rk_direct_read",
    );
    direct_symbol.line_range = range(30, 34);
    let mut direct_call = call("direct-read-call", "direct-file", direct_path);
    direct_call.caller_symbol_snapshot_id = Some("direct-read-symbol".to_owned());
    direct_call.caller_name = Some("rk_direct_read".to_owned());
    direct_call.callee_name = "rk_driver_read".to_owned();
    direct_call.target_hint = Some("rk_driver_read".to_owned());
    direct_call.resolution_state = "resolved".to_owned();
    direct_call.confidence_basis_points = 9_000;
    direct_call.confidence_tier = "resolved".to_owned();
    direct_call.line_range = range(32, 32);

    let mut table_symbol = symbol(
        "table-read-symbol",
        "table-file",
        table_path,
        "rk_table_read",
    );
    table_symbol.line_range = range(20, 24);
    let mut indirect_call = call("table-read-call", "table-file", table_path);
    indirect_call.caller_symbol_snapshot_id = Some("table-read-symbol".to_owned());
    indirect_call.caller_name = Some("rk_table_read".to_owned());
    indirect_call.callee_name = "read".to_owned();
    indirect_call.line_range = range(22, 22);

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
            file("direct-file", direct_path, "c"),
            file("table-file", table_path, "c"),
        ],
        symbols: vec![direct_symbol, table_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![direct_call, indirect_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "direct-read-chunk",
                "direct-file",
                direct_path,
                "int rk_direct_read(struct rk_device *dev)\n\
{\n\
    return rk_driver_read(dev);\n\
}",
                Some("direct-read-symbol"),
                range(30, 34),
            ),
            chunk(
                "table-init-chunk",
                "table-file",
                table_path,
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
                table_path,
                "int rk_table_read(struct rk_device *dev, char *buffer, size_t length)\n\
{\n\
    return rk_rows[RK_STAGE_READ].read(dev, buffer, length);\n\
}",
                Some("table-read-symbol"),
                range(20, 24),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("rk_driver_read", CodeQueryKind::Callers))
        .await
        .expect("caller query should merge direct and indirect matches");

    assert!(hits.iter().any(|hit| hit.path == direct_path), "{hits:?}");
    assert!(
        hits.iter().any(|hit| {
            hit.path == table_path && hit.edge_target_hint.as_deref() == Some("rk_driver_read")
        }),
        "{hits:?}"
    );
}

#[tokio::test]
async fn callers_preserve_cross_file_indirect_bindings_with_receiver_context() {
    let binding_path = "src/ops.c";
    let caller_path = "src/driver.c";

    let mut caller_symbol = symbol("driver-read-symbol", "driver-file", caller_path, "rk_read");
    caller_symbol.line_range = range(30, 34);
    caller_symbol.signature = "int rk_read(struct rk_driver_ops *ops)".to_owned();

    let mut indirect_call = call("driver-read-call", "driver-file", caller_path);
    indirect_call.caller_symbol_snapshot_id = Some("driver-read-symbol".to_owned());
    indirect_call.caller_name = Some("rk_read".to_owned());
    indirect_call.callee_name = "read".to_owned();
    indirect_call.line_range = range(32, 32);

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
            file("ops-file", binding_path, "c"),
            file("driver-file", caller_path, "c"),
        ],
        symbols: vec![caller_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![indirect_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "ops-init-chunk",
                "ops-file",
                binding_path,
                "static const struct rk_driver_ops rk_driver_ops = {\n\
    .read = rk_driver_read,\n\
};",
                None,
                range(10, 14),
            ),
            chunk(
                "driver-read-chunk",
                "driver-file",
                caller_path,
                "int rk_read(struct rk_driver_ops *ops)\n\
{\n\
    return ops->read(ops);\n\
}",
                Some("driver-read-symbol"),
                range(30, 34),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("rk_driver_read", CodeQueryKind::Callers))
        .await
        .expect("cross-file indirect caller query should succeed");

    assert!(
        hits.iter().any(|hit| {
            hit.path == caller_path && hit.edge_target_hint.as_deref() == Some("rk_driver_read")
        }),
        "{hits:?}"
    );
}

#[tokio::test]
async fn callers_exclude_generated_indirect_binding_evidence() {
    let binding_path = "generated/ops.c";
    let caller_path = "src/driver.c";

    let mut caller_symbol = symbol("driver-read-symbol", "driver-file", caller_path, "rk_read");
    caller_symbol.line_range = range(30, 34);
    caller_symbol.signature = "int rk_read(struct rk_driver_ops *ops)".to_owned();

    let mut indirect_call = call("driver-read-call", "driver-file", caller_path);
    indirect_call.caller_symbol_snapshot_id = Some("driver-read-symbol".to_owned());
    indirect_call.caller_name = Some("rk_read".to_owned());
    indirect_call.callee_name = "read".to_owned();
    indirect_call.line_range = range(32, 32);

    let mut generated_file = file("ops-file", binding_path, "c");
    generated_file.is_generated = true;
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
        files: vec![generated_file, file("driver-file", caller_path, "c")],
        symbols: vec![caller_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![indirect_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "ops-init-chunk",
                "ops-file",
                binding_path,
                "static const struct rk_driver_ops rk_driver_ops = {\n\
    .read = rk_driver_read,\n\
};",
                None,
                range(10, 14),
            ),
            chunk(
                "driver-read-chunk",
                "driver-file",
                caller_path,
                "int rk_read(struct rk_driver_ops *ops)\n\
{\n\
    return ops->read(ops);\n\
}",
                Some("driver-read-symbol"),
                range(30, 34),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let mut request = request("rk_driver_read", CodeQueryKind::Callers);
    request.exclude_generated = true;
    let hits = store
        .search_code(request)
        .await
        .expect("indirect caller query should ignore generated binding evidence");

    assert!(hits.iter().all(|hit| hit.path != caller_path), "{hits:?}");
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
        workspaces: Vec::new(),
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
        is_generated: false,
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
