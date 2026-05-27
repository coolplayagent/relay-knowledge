use std::collections::BTreeMap;

use rusqlite::params;

use crate::{
    domain::{
        CodeImportRecord, CodeIndexBatch, CodeIndexResourceBudget, CodeIndexSession,
        CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration, CodeRepositorySelector,
        CodeRetrievalRequest, FreshnessPolicy, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

#[tokio::test]
async fn checkpointed_batches_store_edge_search_languages_after_finalize() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:edge-languages";
    let session = session_for_scope(source_scope);
    let rust_file = file(source_scope, "rust-file", "src/lib.rs", "rust");
    let python_file = file(source_scope, "python-file", "py/app.py", "python");
    let rust_reference = reference(
        source_scope,
        "rust-reference",
        "rust-file",
        "src/lib.rs",
        "target",
    );
    let python_import = import(
        source_scope,
        "python-import",
        "python-file",
        "py/app.py",
        "from service import TargetService",
    );

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 1,
            parsed_byte_count: 20,
            files: vec![rust_file, python_file],
            symbols: Vec::new(),
            references: vec![rust_reference],
            imports: vec![python_import],
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("batch should persist");
    assert!(
        search_document_languages(&store, source_scope)
            .await
            .is_empty(),
        "cold scopes should defer edge search rows until finalize rebuilds them"
    );
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let languages = search_document_languages(&store, source_scope).await;

    assert_eq!(
        languages.get(&("reference".to_owned(), "src/lib.rs".to_owned())),
        Some(&"rust".to_owned())
    );
    assert_eq!(
        languages.get(&("call".to_owned(), "src/lib.rs".to_owned())),
        Some(&"rust".to_owned())
    );
    assert_eq!(
        languages.get(&("import".to_owned(), "py/app.py".to_owned())),
        Some(&"python".to_owned())
    );
}

#[tokio::test]
async fn checkpointed_call_search_uses_caller_signature_for_scoped_callee_queries() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:call-signature-search";
    let session = session_for_scope(source_scope);
    let path = "table/table.cc";
    let file = file(source_scope, "table-file", path, "cpp");
    let mut caller = symbol(
        source_scope,
        "internal-get-symbol",
        "table-file",
        path,
        "InternalGet",
        "Status Table::InternalGet(const ReadOptions& options) {",
    );
    caller.line_range = RepositoryCodeRange { start: 20, end: 44 };
    let mut call_reference = reference(
        source_scope,
        "read-block-reference",
        "table-file",
        path,
        "ReadBlock",
    );
    call_reference.line_range = RepositoryCodeRange { start: 30, end: 30 };

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 1,
            parsed_byte_count: 64,
            files: vec![file],
            symbols: vec![caller],
            references: vec![call_reference],
            imports: Vec::new(),
            dependencies: Vec::new(),
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

    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "Table",
        selector,
        CodeQueryKind::Callees,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let hits = store
        .search_code(request)
        .await
        .expect("callee search should succeed");

    assert_eq!(hits[0].path, path);
    assert!(hits[0].excerpt.contains("ReadBlock"));
}

#[tokio::test]
async fn checkpointed_call_search_uses_callee_signature_for_scoped_caller_queries() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:call-callee-signature-search";
    let session = session_for_scope(source_scope);
    let caller_path = "table/table.cc";
    let callee_path = "table/block.cc";
    let caller_file = file(source_scope, "table-file", caller_path, "cpp");
    let callee_file = file(source_scope, "block-file", callee_path, "cpp");
    let mut caller = symbol(
        source_scope,
        "internal-get-symbol",
        "table-file",
        caller_path,
        "InternalGet",
        "Status Table::InternalGet(const ReadOptions& options) {",
    );
    caller.line_range = RepositoryCodeRange { start: 20, end: 44 };
    let callee = symbol(
        source_scope,
        "read-block-symbol",
        "block-file",
        callee_path,
        "ReadBlock",
        "Status BlockReader::ReadBlock(BlockContents* contents) {",
    );
    let mut call_reference = reference(
        source_scope,
        "read-block-reference",
        "table-file",
        caller_path,
        "ReadBlock",
    );
    call_reference.line_range = RepositoryCodeRange { start: 30, end: 30 };

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 1,
            parsed_byte_count: 96,
            files: vec![caller_file, callee_file],
            symbols: vec![caller, callee],
            references: vec![call_reference],
            imports: Vec::new(),
            dependencies: Vec::new(),
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

    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "BlockContents",
        selector,
        CodeQueryKind::Callers,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let hits = store
        .search_code(request)
        .await
        .expect("caller search should succeed");

    assert_eq!(hits[0].path, caller_path);
    assert!(hits[0].excerpt.contains("ReadBlock"));
}

#[tokio::test]
async fn checkpointed_call_search_documents_include_finalized_signatures() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:bulk-call-search-content";
    let session = session_for_scope(source_scope);
    let caller_path = "table/table.cc";
    let callee_path = "table/block.cc";
    let caller_file = file(source_scope, "table-file", caller_path, "cpp");
    let callee_file = file(source_scope, "block-file", callee_path, "cpp");
    let mut caller = symbol(
        source_scope,
        "internal-get-symbol",
        "table-file",
        caller_path,
        "InternalGet",
        "Status Table::InternalGet(const ReadOptions& options) {",
    );
    caller.line_range = RepositoryCodeRange { start: 20, end: 44 };
    let callee = symbol(
        source_scope,
        "read-block-symbol",
        "block-file",
        callee_path,
        "ReadBlock",
        "Status BlockReader::ReadBlock(BlockContents* contents) {",
    );
    let mut call_reference = reference(
        source_scope,
        "read-block-reference",
        "table-file",
        caller_path,
        "ReadBlock",
    );
    call_reference.line_range = RepositoryCodeRange { start: 30, end: 30 };

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 1,
            parsed_byte_count: 96,
            files: vec![caller_file, callee_file],
            symbols: vec![caller, callee],
            references: vec![call_reference],
            imports: Vec::new(),
            dependencies: Vec::new(),
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

    let content = call_search_document_content(&store, source_scope).await;

    assert!(content.contains("InternalGet"));
    assert!(content.contains("ReadBlock"));
    assert!(content.contains("Status Table::InternalGet"));
    assert!(content.contains("Status BlockReader::ReadBlock"));
}

