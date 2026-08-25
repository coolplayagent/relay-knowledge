//! Partitioned staged-owner fencing for durable reference-search pages.

use crate::{
    domain::{
        CodeIndexResourceBudget, CodeReferenceSearchRebuildStage, RepositoryCodeRange,
        RepositoryCodeReferenceRecord, code_reference_search_query_index_repair,
    },
    storage::{
        CodeIndexFinalizationStep, CodeIndexTaskClaimRequest, CodeRepositoryStore,
        PartitionedSqliteKnowledgeStore, StorageError,
    },
};
use rusqlite::{Connection, params};

use super::{
    super::test_support::partitioned_store_with_paths,
    publication_barrier_tests::{
        batch_from_snapshot, now_millis, publication_fence, registration, session_from_snapshot,
        snapshot, task_seed,
    },
};

#[tokio::test]
async fn partitioned_reference_page_requires_the_exact_staged_task_owner() {
    let (store, control_path, _paths) =
        partitioned_store_with_paths("partitioned-reference-page-owner");
    let source_scope = "scope-partitioned-reference-page-owner";
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let mut snapshot = snapshot(source_scope);
    snapshot.references = (1..=5)
        .map(|ordinal| reference(source_scope, ordinal))
        .collect();
    let budget = CodeIndexResourceBudget::new(1, 1024 * 1024, 8).expect("budget should build");
    let mut seed = task_seed(source_scope);
    seed.resource_budget = budget;
    let queued = store
        .queue_code_index_task(seed)
        .await
        .expect("fenced full task should queue");
    let task = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "reference-page-worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("fenced claim should run")
        .expect("fenced task should claim");
    let fence = publication_fence(&task, "reference-page-worker");
    let mut session = session_from_snapshot(&snapshot);
    session.resource_budget = budget;
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced session should begin");
    store
        .apply_code_index_batch_with_fence(batch_from_snapshot(snapshot), fence.clone())
        .await
        .expect("fenced batch should persist");
    let shard = store
        .catalog
        .checkpoint_repository_store("repo".to_owned())
        .await
        .expect("staged shard should resolve")
        .expect("staged shard should exist");
    shard
        .run(move |connection| {
            crate::storage::sqlite::code::ensure_code_query_indexes(connection)?;
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'finalizing:refresh_dependencies'
                 WHERE source_scope = ?1",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("reference-search boundary should persist");
    let control = Connection::open(&control_path).expect("control observer should open");
    assert_eq!(
        control
            .query_row(
                "SELECT state, staged_task_id FROM storage_repository_shard_scopes
                 WHERE repository_id = ?1 AND source_scope = ?2",
                params![&task.repository_id, source_scope],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .expect("staged route should load"),
        ("staged".to_owned(), Some(task.task_id.clone()))
    );
    drop(control);

    for expected in [
        "finalizing:rebuild_reference_search:v2:cleanup:0",
        "finalizing:rebuild_reference_search:v2:discover:0",
        "finalizing:rebuild_reference_search:v2:discover:1",
        "finalizing:rebuild_reference_search:v2:discover:2",
        "finalizing:rebuild_reference_search:v2:discover:3",
        "finalizing:rebuild_reference_search:v2:build:0",
        "finalizing:rebuild_reference_search:v2:build:1",
    ] {
        let step = store
            .advance_code_index_session_with_fence(session.clone(), fence.clone())
            .await
            .expect("exact staged owner should advance one page");
        assert!(matches!(
            step,
            CodeIndexFinalizationStep::Pending { checkpoint_state }
                if checkpoint_state == expected
        ));
    }
    let before = reference_page_state(&shard, source_scope).await;
    assert_eq!(before, ("build".to_owned(), 1, 2, 2, 2));
    let control = Connection::open(&control_path).expect("control owner should reopen");
    assert_eq!(
        control
            .execute(
                "UPDATE storage_repository_shard_scopes
                 SET staged_task_id = 'different-task'
                 WHERE repository_id = ?1 AND source_scope = ?2 AND state = 'staged'",
                params![&task.repository_id, source_scope],
            )
            .expect("staged owner should drift"),
        1
    );

    let error = store
        .advance_code_index_session_with_fence(session, fence)
        .await
        .expect_err("changed staged owner must reject the next build page");
    assert!(matches!(error, StorageError::Invariant(_)));
    assert_eq!(
        reference_page_state(&shard, source_scope).await,
        before,
        "owner drift must roll back FTS, metadata, progress, and checkpoint together"
    );
}

#[tokio::test]
async fn partitioned_reference_build_page_and_repair_token_survive_full_reopen() {
    let (store, control_path, paths) =
        partitioned_store_with_paths("partitioned-reference-page-reopen");
    let source_scope = "scope-partitioned-reference-page-reopen";
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let mut snapshot = snapshot(source_scope);
    snapshot.references = (1..=5)
        .map(|ordinal| reference(source_scope, ordinal))
        .collect();
    let budget = CodeIndexResourceBudget::new(1, 1024 * 1024, 8).expect("budget should build");
    let mut seed = task_seed(source_scope);
    seed.input_fingerprint = "partitioned-reference-page-reopen".to_owned();
    seed.resource_budget = budget;
    let queued = store
        .queue_code_index_task(seed)
        .await
        .expect("fenced full task should queue");
    let task = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "reference-reopen-worker".to_owned(),
            lease_duration_ms: 600_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("fenced claim should run")
        .expect("fenced task should claim");
    let fence = publication_fence(&task, "reference-reopen-worker");
    let mut session = session_from_snapshot(&snapshot);
    session.resource_budget = budget;
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced session should begin");
    store
        .apply_code_index_batch_with_fence(batch_from_snapshot(snapshot), fence.clone())
        .await
        .expect("fenced batch should persist");
    let shard = store
        .catalog
        .checkpoint_repository_store("repo".to_owned())
        .await
        .expect("staged shard should resolve")
        .expect("staged shard should exist");
    shard
        .run(move |connection| {
            crate::storage::sqlite::code::ensure_code_query_indexes(connection)?;
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'finalizing:refresh_dependencies'
                 WHERE source_scope = ?1",
                [source_scope],
            )?;
            Ok(())
        })
        .await
        .expect("reference-search boundary should persist");

    for expected in [
        "finalizing:rebuild_reference_search:v2:cleanup:0",
        "finalizing:rebuild_reference_search:v2:discover:0",
        "finalizing:rebuild_reference_search:v2:discover:1",
        "finalizing:rebuild_reference_search:v2:discover:2",
        "finalizing:rebuild_reference_search:v2:discover:3",
        "finalizing:rebuild_reference_search:v2:build:0",
        "finalizing:rebuild_reference_search:v2:build:1",
    ] {
        let step = store
            .advance_code_index_session_with_fence(session.clone(), fence.clone())
            .await
            .expect("one reference-search quantum should advance");
        assert!(matches!(
            step,
            CodeIndexFinalizationStep::Pending { checkpoint_state }
                if checkpoint_state == expected
        ));
    }
    let committed_page = durable_reference_search_state(&shard, source_scope).await;
    assert_eq!(
        committed_page,
        DurableReferenceSearchState {
            checkpoint_state: "finalizing:rebuild_reference_search:v2:build:1".to_owned(),
            stage: Some("build".to_owned()),
            completed_page_ordinal: Some(1),
            build_cursor_group_id: Some("reference:2".to_owned()),
            built_count: Some(2),
            fact_count: 5,
            search_count: 2,
            metadata_count: 2,
            document_ids: reference_ids(2),
        }
    );
    assert_partition_remains_staged_without_receipt(&store, &task, source_scope).await;

    shard
        .run(|connection| {
            connection.execute(
                "DROP INDEX code_repository_imports_scope_path_line_lookup",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("one appended query index should become missing");
    let wrapper = store
        .advance_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("missing query index should persist a repair wrapper");
    let CodeIndexFinalizationStep::Pending {
        checkpoint_state: wrapper_state,
    } = wrapper
    else {
        panic!("reference-search repair wrapper must remain pending");
    };
    let repair = code_reference_search_query_index_repair(&wrapper_state)
        .expect("wrapper should preserve a canonical reference-search cursor");
    assert_eq!(
        wrapper_state,
        "finalizing:query_index_repair:v3:16:resume:reference_search:v2:build:1"
    );
    assert_eq!(
        repair.reference_search.stage,
        CodeReferenceSearchRebuildStage::Build
    );
    assert_eq!(repair.reference_search.completed_page_ordinal, 1);
    let wrapped_page = durable_reference_search_state(&shard, source_scope).await;
    assert_eq!(wrapped_page.checkpoint_state, wrapper_state);
    assert_eq!(
        wrapped_page.payload(),
        committed_page.payload(),
        "persisting the repair wrapper must not replay the committed page"
    );

    drop(shard);
    drop(store);

    let store = PartitionedSqliteKnowledgeStore::open(control_path, paths)
        .expect("partitioned control and shard files should reopen");
    let recovered = store
        .active_code_index_task("repo".to_owned())
        .await
        .expect("reopened live task should load")
        .expect("reopened live task should exist");
    assert_eq!(recovered.task_id, task.task_id);
    assert_eq!(recovered.source_scope, source_scope);
    assert_eq!(recovered.attempt_count, task.attempt_count);
    assert_eq!(
        recovered.publication_generation,
        task.publication_generation
    );
    assert_eq!(
        recovered.lease_owner.as_deref(),
        Some("reference-reopen-worker")
    );
    let shard = store
        .catalog
        .checkpoint_repository_store("repo".to_owned())
        .await
        .expect("reopened staged shard should resolve")
        .expect("reopened staged shard should exist");
    assert_eq!(
        durable_reference_search_state(&shard, source_scope).await,
        wrapped_page,
        "reopen must preserve the exact wrapper, cursor, FTS rows, and metadata"
    );

    let restored = store
        .advance_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("reopened repair wrapper should restore its exact page token");
    assert!(matches!(
        restored,
        CodeIndexFinalizationStep::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:rebuild_reference_search:v2:build:1"
    ));
    assert_eq!(
        durable_reference_search_state(&shard, source_scope).await,
        committed_page,
        "repair completion must neither replay nor skip the committed build page"
    );
    let next = store
        .advance_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("the exact next build page should commit after reopen");
    assert!(matches!(
        next,
        CodeIndexFinalizationStep::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:rebuild_reference_search:v2:build:2"
    ));
    let completed_pages = durable_reference_search_state(&shard, source_scope).await;
    assert_eq!(completed_pages.completed_page_ordinal, Some(2));
    assert_eq!(
        completed_pages.build_cursor_group_id.as_deref(),
        Some("reference:4")
    );
    assert_eq!(completed_pages.built_count, Some(4));
    assert_eq!(completed_pages.fact_count, 5);
    assert_eq!(completed_pages.search_count, 4);
    assert_eq!(completed_pages.metadata_count, 4);
    assert_eq!(completed_pages.document_ids, reference_ids(4));

    let summary = store
        .finalize_code_index_session_with_fence(session, fence.clone())
        .await
        .expect("reopened session should finish all code finalization quanta");
    assert_eq!(summary.reference_count, 5);
    let finalized = durable_reference_search_state(&shard, source_scope).await;
    assert_eq!(finalized.checkpoint_state, "finalizing:software_projection");
    assert_eq!(finalized.stage, None);
    assert_eq!(finalized.fact_count, 5);
    assert_eq!(finalized.search_count, 5);
    assert_eq!(finalized.metadata_count, 5);
    assert_eq!(finalized.document_ids, reference_ids(5));
    assert_partition_remains_staged_without_receipt(&store, &task, source_scope).await;

    store
        .refresh_software_global_projection_with_fence(source_scope.to_owned(), fence)
        .await
        .expect("software projection and catalog handoff should publish together");
    assert_eq!(
        store
            .catalog
            .active_repository_for_scope(source_scope.to_owned())
            .await
            .expect("active route should load")
            .as_deref(),
        Some("repo")
    );
    assert_eq!(
        store
            .code_index_checkpoint(source_scope.to_owned())
            .await
            .expect("published checkpoint should load")
            .expect("published checkpoint should exist")
            .state,
        "completed"
    );
    assert!(
        store
            .code_index_publication_receipt(
                task.task_id,
                task.repository_id,
                source_scope.to_owned(),
                now_millis(),
            )
            .await
            .expect("publication receipt should load"),
        "only the completed software/catalog handoff may create the receipt"
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DurableReferenceSearchState {
    checkpoint_state: String,
    stage: Option<String>,
    completed_page_ordinal: Option<usize>,
    build_cursor_group_id: Option<String>,
    built_count: Option<usize>,
    fact_count: usize,
    search_count: usize,
    metadata_count: usize,
    document_ids: Vec<String>,
}

impl DurableReferenceSearchState {
    fn payload(&self) -> Self {
        let mut payload = self.clone();
        payload.checkpoint_state.clear();
        payload
    }
}

async fn durable_reference_search_state(
    shard: &crate::storage::SqliteGraphStore,
    source_scope: &str,
) -> DurableReferenceSearchState {
    let source_scope = source_scope.to_owned();
    shard
        .run(move |connection| {
            let mut state = connection.query_row(
                "SELECT checkpoint.state, progress.stage,
                        progress.completed_page_ordinal,
                        progress.build_cursor_group_id, progress.built_count,
                        (SELECT COUNT(*) FROM code_repository_references
                         WHERE source_scope = ?1),
                        (SELECT COUNT(*) FROM code_repository_search
                         WHERE source_scope = ?1 AND document_kind = 'reference'),
                        (SELECT COUNT(*) FROM code_repository_search_metadata
                         WHERE source_scope = ?1 AND document_kind = 'reference')
                 FROM code_repository_index_checkpoints checkpoint
                 LEFT JOIN code_repository_reference_search_progress progress
                   ON progress.source_scope = checkpoint.source_scope
                 WHERE checkpoint.source_scope = ?1",
                [&source_scope],
                |row| {
                    Ok(DurableReferenceSearchState {
                        checkpoint_state: row.get(0)?,
                        stage: row.get(1)?,
                        completed_page_ordinal: row.get(2)?,
                        build_cursor_group_id: row.get(3)?,
                        built_count: row.get(4)?,
                        fact_count: row.get(5)?,
                        search_count: row.get(6)?,
                        metadata_count: row.get(7)?,
                        document_ids: Vec::new(),
                    })
                },
            )?;
            let mut statement = connection.prepare(
                "SELECT search_row.record_id
                 FROM code_repository_search_metadata metadata
                 JOIN code_repository_search search_row
                   ON search_row.rowid = metadata.search_rowid
                  AND search_row.source_scope = metadata.source_scope
                  AND search_row.document_kind = metadata.document_kind
                  AND search_row.record_id = metadata.record_id
                  AND search_row.path = metadata.path
                 WHERE metadata.source_scope = ?1
                   AND metadata.document_kind = 'reference'
                 ORDER BY search_row.record_id",
            )?;
            state.document_ids = statement
                .query_map([&source_scope], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(state)
        })
        .await
        .expect("durable reference-search state should load")
}

async fn assert_partition_remains_staged_without_receipt(
    store: &PartitionedSqliteKnowledgeStore,
    task: &crate::domain::CodeIndexTaskRecord,
    source_scope: &str,
) {
    assert_eq!(
        store
            .catalog
            .repository_for_scope(source_scope.to_owned())
            .await
            .expect("staged route should load")
            .as_deref(),
        Some("repo")
    );
    assert!(
        store
            .catalog
            .active_repository_for_scope(source_scope.to_owned())
            .await
            .expect("active route should load")
            .is_none(),
        "reference-search progress must remain hidden before software/catalog handoff"
    );
    assert!(
        !store
            .code_index_publication_receipt(
                task.task_id.clone(),
                task.repository_id.clone(),
                source_scope.to_owned(),
                now_millis(),
            )
            .await
            .expect("unpublished receipt state should load"),
        "staged reference-search work must not have a publication receipt"
    );
}

fn reference_ids(count: usize) -> Vec<String> {
    (1..=count)
        .map(|ordinal| format!("reference:{ordinal}"))
        .collect()
}

async fn reference_page_state(
    shard: &crate::storage::SqliteGraphStore,
    source_scope: &'static str,
) -> (String, usize, usize, usize, usize) {
    shard
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT progress.stage, progress.completed_page_ordinal,
                            progress.built_count,
                            (SELECT COUNT(*) FROM code_repository_search
                             WHERE source_scope = ?1 AND document_kind = 'reference'),
                            (SELECT COUNT(*) FROM code_repository_search_metadata
                             WHERE source_scope = ?1 AND document_kind = 'reference')
                     FROM code_repository_reference_search_progress progress
                     JOIN code_repository_index_checkpoints checkpoint
                       ON checkpoint.source_scope = progress.source_scope
                      AND checkpoint.state =
                          'finalizing:rebuild_reference_search:v2:build:1'
                     WHERE progress.source_scope = ?1",
                    [source_scope],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("reference page state should load")
}

fn reference(source_scope: &str, ordinal: usize) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        reference_id: format!("reference:{ordinal}"),
        file_id: "file".to_owned(),
        path: "src/lib.rs".to_owned(),
        name: format!("name{ordinal}"),
        kind: "call".to_owned(),
        target_symbol_snapshot_id: None,
        target_hint: Some(format!("Target{ordinal}")),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 2_500,
        confidence_tier: "ambiguous".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 6 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}
