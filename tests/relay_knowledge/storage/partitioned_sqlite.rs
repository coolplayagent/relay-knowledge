use std::{fs, path::PathBuf};

use relay_knowledge::storage::{
    CodeIndexTaskClaimRequest, CodeRepositorySetRefreshTaskSeed, CodeRepositoryStore,
    PartitionedSqliteKnowledgeStore, SqliteGraphStore,
};

use super::support::*;

#[tokio::test]
async fn partitioned_sqlite_routes_repository_code_facts_to_shards() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let store =
        PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone()).expect("store opens");

    let alpha = registration("repo-alpha", "alpha");
    let beta = registration("repo-beta", "beta");
    store
        .upsert_code_repository(alpha)
        .await
        .expect("alpha registers");
    store
        .upsert_code_repository(beta)
        .await
        .expect("beta registers");
    assert!(paths.repository_shard_database_file("repo-alpha").exists());
    assert!(paths.repository_shard_database_file("repo-beta").exists());

    let alpha_scope = "scope-alpha".to_owned();
    let beta_scope = "scope-beta".to_owned();
    store
        .apply_code_index_snapshot(snapshot("repo-alpha", &alpha_scope, "alpha needle"))
        .await
        .expect("alpha indexes");
    store
        .apply_code_index_snapshot(snapshot("repo-beta", &beta_scope, "beta needle"))
        .await
        .expect("beta indexes");

    let alpha_hits = store
        .search_code_scope(
            alpha_scope.clone(),
            retrieval_request("alpha", "alpha needle"),
        )
        .await
        .expect("alpha query succeeds");
    let beta_hits = store
        .search_code_scope(beta_scope.clone(), retrieval_request("beta", "beta needle"))
        .await
        .expect("beta query succeeds");
    let totals = store
        .code_repository_totals()
        .await
        .expect("totals aggregate");

    assert_eq!(control_code_file_count(&control_path), 0);
    assert!(
        alpha_hits
            .iter()
            .all(|hit| hit.repository_id == "repo-alpha")
    );
    assert!(beta_hits.iter().all(|hit| hit.repository_id == "repo-beta"));
    assert_eq!(totals.repository_count, 2);
    assert_eq!(totals.indexed_file_count, 2);
    assert_eq!(totals.chunk_count, 2);
}

#[tokio::test]
async fn partitioned_sqlite_preserves_legacy_single_db_code_facts_without_catalog() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    let legacy_scope = "scope-legacy".to_owned();
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");
    single
        .apply_code_index_snapshot(snapshot("repo-legacy", &legacy_scope, "legacy needle"))
        .await
        .expect("legacy facts persist");

    let store =
        PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone()).expect("store opens");
    let scoped_hits = store
        .search_code_scope(
            legacy_scope.clone(),
            retrieval_request_for_ref("legacy", "repo-legacy-commit", "legacy needle"),
        )
        .await
        .expect("scoped legacy query succeeds");
    let repository_hits = store
        .search_code(retrieval_request_for_ref(
            "legacy",
            "repo-legacy-commit",
            "legacy needle",
        ))
        .await
        .expect("repository legacy query succeeds");
    let totals = store
        .code_repository_totals()
        .await
        .expect("legacy totals remain visible");

    assert_eq!(scoped_hits.len(), 1);
    assert_eq!(repository_hits.len(), 1);
    assert_eq!(totals.repository_count, 1);
    assert_eq!(totals.indexed_file_count, 1);
    assert_eq!(totals.chunk_count, 1);
    assert!(!paths.repository_shard_database_file("repo-legacy").exists());
}

#[tokio::test]
async fn partitioned_sqlite_reregister_legacy_repo_imports_latest_scope_before_activation() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");
    single
        .apply_code_index_snapshot(snapshot("repo-legacy", "scope-legacy", "legacy needle"))
        .await
        .expect("legacy facts persist");

    let store =
        PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone()).expect("store opens");
    store
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy registration migrates latest scope");
    let hits = store
        .search_code(retrieval_request_for_ref(
            "legacy",
            "repo-legacy-commit",
            "legacy needle",
        ))
        .await
        .expect("legacy ref is queryable from imported shard");
    let totals = store
        .code_repository_totals()
        .await
        .expect("imported totals aggregate");

    assert_eq!(hits.len(), 1);
    assert_eq!(totals.repository_count, 1);
    assert_eq!(totals.indexed_file_count, 1);
    assert!(paths.repository_shard_database_file("repo-legacy").exists());
}

