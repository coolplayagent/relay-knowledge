use std::collections::BTreeMap;

use crate::{
    domain::{
        CodeImportRecord, CodeIndexBatch, CodeIndexResourceBudget, CodeIndexSession,
        CodeParseStatus, CodeRepositoryRegistration, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeReferenceRecord,
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
            chunks: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("batch should persist");
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
