use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexSession,
        CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration, RepositoryCodeChunkRecord,
        RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeReferenceRecord,
        code_snapshot_scope_id,
    },
    storage::{
        CodeIndexFinalizationStep, CodeIndexTaskClaimRequest, CodeIndexTaskSeed,
        CodeRepositoryStore, SqliteGraphStore, StorageError,
    },
};

const REPOSITORY_ID: &str = "durable-clone-e2e";
const ALIAS: &str = "durable-clone-e2e";
const BASE_COMMIT: &str = "base-commit";
const TARGET_COMMIT: &str = "target-commit";
const FILE_COUNT: usize = 40;

#[tokio::test]
async fn direct_full_base_without_a_fact_proof_requests_full_staging_before_target_write() {
    let store = SqliteGraphStore::open_in_memory().expect("database should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                REPOSITORY_ID,
                ALIAS,
                "/tmp/durable-clone-unproven",
                vec![],
                vec![],
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    let base_scope = code_snapshot_scope_id(REPOSITORY_ID, "base-tree", &[], &[]);
    store
        .apply_code_index_snapshot(base_snapshot(&base_scope, "base-tree"))
        .await
        .expect("direct full base should publish without fabricating a proof");
    let target_scope = code_snapshot_scope_id(REPOSITORY_ID, "target-tree", &[], &[]);
    let snapshot = target_snapshot(&target_scope, "target-tree");
    let budget = CodeIndexResourceBudget::new(8, 1_000_000, 32)
        .expect("bounded clone budget should validate");
    let queued = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: REPOSITORY_ID.to_owned(),
            alias: ALIAS.to_owned(),
            ref_selector: TARGET_COMMIT.to_owned(),
            resolved_commit_sha: TARGET_COMMIT.to_owned(),
            tree_hash: "target-tree".to_owned(),
            source_scope: target_scope.clone(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::incremental(BASE_COMMIT, TARGET_COMMIT)
                .expect("incremental mode should validate"),
            input_fingerprint: "unproven-direct-base".to_owned(),
            resource_budget: budget,
            payload_json: "{}".to_owned(),
            now_ms: now_millis(),
        })
        .await
        .expect("incremental task should queue");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "unproven-worker".to_owned(),
            lease_duration_ms: 600_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("task claim should run")
        .expect("incremental task should claim");

    let error = store
        .apply_code_index_snapshot_with_fence(snapshot, fence(&running, "unproven-worker"))
        .await
        .expect_err("an unproven direct base must request the full durable pipeline");
    assert!(
        matches!(error, StorageError::DurableStagingRequired(message) if message.contains("no durable fact-row proof"))
    );
    assert_eq!(
        clone_target_surface(&store, &target_scope).await,
        (0, 0, 0, 0)
    );
}

