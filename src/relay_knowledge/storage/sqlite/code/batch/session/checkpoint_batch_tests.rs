//! Direct tests for checkpoint batch replay and path replacement invariants.

use super::tests::{batch, file, registered_store, search, session_for_scope, symbol};
use crate::{
    domain::{CodeIndexBatch, CodeParseStatus, CodeQueryKind},
    storage::{CodeIndexPublicationStore as _, RepositoryCatalogStore as _, StorageError},
};

#[tokio::test]
async fn checkpointed_batch_rejects_a_gap_before_staging_or_fact_writes() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:batch-gap";
    store
        .begin_code_index_session(session_for_scope(source_scope, 1))
        .await
        .expect("session should begin");
    let skipped = CodeIndexBatch {
        files: vec![file(
            source_scope,
            "skipped-file",
            "src/lib.rs",
            "rust",
            CodeParseStatus::Parsed,
        )],
        ..batch(source_scope, 2)
    };

    let error = store
        .apply_code_index_batch(skipped)
        .await
        .expect_err("a skipped durable batch index must fail closed");
    assert!(matches!(error, StorageError::Invariant(_)));
    let checkpoint = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");
    assert_eq!(checkpoint.batch_count, 0);
    assert_eq!(checkpoint.committed_file_count, 0);
    assert!(checkpoint.last_path.is_none());
    let scope = source_scope.to_owned();
    let (file_count, staging_count) = store
        .run(move |connection| {
            let file_count = connection.query_row(
                "SELECT COUNT(*) FROM code_repository_files WHERE source_scope = ?1",
                [&scope],
                |row| row.get::<_, usize>(0),
            )?;
            let staging_count = connection.query_row(
                "SELECT COUNT(*) FROM code_repository_index_batch_staging WHERE source_scope = ?1",
                [&scope],
                |row| row.get::<_, usize>(0),
            )?;
            Ok((file_count, staging_count))
        })
        .await
        .expect("scope rows should count");
    assert_eq!((file_count, staging_count), (0, 0));
}

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
    assert_eq!(first.committed_fact_row_count, 2);
    assert_eq!(resumed.committed_file_count, 1);
    assert_eq!(resumed.committed_symbol_count, 1);
    assert_eq!(resumed.committed_fact_row_count, 2);
    assert_eq!(resumed.batch_count, 1);
    assert_eq!(replayed.committed_file_count, 1);
    assert_eq!(replayed.committed_symbol_count, 1);
    assert_eq!(replayed.committed_fact_row_count, 2);
    assert_eq!(replayed.batch_count, 1);
    assert_eq!(status.indexed_file_count, 1);
    assert_eq!(status.symbol_count, 1);
}

#[tokio::test]
async fn upgraded_partial_checkpoint_keeps_the_missing_fact_proof_sticky() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:legacy-partial-fact-proof";
    let session = session_for_scope(source_scope, 2);
    let first = CodeIndexBatch {
        files: vec![file(
            source_scope,
            "legacy-file",
            "src/legacy.rs",
            "rust",
            CodeParseStatus::Parsed,
        )],
        ..batch(source_scope, 1)
    };
    let second = CodeIndexBatch {
        files: vec![file(
            source_scope,
            "new-file",
            "src/new.rs",
            "rust",
            CodeParseStatus::Parsed,
        )],
        ..batch(source_scope, 2)
    };

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(first)
        .await
        .expect("pre-upgrade batch should persist");
    let scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET committed_fact_row_count = 0
                 WHERE source_scope = ?1 AND batch_count = 1",
                [&scope],
            )?;
            Ok(())
        })
        .await
        .expect("legacy upgrade should leave the partial scope unproven");

    let continued = store
        .apply_code_index_batch(second)
        .await
        .expect("post-upgrade batch should still persist");
    assert_eq!(continued.batch_count, 2);
    assert_eq!(continued.committed_fact_row_count, 0);

    store
        .finalize_code_index_session(session)
        .await
        .expect("legacy partial session should complete through the full path");
    let completed = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");
    assert_eq!(completed.state, "completed");
    assert_eq!(completed.committed_fact_row_count, 0);
}

