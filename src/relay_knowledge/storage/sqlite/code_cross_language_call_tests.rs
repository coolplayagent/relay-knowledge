use std::collections::BTreeSet;

use crate::{
    domain::{
        CodeIndexBatch, CodeIndexResourceBudget, CodeIndexSession, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy,
        RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeReferenceRecord,
        RepositoryCodeSymbolRecord,
    },
    storage::{SqliteGraphStore, code::CodeRepositoryStore},
};

const SOURCE_SCOPE: &str = "code:test:cross-language-calls:commit:tree";

#[tokio::test]
async fn cross_language_call_queries_resolve_c_cpp_cgo_and_rust_ffi_targets() {
    let store = registered_store().await;
    let session = session_for_scope(5);
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: SOURCE_SCOPE.to_owned(),
            batch_index: 1,
            parsed_byte_count: 160,
            files: vec![
                file("header-file", "include/rk_bridge.h", "c"),
                file("c-file", "src/c_entry.c", "c"),
                file("cpp-file", "src/cpp_bridge.cpp", "cpp"),
                file("go-file", "bridge/go_bridge.go", "go"),
                file("rust-file", "crates/rust_bridge/src/lib.rs", "rust"),
            ],
            symbols: vec![
                symbol(
                    "header-rk-c-decode",
                    "header-file",
                    "include/rk_bridge.h",
                    "rk_c_decode",
                    "c",
                    "function_declaration",
                    range(1, 1),
                ),
                symbol(
                    "c-rk-c-decode",
                    "c-file",
                    "src/c_entry.c",
                    "rk_c_decode",
                    "c",
                    "function",
                    range(3, 5),
                ),
                symbol(
                    "c-rk-c-encode",
                    "c-file",
                    "src/c_entry.c",
                    "rk_c_encode",
                    "c",
                    "function",
                    range(6, 6),
                ),
                symbol(
                    "c-entry-process",
                    "c-file",
                    "src/c_entry.c",
                    "rk_c_entry_process",
                    "c",
                    "function",
                    range(7, 11),
                ),
                symbol(
                    "cpp-score",
                    "cpp-file",
                    "src/cpp_bridge.cpp",
                    "rk_cpp_score",
                    "cpp",
                    "function",
                    range(3, 7),
                ),
                symbol(
                    "go-bridge",
                    "go-file",
                    "bridge/go_bridge.go",
                    "RunCgoBridge",
                    "go",
                    "function",
                    range(8, 12),
                ),
                symbol(
                    "rust-bridge",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "run_rust_bridge",
                    "rust",
                    "function",
                    range(8, 11),
                ),
                symbol(
                    "rust-ffi-declaration-a",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "ffi::rk_c_decode",
                    "rust",
                    "function_declaration",
                    range(4, 4),
                ),
                symbol(
                    "rust-ffi-declaration-b",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "ffi::rk_c_decode",
                    "rust",
                    "function_declaration",
                    range(5, 5),
                ),
                with_signature(
                    symbol(
                        "rust-ffi-encode-declaration",
                        "rust-file",
                        "crates/rust_bridge/src/lib.rs",
                        "ffi::rk_c_encode",
                        "rust",
                        "function",
                        range(6, 6),
                    ),
                    "fn rk_c_encode(input: *const u8);",
                ),
                symbol(
                    "c-connect",
                    "c-file",
                    "src/c_entry.c",
                    "connect",
                    "c",
                    "function",
                    range(13, 15),
                ),
                symbol(
                    "constant-not-callable",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "not_callable_target",
                    "rust",
                    "constant",
                    range(16, 16),
                ),
                symbol(
                    "rust-ffi-scoped-constant",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "ffi::scoped_constant",
                    "rust",
                    "constant",
                    range(17, 17),
                ),
            ],
            references: vec![
                reference(
                    "c-calls-cpp",
                    "c-file",
                    "src/c_entry.c",
                    "rk_cpp_score",
                    range(9, 9),
                ),
                reference(
                    "cpp-calls-c",
                    "cpp-file",
                    "src/cpp_bridge.cpp",
                    "rk_c_decode",
                    range(5, 5),
                ),
                reference(
                    "go-calls-c",
                    "go-file",
                    "bridge/go_bridge.go",
                    "C.rk_c_decode",
                    range(10, 10),
                ),
                reference(
                    "rust-calls-c",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "ffi::rk_c_decode",
                    range(10, 10),
                ),
                reference(
                    "rust-calls-c-through-unique-scoped-declaration",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "ffi::rk_c_encode",
                    range(11, 11),
                ),
                reference(
                    "rust-namespaced-connect",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "module::connect",
                    range(12, 12),
                ),
                reference(
                    "rust-module-sys-connect",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "module::sys::connect",
                    range(13, 13),
                ),
                reference(
                    "rust-client-ffi-connect",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "client.ffi.connect",
                    range(14, 14),
                ),
                reference(
                    "rust-c-member-connect",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "C.connect",
                    range(15, 15),
                ),
                reference(
                    "go-object-c-connect",
                    "go-file",
                    "bridge/go_bridge.go",
                    "obj.C.connect",
                    range(14, 14),
                ),
                reference(
                    "rust-ffi-non-callable",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "ffi::not_callable_target",
                    range(18, 18),
                ),
                reference(
                    "rust-ffi-scoped-non-callable",
                    "rust-file",
                    "crates/rust_bridge/src/lib.rs",
                    "ffi::scoped_constant",
                    range(19, 19),
                ),
            ],
            imports: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let c_callers = search(&store, "rk_c_decode", CodeQueryKind::Callers).await;
    let c_caller_paths = c_callers
        .iter()
        .map(|hit| hit.path.as_str())
        .collect::<BTreeSet<_>>();

    assert!(c_caller_paths.contains("src/cpp_bridge.cpp"));
    assert!(c_caller_paths.contains("bridge/go_bridge.go"));
    assert!(c_caller_paths.contains("crates/rust_bridge/src/lib.rs"));
    assert!(
        c_callers
            .iter()
            .all(|hit| hit.edge_resolution_state.as_deref() == Some("resolved"))
    );
    assert!(c_callers.iter().any(|hit| {
        hit.path == "src/cpp_bridge.cpp" && hit.edge_target_hint.as_deref() == Some("rk_c_decode")
    }));
    assert!(c_callers.iter().any(|hit| {
        hit.path == "bridge/go_bridge.go"
            && hit.edge_target_hint.as_deref() == Some("C.rk_c_decode")
    }));
    assert!(c_callers.iter().any(|hit| {
        hit.path == "crates/rust_bridge/src/lib.rs"
            && hit.edge_target_hint.as_deref() == Some("ffi::rk_c_decode")
    }));

    let scoped_rust_callers = search(&store, "ffi::rk_c_decode", CodeQueryKind::Callers).await;
    assert!(scoped_rust_callers.iter().any(|hit| {
        hit.path == "crates/rust_bridge/src/lib.rs"
            && hit.edge_target_hint.as_deref() == Some("ffi::rk_c_decode")
    }));

    let c_encode_callers = search(&store, "rk_c_encode", CodeQueryKind::Callers).await;
    assert!(c_encode_callers.iter().any(|hit| {
        hit.path == "crates/rust_bridge/src/lib.rs"
            && hit.edge_target_hint.as_deref() == Some("ffi::rk_c_encode")
    }));
    let unique_scoped_declaration =
        reference_resolution(&store, "rust-calls-c-through-unique-scoped-declaration").await;
    assert_eq!(unique_scoped_declaration.0, "resolved");
    assert_eq!(
        unique_scoped_declaration.1.as_deref(),
        Some("c-rk-c-encode")
    );
    assert_eq!(
        unique_scoped_declaration.2.as_deref(),
        Some("ffi::rk_c_encode")
    );

    let cpp_callers = search(&store, "rk_cpp_score", CodeQueryKind::Callers).await;
    assert_eq!(cpp_callers[0].path, "src/c_entry.c");
    assert_eq!(
        cpp_callers[0].edge_target_hint.as_deref(),
        Some("rk_cpp_score")
    );

    let c_entry_callees = search(&store, "rk_c_entry_process", CodeQueryKind::Callees).await;
    assert_eq!(c_entry_callees[0].path, "src/c_entry.c");
    assert!(c_entry_callees[0].excerpt.contains("rk_cpp_score"));

    let namespaced_connect = reference_resolution(&store, "rust-namespaced-connect").await;
    assert_eq!(namespaced_connect.0, "unresolved");
    assert_eq!(namespaced_connect.1, None);
    assert_eq!(namespaced_connect.2.as_deref(), Some("module::connect"));

    let module_sys_connect = reference_resolution(&store, "rust-module-sys-connect").await;
    assert_eq!(module_sys_connect.0, "unresolved");
    assert_eq!(module_sys_connect.1, None);
    assert_eq!(
        module_sys_connect.2.as_deref(),
        Some("module::sys::connect")
    );

    let client_ffi_connect = reference_resolution(&store, "rust-client-ffi-connect").await;
    assert_eq!(client_ffi_connect.0, "unresolved");
    assert_eq!(client_ffi_connect.1, None);
    assert_eq!(client_ffi_connect.2.as_deref(), Some("client.ffi.connect"));

    let rust_c_connect = reference_resolution(&store, "rust-c-member-connect").await;
    assert_eq!(rust_c_connect.0, "unresolved");
    assert_eq!(rust_c_connect.1, None);
    assert_eq!(rust_c_connect.2.as_deref(), Some("C.connect"));

    let go_object_c_connect = reference_resolution(&store, "go-object-c-connect").await;
    assert_eq!(go_object_c_connect.0, "unresolved");
    assert_eq!(go_object_c_connect.1, None);
    assert_eq!(go_object_c_connect.2.as_deref(), Some("obj.C.connect"));

    let non_callable = reference_resolution(&store, "rust-ffi-non-callable").await;
    assert_eq!(non_callable.0, "unresolved");
    assert_eq!(non_callable.1, None);
    assert_eq!(non_callable.2.as_deref(), Some("ffi::not_callable_target"));

    let scoped_non_callable = reference_resolution(&store, "rust-ffi-scoped-non-callable").await;
    assert_eq!(scoped_non_callable.0, "unresolved");
    assert_eq!(scoped_non_callable.1, None);
    assert_eq!(
        scoped_non_callable.2.as_deref(),
        Some("ffi::scoped_constant")
    );
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");

    store
}

