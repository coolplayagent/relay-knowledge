//! Attempt-fenced checkpoint publication tests spanning code and software facts.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    domain::{
        CodeIncrementalSummaryReceipt, CodeIndexMode, CodeIndexPublicationFence,
        CodeIndexResourceBudget, CodeIndexSnapshot, CodeIndexTaskState, CodeParseStatus,
    },
    storage::{CodeIndexTaskClaimRequest, CodeIndexTaskSeed, CodeRepositoryStore},
};

use super::tests::{batch, file, reference, registered_store, session_for_scope};

const SOURCE_SCOPE: &str = "git_snapshot:fenced-full-publication";
const DIRECT_FINALIZE_SCOPE: &str = "git_snapshot:direct-finalize-identity";
const LEASE_OWNER: &str = "publication-barrier-worker";

#[tokio::test]
async fn active_scope_reference_search_rebuild_rejects_nonempty_owner_without_mutation() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:active-reference-search";
    let budget = CodeIndexResourceBudget::new(1, 1024 * 1024, 8).expect("budget should build");
    let (session, fence) = begin_fenced_session(
        &store,
        source_scope,
        "active-reference-search",
        "active-reference-worker",
        budget,
    )
    .await;
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch_with_fence(
            crate::domain::CodeIndexBatch {
                files: vec![file(
                    source_scope,
                    "file-1",
                    "src/lib.rs",
                    "rust",
                    CodeParseStatus::Parsed,
                )],
                references: vec![
                    reference(source_scope, "reference:1", "file-1", "src/lib.rs", "first"),
                    reference(
                        source_scope,
                        "reference:2",
                        "file-1",
                        "src/lib.rs",
                        "second",
                    ),
                ],
                ..batch(source_scope, 1)
            },
            fence.clone(),
        )
        .await
        .expect("facts should persist");
    store
        .run(move |connection| {
            super::super::super::schema::ensure_code_query_indexes(connection)?;
            connection.execute(
                "INSERT INTO code_repository_search (
                     source_scope, document_kind, record_id, path, language_id, content
                 ) VALUES (?1, 'reference', 'stale', 'src/old.rs', 'rust', 'stale')",
                [source_scope],
            )?;
            let stale_rowid = connection.last_insert_rowid();
            connection.execute(
                "INSERT INTO code_repository_search_metadata (
                     source_scope, document_kind, record_id, path, search_rowid
                 ) VALUES (?1, 'reference', 'stale', 'src/old.rs', ?2)",
                rusqlite::params![source_scope, stale_rowid],
            )?;
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'finalizing:refresh_dependencies'
                 WHERE source_scope = ?1",
                [source_scope],
            )?;
            connection.execute(
                "INSERT INTO code_repository_scopes (
                     source_scope, repository_id, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, indexed_file_count,
                     symbol_count, reference_count, chunk_count, stale, degraded_reason
                 ) VALUES (?1, 'repo', 'commit', 'tree', '[]', '[]', 1, 0, 2, 0, 0, NULL)",
                [source_scope],
            )?;
            connection.execute(
                "UPDATE code_repositories SET last_indexed_scope_id = ?1 WHERE repository_id = 'repo'",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("active fixture should persist");

    let error = store
        .advance_code_index_session_with_fence(session, fence)
        .await
        .expect_err("an active scope with existing owners requires durable staged cleanup");
    assert!(matches!(
        error,
        crate::storage::StorageError::CapacityExceeded(_)
    ));
    let counts = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                     (SELECT COUNT(*) FROM code_repository_search
                      WHERE source_scope = ?1 AND document_kind = 'reference'),
                     (SELECT COUNT(*) FROM code_repository_search
                      WHERE source_scope = ?1 AND record_id = 'stale'),
                     (SELECT COUNT(*) FROM code_repository_reference_search_progress
                      WHERE source_scope = ?1)",
                    [source_scope],
                    |row| {
                        Ok((
                            row.get::<_, usize>(0)?,
                            row.get::<_, usize>(1)?,
                            row.get::<_, usize>(2)?,
                        ))
                    },
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("atomic rebuild counts should load");
    assert_eq!(counts, (1, 1, 0));
}

