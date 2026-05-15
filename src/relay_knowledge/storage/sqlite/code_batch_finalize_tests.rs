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
async fn checkpointed_batches_finalize_cross_batch_call_edges() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:call-finalize";
    let session = session_for_scope(source_scope, 2);
    let target_file = file(
        source_scope,
        "target-file",
        "src/target.rs",
        "rust",
        CodeParseStatus::Parsed,
    );
    let caller_file = file(
        source_scope,
        "caller-file",
        "src/caller.rs",
        "rust",
        CodeParseStatus::Parsed,
    );
    let target_symbol = symbol(
        source_scope,
        "target-symbol",
        "target-file",
        "src/target.rs",
        "target",
        "rust",
    );
    let target_reference = reference(
        source_scope,
        "target-reference",
        "caller-file",
        "src/caller.rs",
        "target",
    );

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    let checkpoint = store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 1,
            parsed_byte_count: 20,
            files: vec![target_file],
            symbols: vec![target_symbol],
            references: Vec::new(),
            imports: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("first batch should persist");
    assert_eq!(checkpoint.batch_count, 1);
    let indexing_status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("status should load")
        .expect("status should exist");
    assert_eq!(indexing_status.state, "indexing");
    assert_eq!(indexing_status.indexed_file_count, 1);
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 2,
            parsed_byte_count: 20,
            files: vec![caller_file],
            symbols: Vec::new(),
            references: vec![target_reference],
            imports: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("second batch should persist");
    let summary = store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    assert_eq!(summary.progress.batch_count, 2);
    assert_eq!(summary.progress.checkpoint_file_count, 2);
    let hits = search(&store, "target", CodeQueryKind::Callers).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
}

#[tokio::test]
async fn checkpointed_batches_finalize_python_import_edges() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:python-imports";
    let session = session_for_scope(source_scope, 2);
    let model_file = file(
        source_scope,
        "model-file",
        "src/relay_teams/connector/w3_models.py",
        "python",
        CodeParseStatus::Parsed,
    );
    let service_file = file(
        source_scope,
        "service-file",
        "src/relay_teams/connector/service.py",
        "python",
        CodeParseStatus::Parsed,
    );
    let request_symbol = symbol(
        source_scope,
        "request-symbol",
        "model-file",
        "src/relay_teams/connector/w3_models.py",
        "W3ConnectorSaveRequest",
        "python",
    );
    let service_import = import(
        source_scope,
        "service-import",
        "service-file",
        "src/relay_teams/connector/service.py",
        "from relay_teams.connector.w3_models import W3ConnectorSaveRequest",
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
            files: vec![model_file],
            symbols: vec![request_symbol],
            references: Vec::new(),
            imports: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("model batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 2,
            parsed_byte_count: 20,
            files: vec![service_file],
            symbols: Vec::new(),
            references: Vec::new(),
            imports: vec![service_import],
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("service batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "W3ConnectorSaveRequest", CodeQueryKind::Imports).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/relay_teams/connector/service.py");
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
}

#[tokio::test]
async fn checkpointed_batches_finalize_relative_python_import_edges() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:python-relative-imports";
    let session = session_for_scope(source_scope, 2);
    let model_file = file(
        source_scope,
        "model-file",
        "src/relay_teams/connector/w3_models.py",
        "python",
        CodeParseStatus::Parsed,
    );
    let service_file = file(
        source_scope,
        "service-file",
        "src/relay_teams/connector/service.py",
        "python",
        CodeParseStatus::Parsed,
    );
    let request_symbol = symbol(
        source_scope,
        "request-symbol",
        "model-file",
        "src/relay_teams/connector/w3_models.py",
        "W3ConnectorSaveRequest",
        "python",
    );
    let service_import = import(
        source_scope,
        "service-import",
        "service-file",
        "src/relay_teams/connector/service.py",
        "from .w3_models import W3ConnectorSaveRequest",
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
            files: vec![model_file],
            symbols: vec![request_symbol],
            references: Vec::new(),
            imports: Vec::new(),
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("model batch should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            batch_index: 2,
            parsed_byte_count: 20,
            files: vec![service_file],
            symbols: Vec::new(),
            references: Vec::new(),
            imports: vec![service_import],
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("service batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "W3ConnectorSaveRequest", CodeQueryKind::Imports).await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/relay_teams/connector/service.py");
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
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

fn file(
    source_scope: &str,
    file_id: &str,
    path: &str,
    language_id: &str,
    parse_status: CodeParseStatus,
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
        parse_status,
        degraded_reason: None,
    }
}

fn symbol(
    source_scope: &str,
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    language_id: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        name: name.to_owned(),
        qualified_name: format!("{}::{name}", path.replace('/', "::")),
        kind: "function".to_owned(),
        signature: format!("fn {name}()"),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 0, end: 8 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
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
            CodeRetrievalRequest::new(query, selector, kind, 5, FreshnessPolicy::AllowStale)
                .expect("request should validate"),
        )
        .await
        .expect("query should succeed")
}
