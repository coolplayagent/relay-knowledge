//! Attempt-fenced restart contracts for durable ordinary-reference pages.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    domain::{
        CodeIndexPublicationFence, CodeIndexResourceBudget, CodeParseStatus,
        CodeRepositoryRegistration, code_reference_resolution_query_index_repair,
        code_reference_resolution_state,
    },
    storage::{
        CodeIndexFinalizationStep, CodeIndexPublicationStore as _, CodeIndexTaskClaimRequest,
        CodeIndexTaskStore as _, RepositoryCatalogStore as _, SqliteGraphStore, StorageError,
    },
};

use super::{
    publication_barrier_tests::begin_fenced_session,
    tests::{batch, file, reference, registered_store, session_for_scope, symbol},
};

#[tokio::test]
async fn staged_reference_resolution_reopens_through_exact_query_index_repair_cursor() {
    let database_path = database_path("reference-resolution-reopen");
    let source_scope = "git_snapshot:reference-resolution-reopen";
    let budget = page_budget();
    let store = registered_file_store(&database_path).await;
    let (session, fence) = begin_fenced_session(
        &store,
        source_scope,
        "reference-resolution-reopen",
        "reference-resolution-worker",
        budget,
    )
    .await;
    seed_fenced_reference_session(&store, source_scope, &session, &fence, 2).await;

    assert_eq!(
        advance_state(&store, session.clone(), fence.clone()).await,
        resolution_state(0, 0, None)
    );
    assert_eq!(
        advance_state(&store, session.clone(), fence.clone()).await,
        resolution_state(1, 1, Some("reference:01"))
    );
    assert_progress(&store, source_scope, 1, Some("reference:01"), 1).await;
    store
        .run(|connection| {
            connection.execute("DROP INDEX code_repository_symbols_name_path_lookup", [])?;
            Ok(())
        })
        .await
        .expect("required owner index should drop");
    let wrapper = advance_state(&store, session.clone(), fence.clone()).await;
    let repair = code_reference_resolution_query_index_repair(&wrapper)
        .expect("missing owner index must enter an exact repair wrapper");
    assert_eq!(repair.reference_resolution.completed_page_ordinal, 1);
    assert_eq!(
        repair.reference_resolution.checkpoint_state(),
        Some(resolution_state(1, 1, Some("reference:01")))
    );
    drop(store);

    let store = SqliteGraphStore::open(&database_path).expect("store should reopen");
    assert_eq!(
        advance_state(&store, session.clone(), fence.clone()).await,
        resolution_state(1, 1, Some("reference:01"))
    );
    assert_progress(&store, source_scope, 1, Some("reference:01"), 1).await;
    assert_eq!(
        advance_state(&store, session.clone(), fence.clone()).await,
        resolution_state(2, 2, Some("reference:02"))
    );
    assert_eq!(
        advance_state(&store, session, fence).await,
        "finalizing:resolve_references"
    );
    assert_resolved_and_progress_removed(&store, source_scope, 2).await;
    drop(store);
    remove_database_files(&database_path);
}

