//! Bounded multi-values persistence contracts for reference facts.

use crate::{
    domain::{
        CodeIndexBatch, CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget,
        CodeIndexSession, CodeParseStatus, CodeRepositoryRegistration, RepositoryCodeFileRecord,
        RepositoryCodeRange, RepositoryCodeReferenceRecord,
    },
    storage::{
        CodeIndexPublicationStore as _, CodeIndexTaskClaimRequest, CodeIndexTaskSeed,
        CodeIndexTaskStore as _, RepositoryCatalogStore as _, SqliteGraphStore, StorageError,
    },
};
use rusqlite::{limits::Limit, params};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    REFERENCE_INSERT_BATCH_SIZE, REFERENCE_INSERT_BIND_COUNT, REFERENCE_INSERT_COLUMN_COUNT,
};

#[test]
fn reference_multi_values_batch_has_a_fixed_bind_ceiling() {
    assert_eq!(REFERENCE_INSERT_BATCH_SIZE, 1_024);
    assert_eq!(REFERENCE_INSERT_COLUMN_COUNT, 16);
    assert_eq!(REFERENCE_INSERT_BIND_COUNT, 16_384);
}

#[tokio::test]
async fn code_index_persistence_performance_suite_reference_multi_values_preserves_1025_rows_and_replay_is_idempotent()
 {
    let source_scope = "git_snapshot:reference-bulk-boundary";
    let store = store_with_session(source_scope).await;
    mark_scope_active(&store, source_scope).await;
    let references = (0..=REFERENCE_INSERT_BATCH_SIZE)
        .map(|index| reference(source_scope, index))
        .collect::<Vec<_>>();
    let expected_ids = references
        .iter()
        .map(|reference| reference.reference_id.clone())
        .collect::<Vec<_>>();
    let batch = batch(source_scope, references);

    let first = store
        .apply_code_index_batch(batch.clone())
        .await
        .expect("1,025 reference facts should span two bounded statements");
    let replayed = store
        .apply_code_index_batch(batch)
        .await
        .expect("published batch replay should be a no-op");

    assert_eq!(first, replayed);
    assert_eq!(first.batch_count, 1);
    assert_eq!(first.committed_file_count, 1);
    assert_eq!(first.committed_reference_count, expected_ids.len());
    assert_eq!(reference_ids(&store, source_scope).await, expected_ids);
    assert!(
        reference_search_document_ids(&store, source_scope)
            .await
            .is_empty(),
        "reference FTS is deferred until bounded grouped finalization"
    );
    assert_eq!(
        reference_boundary_row(&store, source_scope).await,
        (
            "reference-1024".to_owned(),
            "Target1024".to_owned(),
            Some("symbol-1024".to_owned()),
            Some("Target1024".to_owned()),
            3_524,
            1_024,
            1_030,
            1_025,
            1_025,
        )
    );
}