#[tokio::test]
async fn fenced_clone_reopens_takes_over_and_publishes_one_bounded_page_at_a_time() {
    let database_path = unique_database_path();
    let base_tree = "base-tree";
    let target_tree = "target-tree";
    let base_scope = code_snapshot_scope_id(REPOSITORY_ID, base_tree, &[], &[]);
    let target_scope = code_snapshot_scope_id(REPOSITORY_ID, target_tree, &[], &[]);
    let budget = CodeIndexResourceBudget::new(8, 1_000_000, 32)
        .expect("bounded clone budget should validate");

    let store = SqliteGraphStore::open(&database_path).expect("database should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                REPOSITORY_ID,
                ALIAS,
                "/tmp/durable-clone",
                vec![],
                vec![],
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should register");
    store
        .apply_code_index_snapshot(base_snapshot(&base_scope, base_tree))
        .await
        .expect("base snapshot should publish");
    persist_base_fact_proof(&store, &base_scope, base_tree, budget).await;
    store
        .run(|connection| {
            connection
                .execute("DROP INDEX code_repository_chunks_lookup", [])
                .map_err(StorageError::from)
        })
        .await
        .expect("required query index should drop for the repair fixture");

    let snapshot = target_snapshot(&target_scope, target_tree);
    let queued = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: REPOSITORY_ID.to_owned(),
            alias: ALIAS.to_owned(),
            ref_selector: TARGET_COMMIT.to_owned(),
            resolved_commit_sha: TARGET_COMMIT.to_owned(),
            tree_hash: target_tree.to_owned(),
            source_scope: target_scope.clone(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::incremental(BASE_COMMIT, TARGET_COMMIT)
                .expect("incremental mode should validate"),
            input_fingerprint: "durable-clone-e2e".to_owned(),
            resource_budget: budget,
            payload_json: "{}".to_owned(),
            now_ms: now_millis(),
        })
        .await
        .expect("incremental task should queue");
    let old_task = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "worker-old".to_owned(),
            lease_duration_ms: 600_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("task claim should run")
        .expect("incremental task should claim");
    let old_fence = fence(&old_task, "worker-old");

    assert_pending_step(
        store
            .apply_code_index_snapshot_with_fence(snapshot.clone(), old_fence.clone())
            .await,
        0,
    );
    assert_eq!(clone_owner_counts(&store, &target_scope).await, (1, 1));
    drop(store);

    let store = SqliteGraphStore::open(&database_path).expect("database should reopen");
    let first_page = store
        .apply_code_index_snapshot_with_fence(snapshot.clone(), old_fence.clone())
        .await;
    let first_completed_step = pending_step(first_page);
    assert!(first_completed_step > 0);
    let before_takeover = clone_observation(&store, &target_scope).await;

    expire_task(&store, &old_task.task_id).await;
    let new_task = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(old_task.task_id.clone()),
            lease_owner: "worker-new".to_owned(),
            lease_duration_ms: 600_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("takeover claim should run")
        .expect("expired task should be reclaimed");
    let stale_error = store
        .apply_code_index_snapshot_with_fence(snapshot.clone(), old_fence)
        .await
        .expect_err("the stale attempt must fail before its next page");
    match stale_error {
        StorageError::InvalidInput(message) => {
            assert!(message.contains(&old_task.task_id));
            assert!(message.contains("no longer active"));
        }
        error => panic!("stale fence must fail closed before page admission: {error}"),
    }
    assert_eq!(
        clone_observation(&store, &target_scope).await,
        before_takeover
    );

    let new_fence = fence(&new_task, "worker-new");
    let mut pending_calls = 1usize;
    let handoff_state = loop {
        match store
            .apply_code_index_snapshot_with_fence(snapshot.clone(), new_fence.clone())
            .await
        {
            Ok(_) => panic!("durable clone must expose its committed finalization handoff"),
            Err(StorageError::DurableStagingPending {
                completed_steps,
                max_steps,
            }) => {
                assert!(completed_steps <= max_steps);
                pending_calls += 1;
                assert!(pending_calls < 256, "clone must finish within its proof");
            }
            Err(StorageError::DurableFinalizationRequired { checkpoint_state }) => {
                break checkpoint_state;
            }
            Err(error) => panic!("durable clone should resume after takeover: {error}"),
        }
    };

    assert!(
        pending_calls > 4,
        "fixture must exercise multiple table/search pages"
    );
    assert_eq!(handoff_state, "indexing");
    drop(store);

    let store = SqliteGraphStore::open(&database_path)
        .expect("response-lost finalization handoff should reopen");
    let session = target_session(&target_scope, target_tree, budget);
    let mut finalization_steps = 0usize;
    let mut finalization_states = Vec::new();
    let summary = loop {
        match store
            .advance_code_index_session_with_fence(session.clone(), new_fence.clone())
            .await
            .expect("durable incremental finalization should advance")
        {
            CodeIndexFinalizationStep::Pending { checkpoint_state } => {
                finalization_steps += 1;
                finalization_states.push(checkpoint_state);
                assert!(finalization_steps < 256);
            }
            CodeIndexFinalizationStep::Ready(summary) => break *summary,
        }
    };

    assert!(finalization_steps >= 6);
    let query_index_state = finalization_states.iter().position(|state| {
        state == "finalizing:build_query_indexes"
            || state.starts_with("finalizing:build_query_indexes:v")
    });
    let reference_state = finalization_states
        .iter()
        .position(|state| state.starts_with("finalizing:resolve_references:v1:"));
    assert!(
        query_index_state.is_some_and(|query_index| {
            reference_state.is_some_and(|reference| query_index < reference)
        }),
        "the receipt-owned indexing handoff must repair populated query owners before reference finalization"
    );
    assert_eq!(summary.indexed_file_count, FILE_COUNT);
    assert_eq!(summary.chunk_count, FILE_COUNT);
    assert!(query_index_exists(&store, "code_repository_chunks_lookup").await);
    assert_eq!(summary.reference_count, FILE_COUNT);
    assert_eq!(summary.changed_path_count, 1);
    assert_eq!(summary.skipped_unchanged_count, FILE_COUNT - 1);
    assert_eq!(summary.progress.parsed_file_count, 1);
    assert_eq!(summary.progress.blob_read_count, 1);
    assert_terminal_state(&store, &target_scope).await;

    drop(store);
    remove_database_files(&database_path);
}