#[tokio::test]
async fn checkpointed_batch_replay_with_different_content_is_a_safe_noop() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:batch-replay-content";
    let original = CodeIndexBatch {
        files: vec![file(
            source_scope,
            "original-file",
            "src/original.rs",
            "rust",
            CodeParseStatus::Parsed,
        )],
        symbols: vec![symbol(
            source_scope,
            "original-symbol",
            "original-file",
            "src/original.rs",
            "original_handler",
            "rust",
        )],
        ..batch(source_scope, 1)
    };
    store
        .begin_code_index_session(session_for_scope(source_scope, 1))
        .await
        .expect("session should begin");
    let committed = store
        .apply_code_index_batch(original)
        .await
        .expect("original batch should persist");
    let conflicting_replay = CodeIndexBatch {
        files: vec![file(
            source_scope,
            "replacement-file",
            "src/replacement.rs",
            "rust",
            CodeParseStatus::Parsed,
        )],
        symbols: vec![symbol(
            source_scope,
            "replacement-symbol",
            "replacement-file",
            "src/replacement.rs",
            "replacement_handler",
            "rust",
        )],
        ..batch(source_scope, 1)
    };

    let replayed = store
        .apply_code_index_batch(conflicting_replay)
        .await
        .expect("already committed batch index should be a safe no-op");
    let scope = source_scope.to_owned();
    let (file_paths, symbol_names, staging_state) = store
        .run(move |connection| {
            let mut files = connection.prepare(
                "SELECT path FROM code_repository_files WHERE source_scope = ?1 ORDER BY path",
            )?;
            let file_paths = files
                .query_map([&scope], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut symbols = connection.prepare(
                "SELECT name FROM code_repository_symbols WHERE source_scope = ?1 ORDER BY name",
            )?;
            let symbol_names = symbols
                .query_map([&scope], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let staging_state = connection.query_row(
                "SELECT state FROM code_repository_index_batch_staging
                 WHERE source_scope = ?1 AND batch_index = 1",
                [&scope],
                |row| row.get::<_, String>(0),
            )?;
            Ok((file_paths, symbol_names, staging_state))
        })
        .await
        .expect("replayed scope should inspect");

    assert_eq!(replayed, committed);
    assert_eq!(file_paths, ["src/original.rs"]);
    assert_eq!(symbol_names, ["original_handler"]);
    assert_eq!(staging_state, "published");
}

#[tokio::test]
async fn older_duplicate_batch_cannot_move_checkpoint_last_path_backwards() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:batch-last-path-monotonic";
    let session = session_for_scope(source_scope, 2);
    let first = CodeIndexBatch {
        files: vec![file(
            source_scope,
            "first-file",
            "src/a.rs",
            "rust",
            CodeParseStatus::Parsed,
        )],
        ..batch(source_scope, 1)
    };
    let second = CodeIndexBatch {
        files: vec![file(
            source_scope,
            "second-file",
            "src/b.rs",
            "rust",
            CodeParseStatus::Parsed,
        )],
        ..batch(source_scope, 2)
    };

    store
        .begin_code_index_session(session)
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(first.clone())
        .await
        .expect("first batch should persist");
    store
        .apply_code_index_batch(second)
        .await
        .expect("second batch should persist");
    let replayed = store
        .apply_code_index_batch(first)
        .await
        .expect("older duplicate should remain replay-safe");

    assert_eq!(replayed.batch_count, 2);
    assert_eq!(replayed.committed_file_count, 2);
    assert_eq!(replayed.last_path.as_deref(), Some("src/b.rs"));
    assert_eq!(replayed.resolved_commit_sha, "commit");
    assert_eq!(replayed.tree_hash, "tree");
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