#[tokio::test]
async fn partitioned_sqlite_queue_task_does_not_publish_scope_before_shard_exists() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");

    let store =
        PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone()).expect("store opens");
    store
        .queue_code_index_task(code_index_task_seed(
            "repo-legacy",
            "legacy",
            "fingerprint-queued",
            "scope-queued",
            1,
        ))
        .await
        .expect("task queues");
    let fingerprints = store
        .code_file_fingerprints_for_scope("scope-queued".to_owned())
        .await
        .expect("unpublished queued scope falls back to control");

    assert!(fingerprints.is_empty());
    assert!(!paths.repository_shard_database_file("repo-legacy").exists());
}

#[tokio::test]
async fn partitioned_sqlite_totals_include_legacy_control_rows_when_shards_exist() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");
    single
        .apply_code_index_snapshot(snapshot("repo-legacy", "scope-legacy", "legacy needle"))
        .await
        .expect("legacy facts persist");

    let store =
        PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone()).expect("store opens");
    store
        .upsert_code_repository(registration("repo-alpha", "alpha"))
        .await
        .expect("alpha registers");
    store
        .apply_code_index_snapshot(snapshot("repo-alpha", "scope-alpha", "alpha needle"))
        .await
        .expect("alpha indexes");
    let totals = store
        .code_repository_totals()
        .await
        .expect("mixed totals aggregate");

    assert_eq!(totals.repository_count, 2);
    assert_eq!(totals.indexed_file_count, 2);
    assert_eq!(totals.chunk_count, 2);
    assert!(!paths.repository_shard_database_file("repo-legacy").exists());
}

#[tokio::test]
async fn partitioned_sqlite_totals_do_not_double_count_migrated_control_rows() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");
    single
        .apply_code_index_snapshot(snapshot("repo-legacy", "scope-legacy", "legacy needle"))
        .await
        .expect("legacy facts persist");

    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store opens");
    store
        .apply_code_index_snapshot(snapshot_with_commit(
            "repo-legacy",
            "scope-legacy-next",
            "repo-legacy-next-commit",
            "legacy next needle",
        ))
        .await
        .expect("partitioned reindex succeeds");
    let totals = store
        .code_repository_totals()
        .await
        .expect("migrated totals aggregate");

    assert_eq!(totals.repository_count, 1);
    assert_eq!(totals.indexed_file_count, 1);
    assert_eq!(totals.chunk_count, 1);
}

#[tokio::test]
async fn partitioned_sqlite_repository_search_falls_back_to_control_for_legacy_ref_after_sharding()
{
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");
    single
        .apply_code_index_snapshot(snapshot("repo-legacy", "scope-legacy", "legacy needle"))
        .await
        .expect("legacy facts persist");

    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store opens");
    store
        .apply_code_index_snapshot(snapshot_with_commit(
            "repo-legacy",
            "scope-legacy-next",
            "repo-legacy-next-commit",
            "legacy next needle",
        ))
        .await
        .expect("partitioned reindex succeeds");
    let hits = store
        .search_code(retrieval_request_for_ref(
            "legacy",
            "repo-legacy-commit",
            "legacy needle",
        ))
        .await
        .expect("legacy ref falls back to control");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].scope_id, "scope-legacy");
}

#[tokio::test]
async fn partitioned_sqlite_scope_status_falls_back_to_control_for_legacy_ref_after_sharding() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");
    single
        .apply_code_index_snapshot(snapshot("repo-legacy", "scope-legacy", "legacy needle"))
        .await
        .expect("legacy facts persist");

    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store opens");
    store
        .apply_code_index_snapshot(snapshot_with_commit(
            "repo-legacy",
            "scope-legacy-next",
            "repo-legacy-next-commit",
            "legacy next needle",
        ))
        .await
        .expect("partitioned reindex succeeds");
    let status = store
        .code_repository_scope_status(
            "legacy".to_owned(),
            "repo-legacy-commit".to_owned(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("scope status lookup succeeds")
        .expect("legacy scope status falls back to control");

    assert_eq!(
        status.last_indexed_scope_id.as_deref(),
        Some("scope-legacy")
    );
    assert_eq!(status.alias, "legacy");
}

#[tokio::test]
async fn partitioned_sqlite_checkpoint_full_index_keeps_legacy_report_visible_before_finalize() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");
    single
        .apply_code_index_snapshot(snapshot("repo-legacy", "scope-legacy", "legacy needle"))
        .await
        .expect("legacy facts persist");

    let store =
        PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone()).expect("store opens");
    let staged = snapshot_with_commit(
        "repo-legacy",
        "scope-legacy-next",
        "repo-legacy-next-commit",
        "legacy next needle",
    );
    let session = session_for_snapshot(&staged);
    store
        .begin_code_index_session(session)
        .await
        .expect("checkpoint session begins");
    store
        .apply_code_index_batch(batch_from_snapshot(staged))
        .await
        .expect("checkpoint batch is staged");

    let report = store
        .code_repository_report("legacy".to_owned())
        .await
        .expect("legacy report remains visible from imported scope");

    assert_eq!(report.repository_id, "repo-legacy");
    assert_eq!(report.indexed_file_count, 1);
    assert!(paths.repository_shard_database_file("repo-legacy").exists());
}