fn target_session(
    source_scope: &str,
    tree_hash: &str,
    resource_budget: CodeIndexResourceBudget,
) -> CodeIndexSession {
    CodeIndexSession {
        repository_id: REPOSITORY_ID.to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: Some(BASE_COMMIT.to_owned()),
        resolved_commit_sha: TARGET_COMMIT.to_owned(),
        tree_hash: tree_hash.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count: FILE_COUNT,
        changed_path_count: 1,
        skipped_unchanged_count: FILE_COUNT - 1,
        deleted_paths: Vec::new(),
        changed_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget,
    }
}

async fn persist_base_fact_proof(
    store: &SqliteGraphStore,
    source_scope: &str,
    tree_hash: &str,
    budget: CodeIndexResourceBudget,
) {
    let source_scope = source_scope.to_owned();
    let tree_hash = tree_hash.to_owned();
    let resource_budget_json = serde_json::to_string(&budget).expect("budget should serialize");
    store
        .run(move |connection| {
            let changed = connection.execute(
                "INSERT INTO code_repository_index_checkpoints (
                     source_scope, repository_id, state, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, total_path_count,
                     parsed_file_count, committed_file_count, committed_symbol_count,
                     committed_reference_count, committed_chunk_count, committed_fact_row_count,
                     batch_count, last_path, resource_budget_json, updated_at_ms, error_message
                 ) VALUES (
                     ?1, ?2, 'completed', ?3, ?4, '[]', '[]', ?5,
                     ?5, ?5, 0, ?5, ?5, ?6, 3, ?7, ?8, ?9, NULL
                 )",
                rusqlite::params![
                    source_scope,
                    REPOSITORY_ID,
                    BASE_COMMIT,
                    tree_hash,
                    FILE_COUNT,
                    FILE_COUNT * 3,
                    path(FILE_COUNT - 1),
                    resource_budget_json,
                    now_millis(),
                ],
            )?;
            if changed != 1 {
                return Err(StorageError::Invariant(
                    "base fact proof fixture did not insert exactly once".to_owned(),
                ));
            }
            Ok(())
        })
        .await
        .expect("base fact proof should persist");
}

fn base_snapshot(source_scope: &str, tree_hash: &str) -> CodeIndexSnapshot {
    let files = (0..FILE_COUNT)
        .map(|index| file(source_scope, index, &format!("base-{index}")))
        .collect();
    let chunks = (0..FILE_COUNT)
        .map(|index| chunk(source_scope, index, &format!("basecontent{index}")))
        .collect();
    let references = (0..FILE_COUNT)
        .map(|index| reference(source_scope, index, &format!("base_name_{index}")))
        .collect();
    CodeIndexSnapshot {
        repository_id: REPOSITORY_ID.to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: BASE_COMMIT.to_owned(),
        tree_hash: tree_hash.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: FILE_COUNT,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files,
        symbols: Vec::new(),
        references,
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks,
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn target_snapshot(source_scope: &str, tree_hash: &str) -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: REPOSITORY_ID.to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: Some(BASE_COMMIT.to_owned()),
        resolved_commit_sha: TARGET_COMMIT.to_owned(),
        tree_hash: tree_hash.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: false,
        changed_path_count: 1,
        skipped_unchanged_count: FILE_COUNT - 1,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file(source_scope, 0, "target")],
        symbols: Vec::new(),
        references: vec![reference(source_scope, 0, "target_name")],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![chunk(source_scope, 0, "targetcontent")],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn file(source_scope: &str, index: usize, blob_hash: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: REPOSITORY_ID.to_owned(),
        source_scope: source_scope.to_owned(),
        file_id: format!("file-{index:03}"),
        path: path(index),
        language_id: "rust".to_owned(),
        blob_hash: blob_hash.to_owned(),
        byte_len: 32,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    }
}