#[tokio::test]
async fn staged_reference_resolution_takeover_fences_old_page_and_resumes_exact_cursor() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:reference-resolution-takeover";
    let (session, old_fence) = begin_fenced_session(
        &store,
        source_scope,
        "reference-resolution-takeover",
        "reference-resolution-old",
        page_budget(),
    )
    .await;
    seed_fenced_reference_session(&store, source_scope, &session, &old_fence, 2).await;
    assert_eq!(
        advance_state(&store, session.clone(), old_fence.clone()).await,
        resolution_state(0, 0, None)
    );
    assert_eq!(
        advance_state(&store, session.clone(), old_fence.clone()).await,
        resolution_state(1, 1, Some("reference:01"))
    );
    let task_id = old_fence.task_id.clone();
    store
        .run({
            let task_id = task_id.clone();
            move |connection| {
                connection.execute(
                    "UPDATE code_repository_index_tasks SET lease_expires_at_ms = 0
                     WHERE task_id = ?1",
                    [task_id],
                )?;
                Ok(())
            }
        })
        .await
        .expect("old attempt should expire");
    let error = store
        .advance_code_index_session_with_fence(session.clone(), old_fence)
        .await
        .expect_err("expired attempt must not mutate the next page");
    assert!(matches!(error, StorageError::InvalidInput(_)));
    assert_progress(&store, source_scope, 1, Some("reference:01"), 1).await;

    let takeover = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(task_id),
            lease_owner: "reference-resolution-new".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("takeover should run")
        .expect("expired task should be reclaimed");
    let new_fence = CodeIndexPublicationFence {
        repository_id: takeover.repository_id,
        task_id: takeover.task_id,
        lease_owner: "reference-resolution-new".to_owned(),
        attempt_count: takeover.attempt_count,
        generation: takeover.publication_generation,
    };
    assert_eq!(
        advance_state(&store, session, new_fence).await,
        resolution_state(2, 2, Some("reference:02"))
    );
    assert_progress(&store, source_scope, 2, Some("reference:02"), 2).await;
}

#[tokio::test]
async fn active_scope_rejects_durable_reference_resolution_without_partial_mutation() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:active-reference-resolution";
    let (session, fence) = begin_fenced_session(
        &store,
        source_scope,
        "active-reference-resolution",
        "active-reference-resolution-worker",
        page_budget(),
    )
    .await;
    seed_fenced_reference_session(&store, source_scope, &session, &fence, 1).await;
    store
        .run(move |connection| {
            connection.execute(
                "INSERT INTO code_repository_scopes (
                     source_scope, repository_id, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, indexed_file_count,
                     symbol_count, reference_count, chunk_count, stale, degraded_reason
                 ) VALUES (?1, 'repo', 'commit', 'tree', '[]', '[]', 1, 1, 1, 0, 0, NULL)",
                [source_scope],
            )?;
            connection.execute(
                "UPDATE code_repositories SET last_indexed_scope_id = ?1
                 WHERE repository_id = 'repo'",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("active scope should seed");

    let error = store
        .advance_code_index_session_with_fence(session, fence)
        .await
        .expect_err("durable pages must reject a queryable target");
    assert!(matches!(error, StorageError::Invariant(_)));
    let persisted = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT checkpoint.state,
                            reference.target_symbol_snapshot_id,
                            (SELECT COUNT(*)
                             FROM code_repository_reference_resolution_progress progress
                             WHERE progress.source_scope = ?1)
                     FROM code_repository_index_checkpoints checkpoint
                     JOIN code_repository_references reference
                       ON reference.source_scope = checkpoint.source_scope
                     WHERE checkpoint.source_scope = ?1",
                    [source_scope],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, usize>(2)?,
                        ))
                    },
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("unchanged active state should load");
    assert_eq!(
        persisted,
        ("finalizing:build_query_indexes".to_owned(), None, 0)
    );
}

#[tokio::test]
async fn unfenced_reference_resolution_keeps_the_legacy_coarse_checkpoint() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:coarse-reference-resolution";
    let mut session = session_for_scope(source_scope, 1);
    session.resource_budget = page_budget();
    store
        .begin_code_index_session(session.clone())
        .await
        .expect("unfenced session should begin");
    store
        .apply_code_index_batch(reference_batch(source_scope, 1))
        .await
        .expect("unfenced facts should persist");
    prepare_resolution_checkpoint(&store, source_scope).await;

    let step = store
        .run(move |connection| super::finalization::advance_session(connection, session))
        .await
        .expect("legacy resolution should advance atomically");
    assert!(matches!(
        step,
        super::finalization::CodeIndexFinalizationAdvance::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:resolve_references"
    ));
    assert_resolved_and_progress_removed(&store, source_scope, 1).await;
}

async fn seed_fenced_reference_session(
    store: &SqliteGraphStore,
    source_scope: &str,
    session: &crate::domain::CodeIndexSession,
    fence: &CodeIndexPublicationFence,
    reference_count: usize,
) {
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced session should begin");
    store
        .apply_code_index_batch_with_fence(
            reference_batch(source_scope, reference_count),
            fence.clone(),
        )
        .await
        .expect("fenced facts should persist");
    prepare_resolution_checkpoint(store, source_scope).await;
}