#[tokio::test]
async fn staged_reference_search_advances_set_based_pages_without_becoming_queryable() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:staged-reference-pages";
    let budget = CodeIndexResourceBudget::new(1, 1024 * 1024, 8).expect("budget should build");
    let (session, fence) = begin_fenced_session(
        &store,
        source_scope,
        "staged-reference-pages",
        "staged-reference-worker",
        budget,
    )
    .await;
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch_with_fence(
            crate::domain::CodeIndexBatch {
                files: vec![file(
                    source_scope,
                    "file-1",
                    "src/lib.rs",
                    "rust",
                    CodeParseStatus::Parsed,
                )],
                references: (1..=5)
                    .map(|ordinal| {
                        reference(
                            source_scope,
                            &format!("reference:{ordinal}"),
                            "file-1",
                            "src/lib.rs",
                            &format!("name{ordinal}"),
                        )
                    })
                    .collect(),
                ..batch(source_scope, 1)
            },
            fence.clone(),
        )
        .await
        .expect("facts should persist");
    store
        .run(move |connection| {
            super::super::super::schema::ensure_code_query_indexes(connection)?;
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'finalizing:refresh_dependencies'
                 WHERE source_scope = ?1",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("coarse checkpoint should persist");

    let mut states = Vec::new();
    for _ in 0..7 {
        let step = store
            .advance_code_index_session_with_fence(session.clone(), fence.clone())
            .await
            .expect("one staged page should advance");
        let crate::storage::CodeIndexFinalizationStep::Pending { checkpoint_state } = step else {
            panic!("reference-search page must remain pending");
        };
        states.push(checkpoint_state);
    }
    store
        .run(|connection| {
            connection.execute(
                "DROP INDEX code_repository_imports_scope_path_line_lookup",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("future appended query descriptor should become missing");
    for _ in 0..5 {
        let step = store
            .advance_code_index_session_with_fence(session.clone(), fence.clone())
            .await
            .expect("repair or staged page should advance");
        let crate::storage::CodeIndexFinalizationStep::Pending { checkpoint_state } = step else {
            panic!("reference-search repair must remain pending");
        };
        states.push(checkpoint_state);
    }
    assert_eq!(
        states,
        [
            "finalizing:rebuild_reference_search:v2:cleanup:0",
            "finalizing:rebuild_reference_search:v2:discover:0",
            "finalizing:rebuild_reference_search:v2:discover:1",
            "finalizing:rebuild_reference_search:v2:discover:2",
            "finalizing:rebuild_reference_search:v2:discover:3",
            "finalizing:rebuild_reference_search:v2:build:0",
            "finalizing:rebuild_reference_search:v2:build:1",
            "finalizing:query_index_repair:v3:16:resume:reference_search:v2:build:1",
            "finalizing:rebuild_reference_search:v2:build:1",
            "finalizing:rebuild_reference_search:v2:build:2",
            "finalizing:rebuild_reference_search:v2:build:3",
            "finalizing:rebuild_reference_search",
        ]
    );
    let persisted = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                         (SELECT COUNT(*) FROM code_repository_search
                          WHERE source_scope = ?1 AND document_kind = 'reference'),
                         (SELECT COUNT(*) FROM code_repository_search_metadata
                          WHERE source_scope = ?1 AND document_kind = 'reference'),
                         (SELECT COUNT(*) FROM code_repository_reference_search_progress
                          WHERE source_scope = ?1),
                         (SELECT COUNT(*) FROM code_repository_scopes
                          WHERE source_scope = ?1)",
                    [source_scope],
                    |row| {
                        Ok((
                            row.get::<_, usize>(0)?,
                            row.get::<_, usize>(1)?,
                            row.get::<_, usize>(2)?,
                            row.get::<_, usize>(3)?,
                        ))
                    },
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("staged counts should load");
    assert_eq!(persisted, (5, 5, 0, 0));
}

#[tokio::test]
async fn staged_reference_search_page_rejects_a_stale_fence_without_partial_mutation() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:stale-reference-page-fence";
    let budget = CodeIndexResourceBudget::new(1, 1024 * 1024, 5).expect("budget should build");
    let (session, fence) = begin_fenced_session(
        &store,
        source_scope,
        "stale-reference-page-fence",
        "stale-reference-page-worker",
        budget,
    )
    .await;
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch_with_fence(
            crate::domain::CodeIndexBatch {
                files: vec![file(
                    source_scope,
                    "file-1",
                    "src/lib.rs",
                    "rust",
                    CodeParseStatus::Parsed,
                )],
                references: vec![
                    reference(source_scope, "reference:1", "file-1", "src/lib.rs", "first"),
                    reference(
                        source_scope,
                        "reference:2",
                        "file-1",
                        "src/lib.rs",
                        "second",
                    ),
                ],
                ..batch(source_scope, 1)
            },
            fence.clone(),
        )
        .await
        .expect("facts should persist");
    store
        .run(move |connection| {
            super::super::super::schema::ensure_code_query_indexes(connection)?;
            connection.execute(
                "INSERT INTO code_repository_search (
                     source_scope, document_kind, record_id, path, language_id, content
                 ) VALUES (?1, 'reference', 'stale', 'src/old.rs', 'rust', 'stale')",
                [source_scope],
            )?;
            let stale_rowid = connection.last_insert_rowid();
            connection.execute(
                "INSERT INTO code_repository_search_metadata (
                     source_scope, document_kind, record_id, path, search_rowid
                 ) VALUES (?1, 'reference', 'stale', 'src/old.rs', ?2)",
                rusqlite::params![source_scope, stale_rowid],
            )?;
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'finalizing:refresh_dependencies'
                 WHERE source_scope = ?1",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("staged fixture should persist");

    let initialized = store
        .advance_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("staged progress should initialize");
    assert!(matches!(
        initialized,
        crate::storage::CodeIndexFinalizationStep::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:rebuild_reference_search:v2:cleanup:0"
    ));
    let task_id = fence.task_id.clone();
    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repository_index_tasks SET lease_expires_at_ms = 0
                 WHERE task_id = ?1",
                [task_id],
            )?;
            Ok(())
        })
        .await
        .expect("lease should expire");
    let error = store
        .advance_code_index_session_with_fence(session, fence)
        .await
        .expect_err("stale fence must reject the cleanup page");
    assert!(matches!(
        error,
        crate::storage::StorageError::InvalidInput(_)
    ));
    let persisted = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT checkpoint.state, progress.completed_page_ordinal,
                            (SELECT COUNT(*) FROM code_repository_search
                             WHERE source_scope = ?1 AND record_id = 'stale')
                     FROM code_repository_index_checkpoints checkpoint
                     JOIN code_repository_reference_search_progress progress
                       ON progress.source_scope = checkpoint.source_scope
                     WHERE checkpoint.source_scope = ?1",
                    [source_scope],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, usize>(1)?,
                            row.get::<_, usize>(2)?,
                        ))
                    },
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("unchanged staged progress should load");
    assert_eq!(
        persisted,
        (
            "finalizing:rebuild_reference_search:v2:cleanup:0".to_owned(),
            0,
            1,
        )
    );
}

