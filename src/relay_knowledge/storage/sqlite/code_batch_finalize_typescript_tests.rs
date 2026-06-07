use crate::{
    domain::{
        CodeImportRecord, CodeIndexBatch, CodeIndexResourceBudget, CodeIndexSession,
        CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration, CodeRepositorySelector,
        CodeRetrievalHit, CodeRetrievalRequest, FreshnessPolicy, RepositoryCodeFileRecord,
        RepositoryCodeRange, RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

#[tokio::test]
async fn checkpointed_batches_finalize_typescript_named_import_edges() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:typescript-import-finalize";
    let session = session_for_scope(source_scope, 2);

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
            files: vec![file(source_scope, "runtime-file", "src/runtime/client.ts")],
            symbols: vec![symbol(
                source_scope,
                "runtime-client-symbol",
                "runtime-file",
                "src/runtime/client.ts",
                "RuntimeClient",
            )],
            references: Vec::new(),
            imports: Vec::new(),
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("target batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 2,
            parsed_byte_count: 20,
            files: vec![file(source_scope, "importer-file", "src/app/use_client.ts")],
            symbols: Vec::new(),
            references: Vec::new(),
            imports: vec![import(
                source_scope,
                "runtime-client-import",
                "importer-file",
                "src/app/use_client.ts",
                "import { RuntimeClient } from \"../runtime/client\";",
            )],
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("import batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "RuntimeClient", CodeQueryKind::Imports).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/app/use_client.ts");
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("src/runtime/client.ts")
    );
}

#[tokio::test]
async fn checkpointed_batches_finalize_typescript_nonstandard_source_root_import_edges() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:typescript-nonstandard-import-finalize";
    let session = session_for_scope(source_scope, 2);

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
            files: vec![file(
                source_scope,
                "external-client-file",
                "external_deps/ts_sdk/sessionClient.ts",
            )],
            symbols: vec![symbol(
                source_scope,
                "external-client-symbol",
                "external-client-file",
                "external_deps/ts_sdk/sessionClient.ts",
                "ExternalSessionClient",
            )],
            references: Vec::new(),
            imports: Vec::new(),
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("target batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 2,
            parsed_byte_count: 20,
            files: vec![file(source_scope, "importer-file", "src/app/use_client.ts")],
            symbols: Vec::new(),
            references: Vec::new(),
            imports: vec![import(
                source_scope,
                "external-client-import",
                "importer-file",
                "src/app/use_client.ts",
                "import { ExternalSessionClient } from \"ts_sdk/sessionClient\";",
            )],
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("import batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "ExternalSessionClient", CodeQueryKind::Imports).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/app/use_client.ts");
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("external_deps/ts_sdk/sessionClient.ts")
    );
}

#[tokio::test]
async fn checkpointed_batches_finalize_keeps_bare_typescript_packages_unresolved() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:typescript-bare-package-finalize";
    let session = session_for_scope(source_scope, 2);

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
            files: vec![file(source_scope, "react-file", "src/react.ts")],
            symbols: vec![symbol(
                source_scope,
                "react-client-symbol",
                "react-file",
                "src/react.ts",
                "ReactClient",
            )],
            references: Vec::new(),
            imports: Vec::new(),
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("target batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 2,
            parsed_byte_count: 20,
            files: vec![file(source_scope, "importer-file", "src/app/use_client.ts")],
            symbols: Vec::new(),
            references: Vec::new(),
            imports: vec![import(
                source_scope,
                "react-import",
                "importer-file",
                "src/app/use_client.ts",
                "import { ReactClient } from \"react\";",
            )],
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("import batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "react", CodeQueryKind::Imports).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/app/use_client.ts");
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("unresolved"));
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("import { ReactClient } from \"react\";")
    );
}