#[tokio::test]
async fn partitioned_sqlite_recomputes_shard_paths_after_runtime_move() {
    let original_root = unique_temp_dir("partitioned-original");
    let original_paths = runtime_paths_for_root(&original_root);
    let original_control_path = original_paths.database_file();
    {
        let store =
            PartitionedSqliteKnowledgeStore::open(&original_control_path, original_paths.clone())
                .expect("store opens");
        store
            .upsert_code_repository(registration("repo-alpha", "alpha"))
            .await
            .expect("alpha registers");
        store
            .apply_code_index_snapshot(snapshot("repo-alpha", "scope-alpha", "alpha needle"))
            .await
            .expect("alpha indexes");
    }
    let locator = catalog_shard_locator(&original_control_path, "repo-alpha");
    let moved_root = unique_temp_dir("partitioned-moved");
    fs::rename(&original_root, &moved_root).expect("runtime root moves");
    let moved_paths = runtime_paths_for_root(&moved_root);
    let moved_store =
        PartitionedSqliteKnowledgeStore::open(moved_paths.database_file(), moved_paths)
            .expect("moved store opens");
    let hits = moved_store
        .search_code(retrieval_request_for_ref(
            "alpha",
            "repo-alpha-commit",
            "alpha needle",
        ))
        .await
        .expect("moved shard remains queryable");

    assert!(!PathBuf::from(locator).is_absolute());
    assert_eq!(hits.len(), 1);
}

#[tokio::test]
async fn partitioned_sqlite_imports_legacy_software_projection_with_scope() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");
    single
        .apply_code_index_snapshot(snapshot("repo-legacy", "scope-legacy", "legacy needle"))
        .await
        .expect("legacy facts persist");
    single
        .refresh_software_global_projection("scope-legacy".to_owned())
        .await
        .expect("legacy projection refreshes");

    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store opens");
    store
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy scope imports into shard");
    let projection = store
        .software_global_projection_for_scope(
            "scope-legacy".to_owned(),
            software_request("legacy", "repo-legacy-commit"),
        )
        .await
        .expect("imported projection remains visible");

    assert!(!projection.status.stale);
    assert_eq!(projection.status.last_error, None);
    assert_eq!(projection.status.file_count, 1);
}

#[tokio::test]
async fn partitioned_sqlite_checkpoint_lookup_falls_back_to_legacy_control_scope() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");
    let legacy = snapshot("repo-legacy", "scope-legacy", "legacy needle");
    let session = session_for_snapshot(&legacy);
    single
        .begin_code_index_session(session.clone())
        .await
        .expect("legacy session begins");
    single
        .apply_code_index_batch(batch_from_snapshot(legacy))
        .await
        .expect("legacy batch applies");
    single
        .finalize_code_index_session(session)
        .await
        .expect("legacy session finalizes");

    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store opens");
    store
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy scope imports into shard");
    let checkpoint = store
        .code_index_checkpoint("scope-legacy".to_owned())
        .await
        .expect("checkpoint lookup succeeds")
        .expect("legacy checkpoint is visible");

    assert_eq!(checkpoint.repository_id, "repo-legacy");
    assert_eq!(checkpoint.source_scope, "scope-legacy");
}

#[tokio::test]
async fn partitioned_sqlite_prune_removes_legacy_control_scopes_after_sharding() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");
    single
        .apply_code_index_snapshot(snapshot("repo-legacy", "scope-legacy", "legacy needle"))
        .await
        .expect("legacy facts persist");

    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store opens");
    store
        .apply_code_index_snapshot(snapshot_with_commit(
            "repo-legacy",
            "scope-active",
            "repo-legacy-active-commit",
            "active needle",
        ))
        .await
        .expect("partitioned reindex succeeds");
    let retention = store
        .prune_code_repository_scopes(relay_knowledge::storage::CodeScopeRetentionRequest {
            repository_id: "repo-legacy".to_owned(),
            active_scope: "scope-active".to_owned(),
            retain_recent_successful_scopes: 0,
        })
        .await
        .expect("retention succeeds");

    assert!(retention.pruned_scopes.contains(&"scope-legacy".to_owned()));
    assert_eq!(control_code_file_count(&control_path), 0);
}