#[tokio::test]
async fn fenced_snapshot_and_session_cannot_rewrite_an_active_scope() {
    let store = registered_store().await;
    let now_ms = now_millis();
    let queued = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: "repo".to_owned(),
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "commit".to_owned(),
            tree_hash: "tree".to_owned(),
            source_scope: SOURCE_SCOPE.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::Full,
            input_fingerprint: "active-scope-rebuild-guard".to_owned(),
            resource_budget: Default::default(),
            payload_json: "{}".to_owned(),
            now_ms,
        })
        .await
        .expect("task should queue");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: LEASE_OWNER.to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("task should claim")
        .expect("queued task should be claimable");
    let fence = CodeIndexPublicationFence {
        repository_id: running.repository_id,
        task_id: running.task_id,
        lease_owner: LEASE_OWNER.to_owned(),
        attempt_count: running.attempt_count,
        generation: running.publication_generation,
    };
    store
        .run(|connection| {
            connection.execute(
                "INSERT INTO code_repository_scopes (
                    source_scope, repository_id, resolved_commit_sha, tree_hash,
                    path_filters_json, language_filters_json, indexed_file_count,
                    symbol_count, reference_count, chunk_count, stale, degraded_reason
                 ) VALUES (?1, 'repo', 'commit', 'tree', '[]', '[]', 7, 0, 0, 0, 0, NULL)",
                [SOURCE_SCOPE],
            )?;
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = ?1, last_indexed_commit = 'commit',
                     tree_hash = 'tree', state = 'indexing', stale = 1
                 WHERE repository_id = 'repo'",
                [SOURCE_SCOPE],
            )?;
            Ok(())
        })
        .await
        .expect("active scope fixture should persist without software rows");

    let session_error = store
        .begin_code_index_session_with_fence(session_for_scope(SOURCE_SCOPE, 0), fence.clone())
        .await
        .expect_err("checkpoint startup must not delete an active target scope");
    assert!(
        session_error.to_string().contains("already-active"),
        "unexpected session guard error: {session_error}"
    );
    let snapshot_error = store
        .apply_code_index_snapshot_with_fence(
            CodeIndexSnapshot {
                repository_id: "repo".to_owned(),
                source_scope: SOURCE_SCOPE.to_owned(),
                base_resolved_commit_sha: None,
                resolved_commit_sha: "commit".to_owned(),
                tree_hash: "tree".to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
                full_replace: true,
                changed_path_count: 0,
                skipped_unchanged_count: 0,
                deleted_paths: Vec::new(),
                tombstones: Vec::new(),
                files: Vec::new(),
                symbols: Vec::new(),
                references: Vec::new(),
                imports: Vec::new(),
                calls: Vec::new(),
                dependencies: Vec::new(),
                feature_flags: Vec::new(),
                routes: Vec::new(),
                chunks: Vec::new(),
                workspaces: Vec::new(),
                diagnostics: Vec::new(),
            },
            fence,
        )
        .await
        .expect_err("snapshot apply must not replace an active target scope");
    assert!(
        snapshot_error.to_string().contains("already-active"),
        "unexpected snapshot guard error: {snapshot_error}"
    );

    let state = store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT repository.last_indexed_scope_id, repository.state, repository.stale,
                        scope.stale,
                        (SELECT COUNT(*) FROM code_repository_index_checkpoints
                         WHERE source_scope = ?1)
                 FROM code_repositories repository
                 JOIN code_repository_scopes scope
                   ON scope.source_scope = repository.last_indexed_scope_id
                 WHERE repository.repository_id = 'repo'",
                    [SOURCE_SCOPE],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, bool>(2)?,
                            row.get::<_, bool>(3)?,
                            row.get::<_, usize>(4)?,
                        ))
                    },
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("active scope state should remain readable");
    assert_eq!(
        state,
        (
            SOURCE_SCOPE.to_owned(),
            "indexing".to_owned(),
            true,
            false,
            0
        )
    );
}

