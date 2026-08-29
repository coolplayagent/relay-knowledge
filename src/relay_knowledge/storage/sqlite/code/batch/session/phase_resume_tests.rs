//! Direct finalization-phase checkpoint resume invariants.

use super::{finalization_phase_pending, tests};
use crate::{
    domain::{
        CodeIndexBatch, CodeParseStatus, CodeQueryIndexRepairResumePhase,
        CodeRepositoryRegistration, code_query_index_repair, code_query_index_subphase,
    },
    storage::{
        CodeIndexPublicationStore as _, RepositoryCatalogStore as _, SqliteGraphStore, StorageError,
    },
};
use rusqlite::params;

#[test]
fn finalization_resume_runs_only_phases_after_the_durable_checkpoint() {
    let phases = super::finalize::phases::ORDERED_FINALIZATION_PHASES;

    for (completed_index, completed) in phases.iter().enumerate() {
        for (target_index, target) in phases.iter().enumerate() {
            assert_eq!(
                finalization_phase_pending(completed, target)
                    .expect("known finalization phases should compare"),
                target_index > completed_index,
                "completed={completed} target={target}"
            );
        }
    }
    assert!(
        finalization_phase_pending("indexing", phases[0])
            .expect("indexing should precede finalization")
    );
    assert!(
        !finalization_phase_pending("completed", phases[0])
            .expect("completed should follow finalization")
    );
}