fn chunk(source_scope: &str, index: usize, content: &str) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: REPOSITORY_ID.to_owned(),
        source_scope: source_scope.to_owned(),
        chunk_id: format!("chunk-{index:03}"),
        file_id: format!("file-{index:03}"),
        path: path(index),
        language_id: "rust".to_owned(),
        content: content.to_owned(),
        byte_range: range(),
        line_range: range(),
        symbol_snapshot_id: None,
    }
}

fn reference(source_scope: &str, index: usize, name: &str) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: REPOSITORY_ID.to_owned(),
        source_scope: source_scope.to_owned(),
        reference_id: format!("reference-{index:03}"),
        file_id: format!("file-{index:03}"),
        path: path(index),
        name: name.to_owned(),
        kind: "usage".to_owned(),
        target_symbol_snapshot_id: None,
        target_hint: Some(name.to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 0,
        confidence_tier: "unknown".to_owned(),
        byte_range: range(),
        line_range: range(),
    }
}

fn range() -> RepositoryCodeRange {
    RepositoryCodeRange::new("range", 0, 1).expect("range should validate")
}

fn path(index: usize) -> String {
    format!("src/file-{index:03}.rs")
}

fn fence(task: &crate::domain::CodeIndexTaskRecord, owner: &str) -> CodeIndexPublicationFence {
    CodeIndexPublicationFence {
        repository_id: task.repository_id.clone(),
        task_id: task.task_id.clone(),
        lease_owner: owner.to_owned(),
        attempt_count: task.attempt_count,
        generation: task.publication_generation,
    }
}

fn assert_pending_step(
    result: Result<crate::domain::CodeIndexSummary, StorageError>,
    expected_step: usize,
) {
    assert_eq!(pending_step(result), expected_step);
}

fn pending_step(result: Result<crate::domain::CodeIndexSummary, StorageError>) -> usize {
    match result {
        Err(StorageError::DurableStagingPending {
            completed_steps,
            max_steps,
        }) => {
            assert!(completed_steps <= max_steps);
            completed_steps
        }
        Ok(_) => panic!("durable clone unexpectedly finished in one store call"),
        Err(error) => panic!("durable clone returned the wrong state: {error}"),
    }
}