fn session_for_scope(total_path_count: usize) -> CodeIndexSession {
    CodeIndexSession {
        repository_id: "repo".to_owned(),
        source_scope: SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count,
        changed_path_count: total_path_count,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        resource_budget: CodeIndexResourceBudget::new(1, 1024, 1024).expect("budget"),
    }
}

async fn search(
    store: &SqliteGraphStore,
    query: &str,
    kind: CodeQueryKind,
) -> Vec<crate::domain::CodeRetrievalHit> {
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    store
        .search_code(
            CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
                .expect("request should validate"),
        )
        .await
        .expect("query should succeed")
}

async fn reference_resolution(
    store: &SqliteGraphStore,
    reference_id: &str,
) -> (String, Option<String>, Option<String>) {
    let reference_id = reference_id.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "
                    SELECT resolution_state, target_symbol_snapshot_id, target_hint
                    FROM code_repository_references
                    WHERE source_scope = ?1 AND reference_id = ?2
                    ",
                    rusqlite::params![SOURCE_SCOPE, reference_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("reference resolution should load")
}

fn file(file_id: &str, path: &str, language_id: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("{file_id}-hash"),
        byte_len: 80,
        line_count: 20,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    }
}

fn symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    language_id: &str,
    kind: &str,
    line_range: RepositoryCodeRange,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        name: name.to_owned(),
        qualified_name: format!("{}::{name}", path.replace('/', "::")),
        kind: kind.to_owned(),
        signature: format!("{kind} {name}"),
        doc_comment: None,
        byte_range: range(0, 8),
        line_range,
    }
}

fn with_signature(
    mut record: RepositoryCodeSymbolRecord,
    signature: &str,
) -> RepositoryCodeSymbolRecord {
    record.signature = signature.to_owned();
    record
}

fn reference(
    reference_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    line_range: RepositoryCodeRange,
) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: SOURCE_SCOPE.to_owned(),
        reference_id: reference_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        name: name.to_owned(),
        kind: "call".to_owned(),
        target_symbol_snapshot_id: None,
        target_hint: Some(name.to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 2_500,
        confidence_tier: "ambiguous".to_owned(),
        byte_range: range(0, 8),
        line_range,
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}
