//! Restart and resume query-index preparation policy contracts.

use crate::{
    domain::{CodeIndexBatch, CodeParseStatus},
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

use super::tests::{batch, file, registered_store, session_for_scope};

#[tokio::test]
async fn code_index_task_restart_prepares_only_empty_chunk_owner_indexes_even_for_one_path() {
    let store = registered_store().await;

    store
        .begin_code_index_session(session_for_scope("git_snapshot:single-batch-indexes", 1))
        .await
        .expect("fresh restart should begin");

    assert!(!query_index_exists(&store, "code_repository_symbols_lookup").await);
    assert!(
        !query_index_exists(&store, "code_repository_symbols_name_path_lookup").await,
        "Restart must not infer a single batch from path count and prebuild heavy indexes"
    );
    assert!(query_index_exists(&store, "code_repository_chunks_lookup").await);
    assert!(query_index_exists(&store, "code_repository_chunks_symbol_lookup").await);
}

#[tokio::test]
async fn restart_leaves_populated_missing_owner_to_durable_finalization() {
    let store = registered_store().await;
    store
        .run(|connection| {
            connection.execute(
                "INSERT INTO code_repository_search_metadata (
                    source_scope, document_kind, record_id, path, search_rowid
                 ) VALUES ('active-scope', 'symbol', 'active-symbol', 'src/lib.rs', 1)",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("populated query-index owner should insert");
    let source_scope = "git_snapshot:single-batch-populated-owner";
    let session = session_for_scope(source_scope, 1);

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("fresh restart should begin");
    assert!(
        !query_index_exists(&store, "code_repository_search_metadata_scope_path").await,
        "session begin must not synchronously build an index for a populated owner"
    );
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![file(
                source_scope,
                "single-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..batch(source_scope, 1)
        })
        .await
        .expect("single batch should persist");
    let advance = store
        .run(move |connection| super::finalization::advance_session(connection, session))
        .await
        .expect("durable finalization should build the missing descriptor");

    assert!(matches!(
        advance,
        super::finalization::CodeIndexFinalizationAdvance::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:build_query_indexes:v3:0"
    ));
    assert!(query_index_exists(&store, "code_repository_search_metadata_scope_path").await);
}

#[tokio::test]
async fn code_index_task_multi_batch_restart_prepares_empty_chunk_owner_and_resume_does_not_expand_the_plan()
 {
    let store = registered_store().await;
    let source_scope = "git_snapshot:multi-batch-indexes";
    let session = session_for_scope(source_scope, 2);

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("multi-batch session should begin");
    assert!(!query_index_exists(&store, "code_repository_symbols_lookup").await);
    assert!(
        !query_index_exists(&store, "code_repository_symbols_name_path_lookup").await,
        "populated-heavy symbol indexes must remain deferred"
    );
    assert!(query_index_exists(&store, "code_repository_chunks_lookup").await);
    assert!(query_index_exists(&store, "code_repository_chunks_symbol_lookup").await);
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![file(
                source_scope,
                "first-file",
                "src/first.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..batch(source_scope, 1)
        })
        .await
        .expect("first batch should persist with only the prepared chunk lookups");

    store
        .begin_code_index_session(session)
        .await
        .expect("multi-batch checkpoint should resume");
    assert!(!query_index_exists(&store, "code_repository_symbols_lookup").await);
    assert!(!query_index_exists(&store, "code_repository_symbols_name_path_lookup").await);
    assert!(query_index_exists(&store, "code_repository_chunks_lookup").await);
    assert!(query_index_exists(&store, "code_repository_chunks_symbol_lookup").await);
}

async fn query_index_exists(store: &SqliteGraphStore, name: &str) -> bool {
    let name = name.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1)",
                    [name],
                    |row| row.get(0),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("query-index state should load")
}