#[tokio::test]
async fn finalization_page_allows_replay_but_rejects_a_late_fenced_batch_atomically() {
    let source_scope = "git_snapshot:reference-late-batch";
    let budget = CodeIndexResourceBudget::new(1, 1024 * 1024, 8).expect("budget should build");
    let store = registered_store().await;
    let now_ms = now_millis();
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
            input_fingerprint: "reference-late-batch".to_owned(),
            resource_budget: budget,
            payload_json: "{}".to_owned(),
            now_ms,
        })
        .await
        .expect("task should queue");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "reference-late-batch-worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms,
        })
        .await
        .expect("task should claim")
        .expect("queued task should be claimable");
    let fence = CodeIndexPublicationFence {
        repository_id: running.repository_id,
        task_id: running.task_id,
        lease_owner: "reference-late-batch-worker".to_owned(),
        attempt_count: running.attempt_count,
        generation: running.publication_generation,
    };
    let session = session_for_scope(source_scope, budget);
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced session should begin");
    let first_batch = batch(source_scope, vec![reference(source_scope, 0)]);
    store
        .apply_code_index_batch_with_fence(first_batch.clone(), fence.clone())
        .await
        .expect("first batch should persist");
    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'finalizing:refresh_dependencies'
                 WHERE source_scope = ?1",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("finalization boundary should persist");

    for expected_state in [
        "finalizing:rebuild_reference_search:v2:cleanup:0",
        "finalizing:rebuild_reference_search:v2:discover:0",
        "finalizing:rebuild_reference_search:v2:discover:1",
        "finalizing:rebuild_reference_search:v2:build:0",
        "finalizing:rebuild_reference_search:v2:build:1",
    ] {
        let mut matched_reference_page = false;
        for _ in 0..=crate::domain::CODE_QUERY_INDEX_PLAN_UNIT_COUNT + 1 {
            let step = store
                .advance_code_index_session_with_fence(session.clone(), fence.clone())
                .await
                .expect("query-index repair or reference-search page should advance");
            let crate::storage::CodeIndexFinalizationStep::Pending { checkpoint_state } = step
            else {
                panic!("reference-search page should remain pending");
            };
            if crate::domain::code_query_index_repair(&checkpoint_state).is_some() {
                continue;
            }
            if checkpoint_state == "finalizing:refresh_dependencies" {
                continue;
            }
            assert_eq!(checkpoint_state, expected_state);
            matched_reference_page = true;
            break;
        }
        assert!(
            matched_reference_page,
            "bounded query-index repair should resume the expected reference-search page"
        );
    }

    let before = late_batch_snapshot(&store, source_scope).await;
    assert_eq!(
        before.checkpoint_state,
        "finalizing:rebuild_reference_search:v2:build:1"
    );
    assert_eq!(before.batch_count, 1);
    assert_eq!(before.committed_file_count, 1);
    assert_eq!(before.committed_reference_count, 1);
    assert_eq!(before.progress_stage, "build");
    assert_eq!(before.progress_page, 1);
    assert_eq!(before.reference_count, 1);
    assert_eq!(before.file_count, 1);
    assert_eq!(before.search_count, 1);
    assert_eq!(before.metadata_count, 1);
    assert_eq!(before.search_record_id.as_deref(), Some("reference-0000"));
    assert_eq!(before.progress_built_count, 1);
    assert_eq!(before.progress_cursor.as_deref(), Some("reference-0000"));
    assert_eq!(before.late_staging_count, 0);

    let replayed = store
        .apply_code_index_batch_with_fence(first_batch, fence.clone())
        .await
        .expect("a committed batch replay should remain a pure no-op during finalization");
    assert_eq!(replayed.state, before.checkpoint_state);
    assert_eq!(late_batch_snapshot(&store, source_scope).await, before);

    let mut late_batch = batch(source_scope, vec![reference(source_scope, 1)]);
    late_batch.batch_index = 2;
    late_batch.files.clear();
    let error = store
        .apply_code_index_batch_with_fence(late_batch, fence)
        .await
        .expect_err("a new batch must not mutate facts behind a durable search-page cursor");
    assert!(matches!(
        error,
        StorageError::Invariant(message) if message.contains("no longer accepts new batch 2")
    ));
    assert_eq!(late_batch_snapshot(&store, source_scope).await, before);
}

#[tokio::test]
async fn second_reference_values_group_failure_rolls_back_the_fact_transaction() {
    let source_scope = "git_snapshot:reference-bulk-rollback";
    let store = store_with_session(source_scope).await;
    let mut references = (0..=REFERENCE_INSERT_BATCH_SIZE)
        .map(|index| reference(source_scope, index))
        .collect::<Vec<_>>();
    references[REFERENCE_INSERT_BATCH_SIZE].reference_id = references[0].reference_id.clone();

    let error = store
        .apply_code_index_batch(batch(source_scope, references))
        .await
        .expect_err("a duplicate in the second values group must reject the batch");

    assert!(error.to_string().contains("UNIQUE constraint failed"));
    assert_eq!(fact_counts(&store, source_scope).await, (0, 0, 0));
    let checkpoint = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");
    assert_eq!(checkpoint.committed_file_count, 0);
    assert_eq!(checkpoint.committed_reference_count, 0);
    assert_eq!(checkpoint.batch_count, 0);
    assert_eq!(batch_staging_state(&store, source_scope).await, "staged");
}

#[tokio::test]
async fn lower_connection_variable_limit_shrinks_reference_groups() {
    let source_scope = "git_snapshot:reference-lower-variable-limit";
    let store = store_with_session(source_scope).await;
    set_variable_limit(&store, 31).await;
    let references = (0..3)
        .map(|index| reference(source_scope, index))
        .collect::<Vec<_>>();
    let expected_ids = references
        .iter()
        .map(|reference| reference.reference_id.clone())
        .collect::<Vec<_>>();

    let checkpoint = store
        .apply_code_index_batch(batch(source_scope, references))
        .await
        .expect("31 variables should admit one 16-column row per statement");

    assert_eq!(checkpoint.committed_reference_count, 3);
    assert_eq!(reference_ids(&store, source_scope).await, expected_ids);
}