#[tokio::test]
async fn fenced_full_checkpoint_waits_for_software_projection_before_becoming_fresh() {
    let store = registered_store().await;
    let now_ms = now_millis();
    let queued = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: "repo".to_owned(),
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "commit".to_owned(),
            tree_hash: "tree".to_owned(),
            source_scope: SOURCE_SCOPE.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::Full,
            input_fingerprint: "fenced-full-publication".to_owned(),
            resource_budget: Default::default(),
            payload_json: "{}".to_owned(),
            now_ms,
        })
        .await
        .expect("full task should queue");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: LEASE_OWNER.to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("full task should claim")
        .expect("queued task should be claimable");
    let fence = CodeIndexPublicationFence {
        repository_id: running.repository_id.clone(),
        task_id: running.task_id.clone(),
        lease_owner: LEASE_OWNER.to_owned(),
        attempt_count: running.attempt_count,
        generation: running.publication_generation,
    };
    let session = session_for_scope(SOURCE_SCOPE, 0);
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced full session should begin");
    store
        .finalize_code_index_session_with_fence(session, fence.clone())
        .await
        .expect("fenced code facts should stage");
    let staged_checkpoint = store
        .code_index_checkpoint(SOURCE_SCOPE.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");
    assert_eq!(staged_checkpoint.state, "finalizing:software_projection");
    let staged_status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("repository status should load")
        .expect("repository should exist");
    assert_eq!(staged_status.state, "indexing");
    assert!(staged_status.stale);
    let projection = store
        .refresh_software_global_projection_with_fence(SOURCE_SCOPE.to_owned(), fence)
        .await
        .expect("software facts should complete fenced publication");
    assert!(!projection.status.stale);
    let completed_checkpoint = store
        .code_index_checkpoint(SOURCE_SCOPE.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");
    assert_eq!(completed_checkpoint.state, "completed");
    let published_status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("repository status should load")
        .expect("repository should exist");
    assert_eq!(published_status.state, "fresh");
    assert!(!published_status.stale);
    assert_eq!(
        published_status.last_indexed_scope_id.as_deref(),
        Some(SOURCE_SCOPE)
    );
    let active = store
        .active_code_index_task("repo".to_owned())
        .await
        .expect("active task should load")
        .expect("worker completes the task after the publication response");
    assert_eq!(active.state, CodeIndexTaskState::Running);
}

#[tokio::test]
async fn publication_checkpoint_resume_preserves_facts_and_skips_finalize_phases() {
    let store = registered_store().await;
    let (session, fence) = staged_publication_session(&store, "resume-worker").await;
    let resumed = store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("publication checkpoint should resume without deleting its scope");
    assert_eq!(resumed.state, "finalizing:software_projection");
    let summary = store
        .finalize_code_index_session_with_fence(session, fence)
        .await
        .expect("publication resume should rebuild only the summary");

    assert_eq!(summary.source_scope, SOURCE_SCOPE);
    let checkpoint = store
        .code_index_checkpoint(SOURCE_SCOPE.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should remain durable");
    assert_eq!(checkpoint.state, "finalizing:software_projection");
}

#[tokio::test]
async fn completed_same_task_incremental_receipt_resumes_exact_summary_for_projection_repair() {
    let store = registered_store().await;
    let (mut session, fence) = staged_publication_session(&store, "same-task-repair-worker").await;
    store
        .refresh_software_global_projection_with_fence(SOURCE_SCOPE.to_owned(), fence.clone())
        .await
        .expect("software publication should complete");
    let receipt = terminal_receipt(&fence.task_id);
    install_terminal_receipt(&store, &receipt).await;
    session.base_resolved_commit_sha = Some(receipt.base_resolved_commit_sha.clone());

    let advance = store
        .advance_code_index_session_with_fence(session, fence)
        .await
        .expect("the same task should recover its completed receipt");
    let crate::storage::CodeIndexFinalizationStep::Ready(summary) = advance else {
        panic!("a completed checkpoint should be ready");
    };

    assert_eq!(summary.base_resolved_commit_sha.as_deref(), Some("base"));
    assert_eq!(summary.changed_path_count, 3);
    assert_eq!(summary.skipped_unchanged_count, 7);
    assert_eq!(summary.progress.batch_count, 1);
    let checkpoint = store
        .code_index_checkpoint(SOURCE_SCOPE.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");
    assert_eq!(checkpoint.incremental_summary, Some(receipt));
}

#[tokio::test]
async fn completed_different_task_receipt_is_cleared_before_generic_terminal_repair() {
    let store = registered_store().await;
    let (session, fence) = staged_publication_session(&store, "new-task-repair-worker").await;
    store
        .refresh_software_global_projection_with_fence(SOURCE_SCOPE.to_owned(), fence.clone())
        .await
        .expect("software publication should complete");
    install_terminal_receipt(&store, &terminal_receipt("previous-task")).await;

    let mut forged_incremental_session = session.clone();
    forged_incremental_session.base_resolved_commit_sha = Some("forged-base".to_owned());
    let error = store
        .advance_code_index_session_with_fence(forged_incremental_session, fence.clone())
        .await
        .expect_err("another task's receipt must not become a forged incremental summary");
    assert!(
        error
            .to_string()
            .contains("only to a generic repair session")
    );
    let retained = store
        .code_index_checkpoint(SOURCE_SCOPE.to_owned())
        .await
        .expect("checkpoint should load after rejected transfer")
        .expect("checkpoint should exist after rejected transfer");
    assert!(retained.incremental_summary.is_some());

    let advance = store
        .advance_code_index_session_with_fence(session, fence)
        .await
        .expect("a terminal repair may transfer receipt ownership");
    let crate::storage::CodeIndexFinalizationStep::Ready(summary) = advance else {
        panic!("a completed checkpoint should be ready");
    };

    assert_eq!(summary.base_resolved_commit_sha, None);
    assert_eq!(summary.changed_path_count, 0);
    assert_eq!(summary.progress.batch_count, 0);
    let checkpoint = store
        .code_index_checkpoint(SOURCE_SCOPE.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");
    assert!(checkpoint.incremental_summary.is_none());
}

#[tokio::test]
async fn publication_checkpoint_resume_rejects_identity_mismatch() {
    let store = registered_store().await;
    let (mut session, fence) = staged_publication_session(&store, "identity-worker").await;
    session.tree_hash = "different-tree".to_owned();

    let error = store
        .begin_code_index_session_with_fence(session, fence)
        .await
        .expect_err("publication resume must match the durable checkpoint identity");

    assert!(error.to_string().contains("checkpoint identity"));
    let checkpoint = store
        .code_index_checkpoint(SOURCE_SCOPE.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("mismatched resume must not delete the checkpoint");
    assert_eq!(checkpoint.state, "finalizing:software_projection");
}

#[tokio::test]
async fn direct_finalize_rejects_commit_mismatch_before_phases_and_completed_shortcut() {
    let store = registered_store().await;
    let session = session_for_scope(DIRECT_FINALIZE_SCOPE, 0);
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("direct finalization checkpoint should begin");

    let mut mismatched = session.clone();
    mismatched.resolved_commit_sha = "different-commit".to_owned();
    let indexing_error = store
        .finalize_code_index_session(mismatched.clone())
        .await
        .expect_err("direct finalization must validate identity before running phases");
    assert!(indexing_error.to_string().contains("checkpoint identity"));
    let indexing_checkpoint = store
        .code_index_checkpoint(DIRECT_FINALIZE_SCOPE.to_owned())
        .await
        .expect("indexing checkpoint should load")
        .expect("indexing checkpoint should remain durable");
    assert_eq!(indexing_checkpoint.state, "indexing");
    let unpublished_scope_count = store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM code_repository_scopes WHERE source_scope = ?1",
                    [DIRECT_FINALIZE_SCOPE],
                    |row| row.get::<_, usize>(0),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("unpublished scope count should load");
    assert_eq!(unpublished_scope_count, 0);

    store
        .finalize_code_index_session(session)
        .await
        .expect("matching direct finalization should publish");
    let completed_error = store
        .finalize_code_index_session(mismatched)
        .await
        .expect_err("completed checkpoint must not accept a same-tree commit alias directly");
    assert!(completed_error.to_string().contains("checkpoint identity"));

    let persisted = store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT checkpoint.state, checkpoint.resolved_commit_sha,
                            scope.resolved_commit_sha, repository.last_indexed_scope_id,
                            repository.last_indexed_commit
                     FROM code_repository_index_checkpoints checkpoint
                     JOIN code_repository_scopes scope
                       ON scope.source_scope = checkpoint.source_scope
                     JOIN code_repositories repository
                       ON repository.repository_id = checkpoint.repository_id
                     WHERE checkpoint.source_scope = ?1",
                    [DIRECT_FINALIZE_SCOPE],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("completed publication identity should load");
    assert_eq!(
        persisted,
        (
            "completed".to_owned(),
            "commit".to_owned(),
            "commit".to_owned(),
            Some(DIRECT_FINALIZE_SCOPE.to_owned()),
            Some("commit".to_owned()),
        )
    );
}

async fn staged_publication_session(
    store: &crate::storage::SqliteGraphStore,
    lease_owner: &str,
) -> (crate::domain::CodeIndexSession, CodeIndexPublicationFence) {
    let now_ms = now_millis();
    let queued = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: "repo".to_owned(),
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "commit".to_owned(),
            tree_hash: "tree".to_owned(),
            source_scope: SOURCE_SCOPE.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::Full,
            input_fingerprint: format!("publication-resume-{lease_owner}"),
            resource_budget: Default::default(),
            payload_json: "{}".to_owned(),
            now_ms,
        })
        .await
        .expect("task should queue");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: lease_owner.to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("task should claim")
        .expect("task should be claimable");
    let fence = CodeIndexPublicationFence {
        repository_id: running.repository_id,
        task_id: running.task_id,
        lease_owner: lease_owner.to_owned(),
        attempt_count: running.attempt_count,
        generation: running.publication_generation,
    };
    let session = session_for_scope(SOURCE_SCOPE, 0);
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("session should begin");
    store
        .finalize_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("session should stage for projection");
    (session, fence)
}

pub(super) async fn begin_fenced_session(
    store: &crate::storage::SqliteGraphStore,
    source_scope: &str,
    input_fingerprint: &str,
    lease_owner: &str,
    resource_budget: CodeIndexResourceBudget,
) -> (crate::domain::CodeIndexSession, CodeIndexPublicationFence) {
    let queued = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: "repo".to_owned(),
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "commit".to_owned(),
            tree_hash: "tree".to_owned(),
            source_scope: source_scope.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::Full,
            input_fingerprint: input_fingerprint.to_owned(),
            resource_budget,
            payload_json: "{}".to_owned(),
            now_ms: now_millis(),
        })
        .await
        .expect("task should queue");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: lease_owner.to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("task should claim")
        .expect("task should be claimable");
    let fence = CodeIndexPublicationFence {
        repository_id: running.repository_id,
        task_id: running.task_id,
        lease_owner: lease_owner.to_owned(),
        attempt_count: running.attempt_count,
        generation: running.publication_generation,
    };
    let mut session = session_for_scope(source_scope, 1);
    session.resource_budget = resource_budget;
    (session, fence)
}

fn terminal_receipt(task_id: &str) -> CodeIncrementalSummaryReceipt {
    CodeIncrementalSummaryReceipt {
        task_id: task_id.to_owned(),
        base_resolved_commit_sha: "base".to_owned(),
        changed_path_count: 3,
        skipped_unchanged_count: 7,
        deleted_path_count: 0,
        affected_path_count: 0,
        blob_read_count: 0,
        parsed_file_count: 0,
        sqlite_write_count: 0,
        degraded_file_count: 0,
        batch_count: 1,
    }
}

async fn install_terminal_receipt(
    store: &crate::storage::SqliteGraphStore,
    receipt: &CodeIncrementalSummaryReceipt,
) {
    let encoded = super::super::super::checkpoint_receipt::encode(receipt)
        .expect("terminal receipt should encode");
    store
        .run(move |connection| {
            let changed = connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET incremental_summary_json = ?1
                 WHERE source_scope = ?2 AND state = 'completed'",
                rusqlite::params![encoded, SOURCE_SCOPE],
            )?;
            if changed != 1 {
                return Err(crate::storage::StorageError::Invariant(
                    "completed receipt fixture did not update exactly once".to_owned(),
                ));
            }
            Ok(())
        })
        .await
        .expect("terminal receipt should persist");
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