#[tokio::test]
async fn code_index_task_v1_v2_and_v3_retired_prefix_policies_reach_the_finalizer() {
    let store = tests::registered_store().await;
    let source_scope = "git_snapshot:versioned-retired-prefix";
    let session = tests::session_for_scope(source_scope, 1);
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("single-batch session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![tests::file(
                source_scope,
                "versioned-prefix-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..tests::batch(source_scope, 1)
        })
        .await
        .expect("complete batch should persist");
    store
        .run(|connection| {
            connection.execute(
                "CREATE INDEX code_repository_search_metadata_scope_path
                 ON code_repository_search_metadata(source_scope, path)",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("legacy completed unit zero should have its exact physical index");

    for state in [
        "finalizing:build_query_indexes:v1:0",
        "finalizing:build_query_indexes:v1:1",
        "finalizing:build_query_indexes:v2:0",
        "finalizing:build_query_indexes:v2:1",
    ] {
        store
            .run(move |connection| {
                connection.execute(
                    "UPDATE code_repository_index_checkpoints
                     SET state = ?1
                     WHERE source_scope = ?2",
                    params![state, source_scope],
                )?;
                Ok(())
            })
            .await
            .expect("legacy cursor should persist");
        let error = store
            .run({
                let session = session.clone();
                move |connection| super::finalization::advance_session(connection, session)
            })
            .await
            .expect_err("a legacy cursor must retain its physical unit one");
        assert!(matches!(error, StorageError::Invariant(_)), "state={state}");
    }

    for (state, expected_next) in [
        (
            "finalizing:build_query_indexes:v1:1",
            "finalizing:build_query_indexes:v1:2",
        ),
        (
            "finalizing:build_query_indexes:v2:1",
            "finalizing:build_query_indexes:v2:2",
        ),
    ] {
        store
            .run(move |connection| {
                connection.execute_batch(
                    "CREATE INDEX IF NOT EXISTS code_repository_symbols_lookup
                         ON code_repository_symbols(source_scope, name, qualified_name, path);
                     DROP INDEX IF EXISTS code_repository_symbols_name_path_lookup;",
                )?;
                connection.execute(
                    "UPDATE code_repository_index_checkpoints
                     SET state = ?1
                     WHERE source_scope = ?2",
                    params![state, source_scope],
                )?;
                Ok(())
            })
            .await
            .expect("legacy cursor and exact retired prefix should persist");
        let legacy_session = session.clone();
        let advance = store
            .run(move |connection| super::finalization::advance_session(connection, legacy_session))
            .await
            .expect("legacy cursor should build its next missing stable unit");
        assert!(matches!(
            advance,
            super::finalization::CodeIndexFinalizationAdvance::Pending { checkpoint_state }
                if checkpoint_state == expected_next
        ));
        store
            .run(|connection| {
                connection.execute("DROP INDEX code_repository_symbols_lookup", [])?;
                Ok(())
            })
            .await
            .expect("retired legacy prefix should be removable by the test");
        let error = store
            .run({
                let session = session.clone();
                move |connection| super::finalization::advance_session(connection, session)
            })
            .await
            .expect_err("the advanced legacy cursor must keep requiring its retired prefix");
        assert!(matches!(error, StorageError::Invariant(_)), "state={state}");
    }

    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'finalizing:build_query_indexes:v3:1'
                 WHERE source_scope = ?1",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("current cursor should persist");
    let current_session = session.clone();
    let advance = store
        .run(move |connection| super::finalization::advance_session(connection, current_session))
        .await
        .expect("version-three cursor should accept the retired stable skip");
    assert!(matches!(
        advance,
        super::finalization::CodeIndexFinalizationAdvance::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:build_query_indexes:v3:3"
    ));

    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'finalizing:query_index_repair:v2:1:resume:0'
                 WHERE source_scope = ?1",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("legacy repair cursor should persist");
    let error = store
        .run({
            let session = session.clone();
            move |connection| super::finalization::advance_session(connection, session)
        })
        .await
        .expect_err("a version-two repair must retain its completed physical unit one");
    assert!(matches!(error, StorageError::Invariant(_)));

    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'finalizing:query_index_repair:v3:1:resume:0'
                 WHERE source_scope = ?1",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("current repair cursor should persist");
    let advance = store
        .run(move |connection| super::finalization::advance_session(connection, session))
        .await
        .expect("version-three repair should accept the retired stable skip");
    assert!(matches!(
        advance,
        super::finalization::CodeIndexFinalizationAdvance::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:query_index_repair:v3:4:resume:0"
    ));
    let retired_exists = store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT EXISTS (
                        SELECT 1 FROM sqlite_schema
                        WHERE type = 'index'
                          AND name = 'code_repository_symbols_lookup'
                    )",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("retired index state should load");
    assert!(!retired_exists);
}

#[tokio::test]
async fn code_index_task_v3_query_index_ordinal_is_durable_across_reopen() {
    let database_path = std::env::temp_dir().join(format!(
        "relay-knowledge-query-index-resume-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock should follow Unix epoch")
            .as_nanos()
    ));
    let store = SqliteGraphStore::open(&database_path).expect("file store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    let source_scope = "git_snapshot:query-index-subphase-resume";
    let session = tests::session_for_scope(source_scope, 1);
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![tests::file(
                source_scope,
                "query-index-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            symbols: vec![tests::symbol(
                source_scope,
                "query-index-symbol",
                "query-index-file",
                "src/lib.rs",
                "query_index_symbol",
                "rust",
            )],
            ..tests::batch(source_scope, 1)
        })
        .await
        .expect("complete batch should persist");
    store
        .run(|connection| {
            connection.execute_batch(
                "CREATE INDEX code_repository_search_metadata_scope_path
                     ON code_repository_search_metadata(source_scope, path);",
            )?;
            Ok(())
        })
        .await
        .expect("completed unit zero should have its exact physical index");

    let first_session = session.clone();
    let first = store
        .run(move |connection| super::finalization::advance_session(connection, first_session))
        .await
        .expect("one query-index unit should advance");
    let super::finalization::CodeIndexFinalizationAdvance::Pending { checkpoint_state } = first
    else {
        panic!("one missing descriptor should leave finalization pending");
    };
    assert_eq!(checkpoint_state, "finalizing:build_query_indexes:v3:2");
    assert_eq!(
        code_query_index_subphase(&checkpoint_state).map(|cursor| cursor.completed_unit),
        Some(2)
    );
    let only_first_missing_index_rebuilt = store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT
                        EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'index' AND name = 'code_repository_symbols_lookup'),
                        EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'index' AND name = 'code_repository_symbols_name_path_lookup'),
                        EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'index' AND name = 'code_repository_symbols_path_line_lookup')",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, bool>(0)?,
                            row.get::<_, bool>(1)?,
                            row.get::<_, bool>(2)?,
                        ))
                    },
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("rebuilt index state should load");
    assert_eq!(only_first_missing_index_rebuilt, (false, true, false));
    let durable_checkpoint = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should remain");
    assert_eq!(durable_checkpoint.state, checkpoint_state);
    drop(store);

    let store = SqliteGraphStore::open(&database_path).expect("file store should reopen");
    let resumed_session = session.clone();
    let resumed = store
        .run(move |connection| super::finalization::advance_session(connection, resumed_session))
        .await
        .expect("restart should resume after the durable descriptor");
    assert!(matches!(
        resumed,
        super::finalization::CodeIndexFinalizationAdvance::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:build_query_indexes:v3:3"
    ));
    let second_index_rebuilt = store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT EXISTS (
                        SELECT 1 FROM sqlite_schema
                        WHERE type = 'index'
                          AND name = 'code_repository_symbols_path_line_lookup'
                    )",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("second rebuilt index state should load");
    assert!(second_index_rebuilt);
    let stale_error = store
        .begin_code_index_session_at_checkpoint(session, Some(durable_checkpoint))
        .await
        .expect_err("the pre-resume exact token must be stale after the next unit commits");
    assert!(matches!(stale_error, StorageError::Invariant(_)));
    drop(store);
    let _ = std::fs::remove_file(&database_path);
    let _ = std::fs::remove_file(database_path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(database_path.with_extension("sqlite-shm"));
}