fn reference_batch(source_scope: &str, reference_count: usize) -> crate::domain::CodeIndexBatch {
    let references = (1..=reference_count)
        .map(|ordinal| {
            let mut record = reference(
                source_scope,
                &format!("reference:{ordinal:02}"),
                "file:1",
                "src/lib.rs",
                "target",
            );
            record.kind = "read".to_owned();
            record
        })
        .collect();
    crate::domain::CodeIndexBatch {
        files: vec![file(
            source_scope,
            "file:1",
            "src/lib.rs",
            "rust",
            CodeParseStatus::Parsed,
        )],
        symbols: vec![symbol(
            source_scope,
            "symbol:target",
            "file:1",
            "src/lib.rs",
            "target",
            "rust",
        )],
        references,
        ..batch(source_scope, 1)
    }
}

async fn prepare_resolution_checkpoint(store: &SqliteGraphStore, source_scope: &str) {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            super::super::super::schema::ensure_code_query_indexes(connection)?;
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'finalizing:build_query_indexes'
                 WHERE source_scope = ?1",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("resolution checkpoint should prepare");
}

async fn advance_state(
    store: &SqliteGraphStore,
    session: crate::domain::CodeIndexSession,
    fence: CodeIndexPublicationFence,
) -> String {
    let step = store
        .advance_code_index_session_with_fence(session, fence)
        .await
        .expect("one durable reference-resolution quantum should advance");
    let CodeIndexFinalizationStep::Pending { checkpoint_state } = step else {
        panic!("reference-resolution quantum must remain pending");
    };
    checkpoint_state
}

async fn assert_progress(
    store: &SqliteGraphStore,
    source_scope: &str,
    page: usize,
    cursor: Option<&str>,
    resolved: usize,
) {
    let source_scope = source_scope.to_owned();
    let persisted = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT checkpoint.state, progress.completed_page_ordinal,
                            progress.cursor_reference_id, progress.resolved_reference_count
                     FROM code_repository_index_checkpoints checkpoint
                     JOIN code_repository_reference_resolution_progress progress
                       ON progress.source_scope = checkpoint.source_scope
                     WHERE checkpoint.source_scope = ?1",
                    [source_scope],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, usize>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, usize>(3)?,
                        ))
                    },
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("progress should load");
    assert_eq!(
        persisted,
        (
            resolution_state(page, resolved, cursor),
            page,
            cursor.map(str::to_owned),
            resolved,
        )
    );
}

async fn assert_resolved_and_progress_removed(
    store: &SqliteGraphStore,
    source_scope: &str,
    expected_reference_count: usize,
) {
    let source_scope = source_scope.to_owned();
    let persisted = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                         (SELECT COUNT(*) FROM code_repository_references
                          WHERE source_scope = ?1 AND resolution_state = 'resolved'
                            AND target_symbol_snapshot_id = 'symbol:target'),
                         (SELECT COUNT(*) FROM code_repository_reference_resolution_progress
                          WHERE source_scope = ?1)",
                    [source_scope],
                    |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?)),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("resolved state should load");
    assert_eq!(persisted, (expected_reference_count, 0));
}

fn page_budget() -> CodeIndexResourceBudget {
    CodeIndexResourceBudget::new(1, 1024 * 1024, 3).expect("page budget should build")
}

async fn registered_file_store(database_path: &std::path::Path) -> SqliteGraphStore {
    let store = SqliteGraphStore::open(database_path).expect("file store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
}

fn database_path(label: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "relay-knowledge-{label}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn remove_database_files(database_path: &std::path::Path) {
    for path in [
        database_path.to_path_buf(),
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ] {
        let _ = std::fs::remove_file(path);
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn resolution_state(page: usize, resolved: usize, cursor: Option<&str>) -> String {
    code_reference_resolution_state(page, resolved, cursor)
        .expect("test reference-resolution checkpoint should be canonical")
}