#[tokio::test]
async fn one_reference_row_may_use_the_exact_variable_limit() {
    let source_scope = "git_snapshot:reference-exact-variable-limit";
    let store = store_with_session(source_scope).await;
    set_variable_limit(
        &store,
        i32::try_from(REFERENCE_INSERT_COLUMN_COUNT).expect("column count should fit"),
    )
    .await;

    let reference_batch = batch(source_scope, vec![reference(source_scope, 0)]);
    store
        .run(move |connection| {
            let transaction = connection.transaction()?;
            super::insert_references(&transaction, &reference_batch, None)?;
            transaction.commit()?;
            Ok(())
        })
        .await
        .expect("the reference owner should accept the inclusive variable limit");

    assert_eq!(
        reference_ids(&store, source_scope).await,
        vec!["reference-0000".to_owned()]
    );
}

#[tokio::test]
async fn variable_limit_below_one_reference_row_is_rejected() {
    let source_scope = "git_snapshot:reference-insufficient-variable-limit";
    let store = store_with_session(source_scope).await;
    set_variable_limit(
        &store,
        i32::try_from(REFERENCE_INSERT_COLUMN_COUNT - 1).expect("column count should fit"),
    )
    .await;

    let error = store
        .apply_code_index_batch(batch(source_scope, vec![reference(source_scope, 0)]))
        .await
        .expect_err("fewer variables than one row requires must fail closed");

    assert!(
        error
            .to_string()
            .contains("cannot admit one 16-column reference row")
    );
    assert_eq!(fact_counts(&store, source_scope).await, (0, 0, 0));
    let checkpoint = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");
    assert_eq!(checkpoint.committed_file_count, 0);
    assert_eq!(checkpoint.committed_reference_count, 0);
    assert_eq!(checkpoint.batch_count, 0);
    assert_eq!(batch_staging_state(&store, source_scope).await, "staged");
}

async fn store_with_session(source_scope: &str) -> SqliteGraphStore {
    let store = registered_store().await;
    store
        .begin_code_index_session(session_for_scope(
            source_scope,
            CodeIndexResourceBudget::default(),
        ))
        .await
        .expect("session should begin");
    store
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo",
                "fixture",
                "/tmp/reference-bulk-fixture",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
}

fn session_for_scope(
    source_scope: &str,
    resource_budget: CodeIndexResourceBudget,
) -> CodeIndexSession {
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
        changed_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget,
    }
}

async fn mark_scope_active(store: &SqliteGraphStore, source_scope: &str) {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = ?1
                 WHERE repository_id = 'repo'",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("scope should become the active intermediate-search scope");
}

async fn set_variable_limit(store: &SqliteGraphStore, variable_limit: i32) {
    store
        .run(move |connection| {
            connection.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, variable_limit);
            assert_eq!(
                connection.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER),
                variable_limit
            );
            Ok(())
        })
        .await
        .expect("SQLite variable limit should be set");
}

fn batch(source_scope: &str, references: Vec<RepositoryCodeReferenceRecord>) -> CodeIndexBatch {
    CodeIndexBatch {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        batch_index: 1,
        parsed_byte_count: 1_024,
        files: vec![RepositoryCodeFileRecord {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            file_id: "file".to_owned(),
            path: "src/lib.rs".to_owned(),
            language_id: "rust".to_owned(),
            blob_hash: "blob".to_owned(),
            byte_len: 1_024,
            line_count: 512,
            parse_status: CodeParseStatus::Parsed,
            is_generated: false,
            degraded_reason: None,
        }],
        symbols: Vec::new(),
        references,
        imports: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn reference(source_scope: &str, index: usize) -> RepositoryCodeReferenceRecord {
    let name = format!("Target{index}");
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        reference_id: format!("reference-{index:04}"),
        file_id: "file".to_owned(),
        path: "src/lib.rs".to_owned(),
        name: name.clone(),
        kind: "call".to_owned(),
        target_symbol_snapshot_id: (index % 2 == 0).then(|| format!("symbol-{index:04}")),
        target_hint: Some(name),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 2_500 + u16::try_from(index).expect("fixture index should fit"),
        confidence_tier: "ambiguous".to_owned(),
        byte_range: RepositoryCodeRange {
            start: u32::try_from(index).expect("fixture index should fit"),
            end: u32::try_from(index + 6).expect("fixture range should fit"),
        },
        line_range: RepositoryCodeRange {
            start: u32::try_from(index + 1).expect("fixture line should fit"),
            end: u32::try_from(index + 1).expect("fixture line should fit"),
        },
    }
}

async fn reference_ids(store: &SqliteGraphStore, source_scope: &str) -> Vec<String> {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            let mut statement = connection.prepare(
                "SELECT reference_id FROM code_repository_references
                 WHERE source_scope = ?1 ORDER BY rowid",
            )?;
            let rows = statement.query_map([source_scope], |row| row.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("reference ids should load")
}

async fn reference_search_document_ids(
    store: &SqliteGraphStore,
    source_scope: &str,
) -> Vec<String> {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            let mut statement = connection.prepare(
                "SELECT record_id FROM code_repository_search_metadata
                 WHERE source_scope = ?1 AND document_kind = 'reference'
                 ORDER BY search_rowid",
            )?;
            let rows = statement.query_map([source_scope], |row| row.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("reference search document ids should load")
}

async fn reference_boundary_row(
    store: &SqliteGraphStore,
    source_scope: &str,
) -> (
    String,
    String,
    Option<String>,
    Option<String>,
    u16,
    u32,
    u32,
    u32,
    u32,
) {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT reference_id, name, target_symbol_snapshot_id, target_hint,
                            confidence_basis_points, byte_start, byte_end, line_start, line_end
                     FROM code_repository_references
                     WHERE source_scope = ?1 ORDER BY rowid DESC LIMIT 1",
                    [source_scope],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                            row.get(8)?,
                        ))
                    },
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("boundary reference should load")
}