#[tokio::test]
async fn checkpointed_batches_finalize_typescript_re_export_and_dynamic_import_edges() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:typescript-re-export-finalize";
    let session = session_for_scope(source_scope, 2);

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
            files: vec![file(source_scope, "protocol-file", "src/protocol.ts")],
            symbols: vec![symbol(
                source_scope,
                "runtime-client-symbol",
                "protocol-file",
                "src/protocol.ts",
                "RuntimeClient",
            )],
            references: Vec::new(),
            imports: Vec::new(),
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("target batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 2,
            parsed_byte_count: 20,
            files: vec![file(source_scope, "index-file", "src/app/index.ts")],
            symbols: Vec::new(),
            references: Vec::new(),
            imports: vec![
                import(
                    source_scope,
                    "protocol-re-export",
                    "index-file",
                    "src/app/index.ts",
                    "export type { RuntimeClient } from \"../protocol\";",
                ),
                import(
                    source_scope,
                    "protocol-dynamic-import",
                    "index-file",
                    "src/app/index.ts",
                    "await import(\"../protocol\")",
                ),
            ],
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("import batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "../protocol", CodeQueryKind::Imports).await;

    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|hit| {
        hit.path == "src/app/index.ts"
            && hit.edge_resolution_state.as_deref() == Some("resolved")
            && hit.edge_target_hint.as_deref() == Some("src/protocol.ts")
    }));
    assert!(
        hits.iter()
            .any(|hit| hit.excerpt.contains("export type { RuntimeClient }"))
    );
    assert!(
        hits.iter()
            .any(|hit| hit.excerpt.contains("await import(\"../protocol\")"))
    );
}

#[tokio::test]
async fn checkpointed_finalize_resolves_typescript_imported_call_references() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:typescript-imported-reference-finalize";
    let session = session_for_scope(source_scope, 3);

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 1,
            parsed_byte_count: 40,
            files: vec![
                file(
                    source_scope,
                    "redaction-file",
                    "packages/http-recorder/src/redaction.ts",
                ),
                file(
                    source_scope,
                    "executor-file",
                    "packages/llm/src/route/executor.ts",
                ),
            ],
            symbols: vec![
                symbol(
                    source_scope,
                    "redaction-redact-url",
                    "redaction-file",
                    "packages/http-recorder/src/redaction.ts",
                    "redactUrl",
                ),
                symbol(
                    source_scope,
                    "executor-redact-url",
                    "executor-file",
                    "packages/llm/src/route/executor.ts",
                    "redactUrl",
                ),
            ],
            references: Vec::new(),
            imports: Vec::new(),
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("target batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 2,
            parsed_byte_count: 20,
            files: vec![file(
                source_scope,
                "redactor-file",
                "packages/http-recorder/src/redactor.ts",
            )],
            symbols: Vec::new(),
            references: vec![reference(
                source_scope,
                "redactor-call",
                "redactor-file",
                "packages/http-recorder/src/redactor.ts",
                "redactUrl",
            )],
            imports: vec![import(
                source_scope,
                "redactor-import",
                "redactor-file",
                "packages/http-recorder/src/redactor.ts",
                "import { redactUrl } from \"./redaction\";",
            )],
            dependencies: Vec::new(),
            feature_flags: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("importer batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "redactUrl", CodeQueryKind::Callers).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "packages/http-recorder/src/redactor.ts");
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
    assert_eq!(
        reference_target(&store, source_scope, "redactor-call")
            .await
            .as_deref(),
        Some("redaction-redact-url")
    );
    assert_eq!(hits[0].edge_confidence_basis_points, Some(8_500));
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

fn file(source_scope: &str, file_id: &str, path: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        blob_hash: format!("{file_id}-hash"),
        byte_len: 20,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    }
}

fn symbol(
    source_scope: &str,
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        name: name.to_owned(),
        qualified_name: format!("{}::{name}", path.replace('/', "::")),
        kind: "class".to_owned(),
        signature: format!("class {name}"),
        doc_comment: None,
        byte_range: range(0, 8),
        line_range: range(1, 1),
        symbol_role: None,
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
        line_range: range(1, 1),
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
        byte_range: range(0, 8),
        line_range: range(1, 1),
    }
}

fn session_for_scope(source_scope: &str, total_path_count: usize) -> CodeIndexSession {
    CodeIndexSession {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
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
        workspaces: Vec::new(),
        resource_budget: CodeIndexResourceBudget::new(1, 1024, 1024).expect("budget"),
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

async fn search(
    store: &SqliteGraphStore,
    query: &str,
    kind: CodeQueryKind,
) -> Vec<CodeRetrievalHit> {
    let selector = CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    store
        .search_code(
            CodeRetrievalRequest::new(query, selector, kind, 5, FreshnessPolicy::AllowStale)
                .expect("request should validate"),
        )
        .await
        .expect("query should succeed")
}

async fn reference_target(
    store: &SqliteGraphStore,
    source_scope: &str,
    reference_id: &str,
) -> Option<String> {
    let source_scope = source_scope.to_owned();
    let reference_id = reference_id.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "
                    SELECT target_symbol_snapshot_id
                    FROM code_repository_references
                    WHERE source_scope = ?1 AND reference_id = ?2
                    ",
                    (&source_scope, &reference_id),
                    |row| row.get::<_, Option<String>>(0),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("reference target should load")
}