#[tokio::test]
async fn partitioned_sqlite_incremental_snapshot_reports_missing_active_shard() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let shard_path = paths.repository_shard_database_file("repo-alpha");
    {
        let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone())
            .expect("store opens");
        store
            .upsert_code_repository(registration("repo-alpha", "alpha"))
            .await
            .expect("alpha registers");
        store
            .apply_code_index_snapshot(snapshot("repo-alpha", "scope-alpha", "alpha needle"))
            .await
            .expect("alpha indexes");
    }
    fs::remove_file(&shard_path).expect("shard file is removed");
    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store reopens");
    let error = store
        .apply_code_index_snapshot(incremental_snapshot(
            "repo-alpha",
            "scope-alpha-next",
            "repo-alpha-commit",
            "alpha next needle",
        ))
        .await
        .expect_err("missing active shard rejects incremental writes");

    assert!(error.to_string().contains("repository shard"));
    assert!(error.to_string().contains("missing"));
    assert!(!shard_path.exists());
}

#[tokio::test]
async fn partitioned_sqlite_totals_report_missing_active_shards() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let shard_path = paths.repository_shard_database_file("repo-alpha");
    {
        let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone())
            .expect("store opens");
        store
            .upsert_code_repository(registration("repo-alpha", "alpha"))
            .await
            .expect("alpha registers");
        store
            .apply_code_index_snapshot(snapshot("repo-alpha", "scope-alpha", "alpha needle"))
            .await
            .expect("alpha indexes");
    }
    fs::remove_file(&shard_path).expect("shard file is removed");
    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store reopens");

    let error = store
        .code_repository_totals()
        .await
        .expect_err("missing shard should be reported");

    assert!(error.to_string().contains("repository shard"));
    assert!(error.to_string().contains("missing"));
}

#[tokio::test]
async fn partitioned_sqlite_topology_snapshot_reports_missing_active_shards() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let shard_path = paths.repository_shard_database_file("repo-alpha");
    {
        let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone())
            .expect("store opens");
        store
            .upsert_code_repository(registration("repo-alpha", "alpha"))
            .await
            .expect("alpha registers");
        store
            .apply_code_index_snapshot(snapshot("repo-alpha", "scope-alpha", "alpha needle"))
            .await
            .expect("alpha indexes");
    }
    fs::remove_file(&shard_path).expect("shard file is removed");
    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store reopens");
    let snapshot = store
        .topology_snapshot()
        .await
        .expect("topology diagnostics should load");

    assert_eq!(snapshot.shards.len(), 1);
    assert_eq!(snapshot.shards[0].repository_id, "repo-alpha");
    assert_eq!(snapshot.shards[0].state, "active");
    assert_eq!(snapshot.shards[0].source_scope_count, 1);
    assert!(!snapshot.shards[0].exists);
    assert!(snapshot.shards[0].resolved_path.contains("repositories"));
}

#[tokio::test]
async fn partitioned_sqlite_remove_missing_shard_does_not_delete_control_repository() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let shard_path = paths.repository_shard_database_file("repo-alpha");
    {
        let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone())
            .expect("store opens");
        store
            .upsert_code_repository(registration("repo-alpha", "alpha"))
            .await
            .expect("alpha registers");
        store
            .apply_code_index_snapshot(snapshot("repo-alpha", "scope-alpha", "alpha needle"))
            .await
            .expect("alpha indexes");
    }
    fs::remove_file(&shard_path).expect("shard file is removed");
    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store reopens");
    let error = store
        .remove_code_repository("alpha".to_owned(), 2)
        .await
        .expect_err("missing shard rejects removal before control commit");
    let control = SqliteGraphStore::open(&control_path).expect("control opens");
    let status = control
        .code_repository_status("alpha".to_owned())
        .await
        .expect("control status loads");

    assert!(error.to_string().contains("repository shard"));
    assert!(status.is_some());
}