async fn expire_task(store: &SqliteGraphStore, task_id: &str) {
    let task_id = task_id.to_owned();
    let changed = store
        .run(move |connection| {
            connection
                .execute(
                    "UPDATE code_repository_index_tasks
                     SET lease_expires_at_ms = 0
                     WHERE task_id = ?1 AND state = 'running'",
                    [task_id],
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("lease should expire");
    assert_eq!(changed, 1);
}

async fn query_index_exists(store: &SqliteGraphStore, index_name: &str) -> bool {
    let index_name = index_name.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT EXISTS (
                         SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1
                     )",
                    [index_name],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("query-index descriptor should be inspectable")
}

async fn clone_owner_counts(store: &SqliteGraphStore, source_scope: &str) -> (usize, usize) {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                         (SELECT COUNT(*) FROM code_repository_incremental_clone_progress
                          WHERE source_scope = ?1),
                         (SELECT COUNT(*) FROM code_repository_incremental_clone_affected_paths
                          WHERE source_scope = ?1)",
                    [source_scope],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("clone owner counts should load")
}

async fn clone_target_surface(
    store: &SqliteGraphStore,
    source_scope: &str,
) -> (usize, usize, usize, usize) {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                         (SELECT COUNT(*) FROM code_repository_scopes WHERE source_scope = ?1),
                         (SELECT COUNT(*) FROM code_repository_incremental_clone_progress
                          WHERE source_scope = ?1),
                         (SELECT COUNT(*) FROM code_repository_incremental_clone_affected_paths
                          WHERE source_scope = ?1),
                         (SELECT COUNT(*) FROM code_repository_files WHERE source_scope = ?1)",
                    [source_scope],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("target surface should load")
}

async fn clone_observation(store: &SqliteGraphStore, source_scope: &str) -> (usize, usize, usize) {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                         coalesce((SELECT completed_page_ordinal
                                   FROM code_repository_incremental_clone_progress
                                   WHERE source_scope = ?1), 0),
                         (SELECT COUNT(*) FROM code_repository_files WHERE source_scope = ?1),
                         (SELECT COUNT(*) FROM code_repository_chunks WHERE source_scope = ?1)",
                    [source_scope],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("clone observation should load")
}

async fn assert_terminal_state(store: &SqliteGraphStore, source_scope: &str) {
    let source_scope = source_scope.to_owned();
    let state = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                         checkpoint.state,
                         checkpoint.committed_fact_row_count,
                         (SELECT COUNT(*) FROM code_repository_incremental_clone_progress
                          WHERE source_scope = checkpoint.source_scope),
                         (SELECT COUNT(*) FROM code_repository_incremental_clone_affected_paths
                          WHERE source_scope = checkpoint.source_scope),
                         (SELECT COUNT(*) FROM code_repository_files
                          WHERE source_scope = checkpoint.source_scope),
                         (SELECT COUNT(*) FROM code_repository_chunks
                          WHERE source_scope = checkpoint.source_scope),
                         (SELECT blob_hash FROM code_repository_files
                          WHERE source_scope = checkpoint.source_scope AND path = 'src/file-000.rs'),
                         (SELECT content FROM code_repository_chunks
                          WHERE source_scope = checkpoint.source_scope AND path = 'src/file-000.rs'),
                         (SELECT last_indexed_scope_id FROM code_repositories
                          WHERE repository_id = checkpoint.repository_id),
                         (SELECT stale FROM code_repository_scopes
                          WHERE source_scope = checkpoint.source_scope),
                         (SELECT COUNT(*) FROM code_repository_search_metadata
                          WHERE source_scope = checkpoint.source_scope),
                         (SELECT COUNT(*) FROM code_repository_search_metadata metadata
                          JOIN code_repository_search search ON search.rowid = metadata.search_rowid
                          WHERE metadata.source_scope = checkpoint.source_scope
                            AND search.source_scope = checkpoint.source_scope
                            AND search.document_kind = metadata.document_kind
                            AND search.record_id = metadata.record_id
                            AND search.path = metadata.path),
                         (SELECT COUNT(*) FROM code_repository_search
                          WHERE code_repository_search MATCH 'basecontent0'
                            AND source_scope = checkpoint.source_scope),
                         (SELECT COUNT(*) FROM code_repository_search
                          WHERE code_repository_search MATCH 'targetcontent'
                            AND source_scope = checkpoint.source_scope),
                         (SELECT COUNT(*) FROM code_repository_search
                          WHERE code_repository_search MATCH 'basecontent1'
                            AND source_scope = checkpoint.source_scope)
                     FROM code_repository_index_checkpoints checkpoint
                     WHERE checkpoint.source_scope = ?1",
                    [source_scope],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, usize>(1)?,
                            row.get::<_, usize>(2)?,
                            row.get::<_, usize>(3)?,
                            row.get::<_, usize>(4)?,
                            row.get::<_, usize>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, bool>(9)?,
                            row.get::<_, usize>(10)?,
                            row.get::<_, usize>(11)?,
                            row.get::<_, usize>(12)?,
                            row.get::<_, usize>(13)?,
                            row.get::<_, usize>(14)?,
                        ))
                    },
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("terminal clone state should load");

    assert_eq!(state.0, "finalizing:software_projection");
    assert_eq!(state.1, FILE_COUNT * 3 + 3);
    assert_eq!((state.2, state.3), (0, 0));
    assert_eq!((state.4, state.5), (FILE_COUNT, FILE_COUNT));
    assert_eq!(state.6, "target");
    assert_eq!(state.7, "targetcontent");
    assert_eq!(state.8, base_scope_for_target());
    assert!(
        state.9,
        "target must remain nonqueryable before software projection"
    );
    assert_eq!((state.10, state.11), (FILE_COUNT * 2, FILE_COUNT * 2));
    assert_eq!((state.12, state.13, state.14), (0, 1, 1));
}

fn base_scope_for_target() -> String {
    code_snapshot_scope_id(REPOSITORY_ID, "base-tree", &[], &[])
}

fn unique_database_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "relay-knowledge-durable-clone-{}-{}.sqlite3",
        std::process::id(),
        now_millis()
    ))
}

fn remove_database_files(path: &Path) {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        match std::fs::remove_file(candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("temporary clone database should remove: {error}"),
        }
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow epoch")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