async fn fact_counts(store: &SqliteGraphStore, source_scope: &str) -> (usize, usize, usize) {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                         (SELECT COUNT(*) FROM code_repository_files WHERE source_scope = ?1),
                         (SELECT COUNT(*) FROM code_repository_references WHERE source_scope = ?1),
                         (SELECT COUNT(*) FROM code_repository_search_metadata
                          WHERE source_scope = ?1 AND document_kind = 'reference')",
                    [source_scope],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("fact counts should load")
}

async fn batch_staging_state(store: &SqliteGraphStore, source_scope: &str) -> String {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT state FROM code_repository_index_batch_staging
                     WHERE source_scope = ?1 AND batch_index = 1",
                    params![source_scope],
                    |row| row.get(0),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("batch staging state should load")
}

#[derive(Debug, PartialEq, Eq)]
struct LateBatchSnapshot {
    checkpoint_state: String,
    batch_count: usize,
    committed_file_count: usize,
    committed_reference_count: usize,
    progress_stage: String,
    progress_page: usize,
    progress_built_count: usize,
    progress_cursor: Option<String>,
    file_count: usize,
    reference_count: usize,
    search_count: usize,
    metadata_count: usize,
    search_record_id: Option<String>,
    late_staging_count: usize,
}

async fn late_batch_snapshot(store: &SqliteGraphStore, source_scope: &str) -> LateBatchSnapshot {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT checkpoint.state, checkpoint.batch_count,
                            checkpoint.committed_file_count,
                            checkpoint.committed_reference_count,
                            progress.stage, progress.completed_page_ordinal,
                            progress.built_count, progress.build_cursor_group_id,
                            (SELECT COUNT(*) FROM code_repository_files
                             WHERE source_scope = ?1),
                            (SELECT COUNT(*) FROM code_repository_references
                             WHERE source_scope = ?1),
                            (SELECT COUNT(*) FROM code_repository_search
                             WHERE source_scope = ?1 AND document_kind = 'reference'),
                            (SELECT COUNT(*) FROM code_repository_search_metadata
                             WHERE source_scope = ?1 AND document_kind = 'reference'),
                            (SELECT record_id FROM code_repository_search
                             WHERE source_scope = ?1 AND document_kind = 'reference'
                             ORDER BY rowid LIMIT 1),
                            (SELECT COUNT(*) FROM code_repository_index_batch_staging
                             WHERE source_scope = ?1 AND batch_index = 2)
                     FROM code_repository_index_checkpoints checkpoint
                     JOIN code_repository_reference_search_progress progress
                       ON progress.source_scope = checkpoint.source_scope
                     WHERE checkpoint.source_scope = ?1",
                    [source_scope],
                    |row| {
                        Ok(LateBatchSnapshot {
                            checkpoint_state: row.get(0)?,
                            batch_count: row.get(1)?,
                            committed_file_count: row.get(2)?,
                            committed_reference_count: row.get(3)?,
                            progress_stage: row.get(4)?,
                            progress_page: row.get(5)?,
                            progress_built_count: row.get(6)?,
                            progress_cursor: row.get(7)?,
                            file_count: row.get(8)?,
                            reference_count: row.get(9)?,
                            search_count: row.get(10)?,
                            metadata_count: row.get(11)?,
                            search_record_id: row.get(12)?,
                            late_staging_count: row.get(13)?,
                        })
                    },
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("late-batch durability snapshot should load")
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