#[tokio::test]
async fn every_legacy_coarse_checkpoint_repairs_and_restores_across_reopen() {
    let database_path = std::env::temp_dir().join(format!(
        "relay-knowledge-coarse-query-index-repair-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock should follow Unix epoch")
            .as_nanos()
    ));
    let mut store = SqliteGraphStore::open(&database_path).expect("file store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    let source_scope = "git_snapshot:all-legacy-coarse-query-index-repair";
    let session = tests::session_for_scope(source_scope, 1);
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![tests::file(
                source_scope,
                "legacy-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..tests::batch(source_scope, 1)
        })
        .await
        .expect("complete batch should persist");
    store
        .run(|connection| {
            super::super::super::schema::ensure_code_query_indexes(connection)?;
            connection.execute("DROP INDEX code_repository_calls_caller_lookup", [])?;
            connection.execute(
                "ALTER TABLE code_repository_calls DROP COLUMN line_start",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("legacy unit 15 should remain structurally inapplicable");
    for (expected_code, phase) in super::finalize::phases::ORDERED_FINALIZATION_PHASES
        .iter()
        .enumerate()
    {
        let source_scope_for_legacy = source_scope.to_owned();
        let coarse_state = (*phase).to_owned();
        store
            .run(move |connection| {
                connection.execute(
                    "DROP INDEX code_repository_imports_scope_path_line_lookup",
                    [],
                )?;
                connection.execute(
                    "UPDATE code_repository_index_checkpoints SET state = ?1 WHERE source_scope = ?2",
                    params![coarse_state, source_scope_for_legacy],
                )?;
                Ok(())
            })
            .await
            .expect("legacy coarse checkpoint should be constructible");

        let first_session = session.clone();
        let first = store
            .run(move |connection| super::finalization::advance_session(connection, first_session))
            .await
            .expect("legacy coarse repair should create one unit");
        let super::finalization::CodeIndexFinalizationAdvance::Pending { checkpoint_state } = first
        else {
            panic!("legacy coarse repair should remain pending");
        };
        let repair = code_query_index_repair(&checkpoint_state)
            .expect("coarse repair should write a canonical durable token");
        assert_eq!(repair.completed_unit, 16);
        assert_eq!(repair.resume_phase as usize, expected_code);
        assert_eq!(repair.resume_phase.checkpoint_state(), *phase);

        let durable_checkpoint = store
            .code_index_checkpoint(source_scope.to_owned())
            .await
            .expect("repair checkpoint should load")
            .expect("repair checkpoint should remain");
        drop(store);
        store = SqliteGraphStore::open(&database_path).expect("file store should reopen");
        let resumed = store
            .begin_code_index_session_at_checkpoint(
                session.clone(),
                Some(durable_checkpoint.clone()),
            )
            .await
            .expect("repair checkpoint should resume without restart");
        assert_eq!(resumed, durable_checkpoint);

        let second_session = session.clone();
        let second = store
            .run(move |connection| super::finalization::advance_session(connection, second_session))
            .await
            .expect("complete repair plan should restore its original coarse state");
        assert!(matches!(
            second,
            super::finalization::CodeIndexFinalizationAdvance::Pending { checkpoint_state }
                if checkpoint_state == *phase
        ));
    }

    assert_eq!(
        CodeQueryIndexRepairResumePhase::ALL.len(),
        super::finalize::phases::ORDERED_FINALIZATION_PHASES.len()
    );
    drop(store);
    let _ = std::fs::remove_file(&database_path);
    let _ = std::fs::remove_file(database_path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(database_path.with_extension("sqlite-shm"));
}

#[tokio::test]
async fn completed_checkpoint_never_reopens_query_index_repair() {
    let store = tests::registered_store().await;
    let source_scope = "git_snapshot:completed-query-index-no-repair";
    let session = tests::session_for_scope(source_scope, 1);
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![tests::file(
                source_scope,
                "completed-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..tests::batch(source_scope, 1)
        })
        .await
        .expect("complete batch should persist");
    store
        .finalize_code_index_session(session.clone())
        .await
        .expect("session should publish before terminal compatibility is tested");
    store
        .run(|connection| {
            connection.execute(
                "DROP INDEX code_repository_imports_scope_path_line_lookup",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("terminal fixture should remove the appended index");

    let advance = store
        .run(move |connection| super::finalization::advance_session(connection, session))
        .await
        .expect("completed checkpoint should remain a read-only terminal observation");
    assert!(matches!(
        advance,
        super::finalization::CodeIndexFinalizationAdvance::Ready(_)
    ));
    let (checkpoint_state, appended_index_exists) = store
        .run(move |connection| {
            let state = connection.query_row(
                "SELECT state FROM code_repository_index_checkpoints WHERE source_scope = ?1",
                [source_scope],
                |row| row.get::<_, String>(0),
            )?;
            let exists = connection.query_row(
                "SELECT EXISTS (
                     SELECT 1 FROM sqlite_schema
                     WHERE type = 'index'
                       AND name = 'code_repository_imports_scope_path_line_lookup'
                 )",
                [],
                |row| row.get::<_, bool>(0),
            )?;
            Ok((state, exists))
        })
        .await
        .expect("terminal state should remain observable");
    assert_eq!(checkpoint_state, "completed");
    assert!(!appended_index_exists);
}

#[tokio::test]
async fn incomplete_indexing_checkpoint_cannot_enter_finalization() {
    let store = tests::registered_store().await;
    let source_scope = "git_snapshot:incomplete-finalization";
    let session = tests::session_for_scope(source_scope, 2);
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![tests::file(
                source_scope,
                "partial-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..tests::batch(source_scope, 1)
        })
        .await
        .expect("partial batch should persist");

    let error = store
        .run(move |connection| super::finalization::advance_session(connection, session))
        .await
        .expect_err("an incomplete prefix must not enter finalization");

    assert!(matches!(error, StorageError::Invariant(_)));
    assert_eq!(
        store
            .code_index_checkpoint(source_scope.to_owned())
            .await
            .expect("checkpoint should load")
            .expect("checkpoint should remain")
            .state,
        "indexing"
    );
}

#[tokio::test]
async fn begin_preserves_facts_and_checkpoint_for_every_finalizing_phase() {
    for (index, phase) in super::finalize::phases::ORDERED_FINALIZATION_PHASES
        .iter()
        .enumerate()
    {
        let store = tests::registered_store().await;
        let source_scope = format!("git_snapshot:phase-resume-{index}");
        let session = tests::session_for_scope(&source_scope, 1);
        store
            .begin_code_index_session(session.clone())
            .await
            .expect("session should begin");
        store
            .apply_code_index_batch(CodeIndexBatch {
                files: vec![tests::file(
                    &source_scope,
                    "phase-file",
                    "src/lib.rs",
                    "rust",
                    CodeParseStatus::Parsed,
                )],
                ..tests::batch(&source_scope, 1)
            })
            .await
            .expect("complete file prefix should persist");
        let scope = source_scope.clone();
        let durable_phase = (*phase).to_owned();
        let stored_phase = durable_phase.clone();
        store
            .run(move |connection| {
                connection.execute(
                    "UPDATE code_repository_index_checkpoints SET state = ?2 WHERE source_scope = ?1",
                    rusqlite::params![scope, stored_phase],
                )?;
                Ok(())
            })
            .await
            .expect("phase checkpoint should persist");

        let resumed = store
            .begin_code_index_session(session)
            .await
            .expect("known finalization phase should resume without restart");
        assert_eq!(resumed.state, durable_phase);
        assert_eq!(resumed.committed_file_count, 1);
        assert_eq!(resumed.batch_count, 1);
        assert_eq!(resumed.last_path.as_deref(), Some("src/lib.rs"));
        let scope = source_scope.clone();
        let file_count = store
            .run(move |connection| {
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM code_repository_files WHERE source_scope = ?1",
                        [scope],
                        |row| row.get::<_, usize>(0),
                    )
                    .map_err(crate::storage::StorageError::from)
            })
            .await
            .expect("phase facts should count");
        assert_eq!(file_count, 1, "phase {durable_phase} must preserve facts");
    }
}

#[tokio::test]
async fn corrupt_checkpoint_fails_before_repository_state_is_mutated() {
    let store = tests::registered_store().await;
    let source_scope = "git_snapshot:corrupt-resume";
    let session = tests::session_for_scope(source_scope, 1);
    let budget_json =
        serde_json::to_string(&session.resource_budget).expect("budget should serialize");
    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repositories SET state = 'registered', stale = 0 WHERE repository_id = 'repo'",
                [],
            )?;
            connection.execute(
                "INSERT INTO code_repository_index_checkpoints (
                     source_scope, repository_id, state, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, total_path_count,
                     parsed_file_count, committed_file_count, committed_symbol_count,
                     committed_reference_count, committed_chunk_count, batch_count, last_path,
                     resource_budget_json, updated_at_ms, error_message
                 ) VALUES (?1, 'repo', 'indexing', 'commit', 'tree', '[]', '[]',
                           1, 1, 0, 0, 0, 0, 0, NULL, ?2, 1, NULL)",
                rusqlite::params![source_scope, budget_json],
            )?;
            Ok(())
        })
        .await
        .expect("corrupt checkpoint fixture should persist");

    let error = store
        .begin_code_index_session(session)
        .await
        .expect_err("corrupt progress must fail before begin writes status");
    assert!(matches!(error, StorageError::Invariant(_)));
    let status = store
        .code_repository_status("repo".to_owned())
        .await
        .expect("repository status should load")
        .expect("repository should exist");
    assert_eq!(status.state, "registered");
    assert!(!status.stale);
    let checkpoint = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("corrupt checkpoint should remain observable");
    assert_eq!(checkpoint.parsed_file_count, 1);
    assert_eq!(checkpoint.committed_file_count, 0);
}

#[tokio::test]
async fn begin_checkpoint_cas_rejects_a_checkpoint_created_after_missing_preflight() {
    let store = tests::registered_store().await;
    let source_scope = "git_snapshot:checkpoint-cas-missing";
    let session = tests::session_for_scope(source_scope, 1);
    let expected_checkpoint = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("missing preflight should query");
    assert!(expected_checkpoint.is_none());
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("competing session should create the checkpoint");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![tests::file(
                source_scope,
                "race-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..tests::batch(source_scope, 1)
        })
        .await
        .expect("competing batch should persist");
    publish_unrelated_baseline(&store, "missing-cas-baseline").await;
    let status_before = store
        .code_repository_status("repo".to_owned())
        .await
        .expect("repository status should load")
        .expect("repository should exist");

    let error = store
        .begin_code_index_session_at_checkpoint(session, expected_checkpoint)
        .await
        .expect_err("missing expectation must reject a concurrently created checkpoint");

    assert!(matches!(error, StorageError::Invariant(_)));
    assert_repository_status_unchanged(&store, &status_before).await;
}

#[tokio::test]
async fn begin_checkpoint_cas_accepts_the_exact_validated_checkpoint() {
    let store = tests::registered_store().await;
    let source_scope = "git_snapshot:checkpoint-cas-exact";
    let session = tests::session_for_scope(source_scope, 2);
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    let expected_checkpoint = store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![tests::file(
                source_scope,
                "exact-cas-file",
                "src/a.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..tests::batch(source_scope, 1)
        })
        .await
        .expect("checkpoint should persist");

    let resumed = store
        .begin_code_index_session_at_checkpoint(session, Some(expected_checkpoint.clone()))
        .await
        .expect("unchanged checkpoint token should resume");

    assert_eq!(resumed, expected_checkpoint);
}

#[tokio::test]
async fn completed_content_equivalent_commit_restarts_from_zero_at_the_exact_checkpoint() {
    let store = tests::registered_store().await;
    let source_scope = "git_snapshot:completed-content-equivalent-restart";
    let session = tests::session_for_scope(source_scope, 1);
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("original session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![tests::file(
                source_scope,
                "retained-file",
                "src/lib.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..tests::batch(source_scope, 1)
        })
        .await
        .expect("original batch should persist");
    store
        .finalize_code_index_session(session.clone())
        .await
        .expect("original session should complete");
    publish_unrelated_baseline(&store, "content-equivalent-new-active").await;
    let expected = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("completed checkpoint should load")
        .expect("completed checkpoint should remain retained");
    assert_eq!(expected.state, "completed");
    let mut replacement = session;
    replacement.resolved_commit_sha = "commit-alias-with-the-same-tree".to_owned();

    let restarted = store
        .begin_code_index_session_at_checkpoint(replacement.clone(), Some(expected))
        .await
        .expect("the exact completed content identity should restart for the new commit");

    assert_eq!(restarted.state, "indexing");
    assert_eq!(
        restarted.resolved_commit_sha,
        replacement.resolved_commit_sha
    );
    assert_eq!(restarted.parsed_file_count, 0);
    assert_eq!(restarted.committed_file_count, 0);
    assert_eq!(restarted.committed_symbol_count, 0);
    assert_eq!(restarted.committed_reference_count, 0);
    assert_eq!(restarted.committed_chunk_count, 0);
    assert_eq!(restarted.batch_count, 0);
    assert!(restarted.last_path.is_none());
    let scope = source_scope.to_owned();
    let retained_file_count = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM code_repository_files WHERE source_scope = ?1",
                    [scope],
                    |row| row.get::<_, usize>(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("restarted fact count should load");
    assert_eq!(retained_file_count, 0);
}

#[tokio::test]
async fn partial_content_equivalent_commit_mismatch_remains_an_invariant() {
    let store = tests::registered_store().await;
    let source_scope = "git_snapshot:partial-commit-mismatch";
    let session = tests::session_for_scope(source_scope, 2);
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("partial session should begin");
    let expected = store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![tests::file(
                source_scope,
                "partial-file",
                "src/a.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..tests::batch(source_scope, 1)
        })
        .await
        .expect("partial checkpoint should persist");
    let mut mismatched = session;
    mismatched.resolved_commit_sha = "different-in-progress-commit".to_owned();

    let error = store
        .begin_code_index_session_at_checkpoint(mismatched, Some(expected.clone()))
        .await
        .expect_err("partial progress from another commit must not restart");

    assert!(matches!(error, StorageError::Invariant(_)));
    let unchanged = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("partial checkpoint should load")
        .expect("partial checkpoint should remain durable");
    assert_eq!(unchanged, expected);
}

#[tokio::test]
async fn begin_checkpoint_cas_rejects_progress_changed_after_preflight() {
    let store = tests::registered_store().await;
    let source_scope = "git_snapshot:checkpoint-cas-progress";
    let session = tests::session_for_scope(source_scope, 2);
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    let expected_checkpoint = store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![tests::file(
                source_scope,
                "first-race-file",
                "src/a.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..tests::batch(source_scope, 1)
        })
        .await
        .expect("preflight checkpoint should persist");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![tests::file(
                source_scope,
                "second-race-file",
                "src/b.rs",
                "rust",
                CodeParseStatus::Parsed,
            )],
            ..tests::batch(source_scope, 2)
        })
        .await
        .expect("competing progress should persist");
    publish_unrelated_baseline(&store, "progress-cas-baseline").await;
    let status_before = store
        .code_repository_status("repo".to_owned())
        .await
        .expect("repository status should load")
        .expect("repository should exist");

    let error = store
        .begin_code_index_session_at_checkpoint(session, Some(expected_checkpoint))
        .await
        .expect_err("stale checkpoint token must fail before begin mutations");

    assert!(matches!(error, StorageError::Invariant(_)));
    assert_repository_status_unchanged(&store, &status_before).await;
}

async fn publish_unrelated_baseline(store: &crate::storage::SqliteGraphStore, identity: &str) {
    let mut session = tests::session_for_scope(&format!("git_snapshot:{identity}"), 0);
    session.resolved_commit_sha = format!("commit-{identity}");
    session.tree_hash = format!("tree-{identity}");
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("baseline session should begin");
    store
        .finalize_code_index_session(session)
        .await
        .expect("baseline session should publish");
}

async fn assert_repository_status_unchanged(
    store: &crate::storage::SqliteGraphStore,
    expected: &crate::domain::CodeRepositoryStatus,
) {
    let actual = store
        .code_repository_status(expected.repository_id.clone())
        .await
        .expect("repository status should load")
        .expect("repository should exist");
    assert_eq!(actual.state, expected.state);
    assert_eq!(actual.stale, expected.stale);
    assert_eq!(actual.last_indexed_scope_id, expected.last_indexed_scope_id);
    assert_eq!(actual.last_indexed_commit, expected.last_indexed_commit);
    assert_eq!(actual.tree_hash, expected.tree_hash);
}
