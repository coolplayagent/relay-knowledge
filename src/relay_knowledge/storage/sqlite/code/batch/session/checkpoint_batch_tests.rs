//! Direct tests for checkpoint batch replay and path replacement invariants.

use super::tests::{batch, file, registered_store, search, session_for_scope, symbol};
use crate::{
    domain::{CodeIndexBatch, CodeParseStatus, CodeQueryKind},
    storage::CodeRepositoryStore,
};

#[tokio::test]
async fn checkpointed_batch_replay_keeps_progress_counts_stable() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:batch-replay";
    let session = session_for_scope(source_scope, 1);
    let indexed_file = file(
        source_scope,
        "replayed-file",
        "src/lib.rs",
        "rust",
        CodeParseStatus::Parsed,
    );
    let indexed_symbol = symbol(
        source_scope,
        "replayed-symbol",
        "replayed-file",
        "src/lib.rs",
        "run",
        "rust",
    );
    let batch = CodeIndexBatch {
        files: vec![indexed_file],
        symbols: vec![indexed_symbol],
        ..batch(source_scope, 1)
    };

    store
        .begin_code_index_session(session)
        .await
        .expect("session should begin");
    let first = store
        .apply_code_index_batch(batch.clone())
        .await
        .expect("first batch should persist");
    let resumed = store
        .begin_code_index_session(session_for_scope(source_scope, 1))
        .await
        .expect("restarted session should retain checkpoint progress");
    let replayed = store
        .apply_code_index_batch(batch)
        .await
        .expect("batch replay should remain idempotent for progress");
    let status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("status should load")
        .expect("status should exist");

    assert_eq!(first.committed_file_count, 1);
    assert_eq!(first.committed_symbol_count, 1);
    assert_eq!(resumed.committed_file_count, 1);
    assert_eq!(resumed.committed_symbol_count, 1);
    assert_eq!(resumed.batch_count, 1);
    assert_eq!(replayed.committed_file_count, 1);
    assert_eq!(replayed.committed_symbol_count, 1);
    assert_eq!(replayed.batch_count, 1);
    assert_eq!(status.indexed_file_count, 1);
    assert_eq!(status.symbol_count, 1);
}

#[tokio::test]
async fn new_checkpoint_batch_replaces_colliding_path_rows() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:batch-path-collision";
    let session = session_for_scope(source_scope, 2);
    let path = "src/lib.rs";
    let batch = |batch_index, file_id: &str, symbol_id: &str, name: &str| CodeIndexBatch {
        files: vec![file(
            source_scope,
            file_id,
            path,
            "rust",
            CodeParseStatus::Parsed,
        )],
        symbols: vec![symbol(source_scope, symbol_id, file_id, path, name, "rust")],
        ..batch(source_scope, batch_index)
    };

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(batch(1, "first-file", "legacy-symbol", "legacy_handler"))
        .await
        .expect("first batch should persist");
    store
        .apply_code_index_batch(batch(2, "second-file", "current-symbol", "current_handler"))
        .await
        .expect("colliding new batch should replace path rows");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let old_hits = search(&store, "legacy_handler", CodeQueryKind::Symbol).await;
    let new_hits = search(&store, "current_handler", CodeQueryKind::Symbol).await;

    assert!(old_hits.is_empty());
    assert_eq!(new_hits.len(), 1);
    assert_eq!(new_hits[0].path, path);
}