#[tokio::test]
async fn partitioned_sqlite_migrates_legacy_base_scope_before_incremental_snapshot() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let single = SqliteGraphStore::open(&control_path).expect("single store opens");
    single
        .upsert_code_repository(registration("repo-legacy", "legacy"))
        .await
        .expect("legacy repository registers");
    single
        .apply_code_index_snapshot(snapshot("repo-legacy", "scope-legacy", "legacy needle"))
        .await
        .expect("legacy base facts persist");

    let store =
        PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone()).expect("store opens");
    store
        .apply_code_index_snapshot(incremental_snapshot(
            "repo-legacy",
            "scope-legacy-next",
            "repo-legacy-commit",
            "legacy next needle",
        ))
        .await
        .expect("incremental snapshot migrates base scope");
    let hits = store
        .search_code_scope(
            "scope-legacy-next".to_owned(),
            retrieval_request_for_ref("legacy", "repo-legacy-next-commit", "legacy next needle"),
        )
        .await
        .expect("incremental scope is queryable");

    assert_eq!(hits.len(), 1);
    assert!(paths.repository_shard_database_file("repo-legacy").exists());
}

#[tokio::test]
async fn partitioned_sqlite_prune_retains_control_task_scopes() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store opens");
    store
        .upsert_code_repository(registration("repo-alpha", "alpha"))
        .await
        .expect("alpha registers");
    store
        .apply_code_index_snapshot(snapshot_with_commit(
            "repo-alpha",
            "scope-old",
            "repo-alpha-old-commit",
            "old retained needle",
        ))
        .await
        .expect("old scope indexes");
    store
        .apply_code_index_snapshot(snapshot_with_commit(
            "repo-alpha",
            "scope-active",
            "repo-alpha-active-commit",
            "active needle",
        ))
        .await
        .expect("active scope indexes");
    store
        .queue_code_index_task(code_index_task_seed(
            "repo-alpha",
            "alpha",
            "fingerprint-old",
            "scope-old",
            1,
        ))
        .await
        .expect("control task queues");

    let retention = store
        .prune_code_repository_scopes(relay_knowledge::storage::CodeScopeRetentionRequest {
            repository_id: "repo-alpha".to_owned(),
            active_scope: "scope-active".to_owned(),
            retain_recent_successful_scopes: 0,
        })
        .await
        .expect("retention succeeds");
    let old_hits = store
        .search_code_scope(
            "scope-old".to_owned(),
            retrieval_request_for_ref("alpha", "repo-alpha-old-commit", "old retained needle"),
        )
        .await
        .expect("control task scope remains queryable");

    assert!(retention.retained_scopes.contains(&"scope-old".to_owned()));
    assert_eq!(old_hits.len(), 1);
}

#[tokio::test]
async fn partitioned_sqlite_remove_keeps_shard_when_control_rejects_running_task() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let shard_path = paths.repository_shard_database_file("repo-alpha");
    let store =
        PartitionedSqliteKnowledgeStore::open(&control_path, paths.clone()).expect("store opens");
    let source_scope = "scope-alpha".to_owned();
    store
        .upsert_code_repository(registration("repo-alpha", "alpha"))
        .await
        .expect("alpha registers");
    store
        .apply_code_index_snapshot(snapshot("repo-alpha", &source_scope, "alpha needle"))
        .await
        .expect("alpha indexes");
    let task = store
        .queue_code_index_task(code_index_task_seed(
            "repo-alpha",
            "alpha",
            "fingerprint-alpha",
            "scope-alpha-next",
            10,
        ))
        .await
        .expect("index task queues");
    store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(task.task_id.clone()),
            lease_owner: "partitioned-test-worker".to_owned(),
            lease_duration_ms: 1_000,
            max_attempts: 3,
            now_ms: 11,
        })
        .await
        .expect("claim succeeds")
        .expect("task is claimed");

    let error = store
        .remove_code_repository("alpha".to_owned(), 12)
        .await
        .expect_err("running task rejects removal");
    let hits = store
        .search_code_scope(
            source_scope.clone(),
            retrieval_request("alpha", "alpha needle"),
        )
        .await
        .expect("shard facts remain queryable");

    assert!(error.to_string().contains(&task.task_id));
    assert_eq!(hits.len(), 1);
    assert!(shard_path.exists());
}

#[tokio::test]
async fn partitioned_sqlite_rejects_repository_set_refresh_tasks() {
    let paths = runtime_paths();
    let control_path = paths.database_file();
    let store = PartitionedSqliteKnowledgeStore::open(&control_path, paths).expect("store opens");

    let error = store
        .queue_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskSeed {
            set_id: "set-workspace".to_owned(),
            set_alias: "workspace".to_owned(),
            input_fingerprint: "set-fingerprint".to_owned(),
            now_ms: 1,
        })
        .await
        .expect_err("partitioned topology rejects unsupported refresh tasks");

    assert!(error.to_string().contains("single_sqlite"));
    assert!(error.to_string().contains("workspace"));
}