#[tokio::test]
async fn active_scope_reindex_keeps_intermediate_edge_search_rows() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:active-edge-languages";
    let session = session_for_scope(source_scope);
    let rust_file = file(source_scope, "rust-file", "src/lib.rs", "rust");
    let python_file = file(source_scope, "python-file", "py/app.py", "python");
    let rust_reference = reference(
        source_scope,
        "rust-reference",
        "rust-file",
        "src/lib.rs",
        "target",
    );
    let python_import = import(
        source_scope,
        "python-import",
        "python-file",
        "py/app.py",
        "from service import TargetService",
    );

    store
        .begin_code_index_session(session)
        .await
        .expect("session should begin");
    mark_scope_active(&store, source_scope).await;
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 1,
            parsed_byte_count: 20,
            files: vec![rust_file, python_file],
            symbols: Vec::new(),
            references: vec![rust_reference],
            imports: vec![python_import],
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("batch should persist");

    let languages = search_document_languages(&store, source_scope).await;

    assert_eq!(
        languages.get(&("reference".to_owned(), "src/lib.rs".to_owned())),
        Some(&"rust".to_owned())
    );
    assert_eq!(
        languages.get(&("import".to_owned(), "py/app.py".to_owned())),
        Some(&"python".to_owned())
    );
}

#[tokio::test]
async fn retained_scope_reindex_keeps_intermediate_edge_search_rows() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:retained-edge-languages";
    let session = session_for_scope(source_scope);
    mark_scope_retained(&store, source_scope).await;
    let rust_file = file(source_scope, "rust-file", "src/lib.rs", "rust");
    let rust_reference = reference(
        source_scope,
        "rust-reference",
        "rust-file",
        "src/lib.rs",
        "target",
    );

    store
        .begin_code_index_session(session)
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 1,
            parsed_byte_count: 20,
            files: vec![rust_file],
            symbols: Vec::new(),
            references: vec![rust_reference],
            imports: Vec::new(),
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("batch should persist");

    let languages = search_document_languages(&store, source_scope).await;

    assert_eq!(
        languages.get(&("reference".to_owned(), "src/lib.rs".to_owned())),
        Some(&"rust".to_owned())
    );
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");

    store
}

async fn mark_scope_active(store: &SqliteGraphStore, source_scope: &str) {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection.execute(
                "
                UPDATE code_repositories
                SET last_indexed_scope_id = ?1
                WHERE repository_id = 'repo'
                ",
                [source_scope],
            )?;

            Ok(())
        })
        .await
        .expect("active scope should update");
}

async fn mark_scope_retained(store: &SqliteGraphStore, source_scope: &str) {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection.execute(
                "
                INSERT INTO code_repository_scopes (
                    source_scope, repository_id, resolved_commit_sha, tree_hash,
                    path_filters_json, language_filters_json, indexed_file_count,
                    symbol_count, reference_count, chunk_count, stale, degraded_reason
                )
                VALUES (?1, 'repo', 'commit', 'tree', '[]', '[]', 0, 0, 0, 0, 0, NULL)
                ",
                params![source_scope],
            )?;

            Ok(())
        })
        .await
        .expect("retained scope should insert");
}

fn file(
    source_scope: &str,
    file_id: &str,
    path: &str,
    language_id: &str,
) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("{file_id}-hash"),
        byte_len: 20,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    }
}

fn reference(
    source_scope: &str,
    reference_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
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
        byte_range: RepositoryCodeRange { start: 0, end: 6 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn symbol(
    source_scope: &str,
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    signature: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "cpp".to_owned(),
        name: name.to_owned(),
        qualified_name: format!("{}::{name}", path.replace('/', "::")),
        kind: "function".to_owned(),
        signature: signature.to_owned(),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 0, end: 64 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn import(
    source_scope: &str,
    import_id: &str,
    file_id: &str,
    path: &str,
    module: &str,
) -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        import_id: import_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        module: module.to_owned(),
        target_hint: Some(module.to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 10_000,
        confidence_tier: "extracted".to_owned(),
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

fn session_for_scope(source_scope: &str) -> CodeIndexSession {
    CodeIndexSession {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count: 1,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        resource_budget: CodeIndexResourceBudget::new(1, 1024, 1024).expect("budget"),
    }
}

async fn search_document_languages(
    store: &SqliteGraphStore,
    source_scope: &str,
) -> BTreeMap<(String, String), String> {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            let mut statement = connection.prepare(
                "
                SELECT document_kind, path, language_id
                FROM code_repository_search
                WHERE source_scope = ?1
                  AND document_kind IN ('reference', 'import', 'call')
                ",
            )?;
            let rows = statement.query_map([source_scope], |row| {
                Ok((
                    (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                    row.get::<_, String>(2)?,
                ))
            })?;

            rows.collect::<Result<BTreeMap<_, _>, _>>()
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("search document languages should load")
}

async fn call_search_document_content(store: &SqliteGraphStore, source_scope: &str) -> String {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "
                    SELECT content
                    FROM code_repository_search
                    WHERE source_scope = ?1
                      AND document_kind = 'call'
                    ",
                    [source_scope],
                    |row| row.get(0),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("call search document should load")
}
