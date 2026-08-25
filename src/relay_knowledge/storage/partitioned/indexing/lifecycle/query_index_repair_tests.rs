//! Partitioned query-index repair and catalog-handoff recovery.

use crate::{
    domain::code_query_index_repair,
    storage::{
        CodeIndexFinalizationStep, CodeIndexPublicationTarget, CodeIndexTaskClaimRequest,
        CodeRepositoryStore, PartitionedSqliteKnowledgeStore,
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
async fn partitioned_publish_repairs_missing_query_index_before_catalog_handoff() {
    let (store, control_path, paths) =
        partitioned_store_with_paths("partitioned-query-index-repair");
    let source_scope = "scope-partitioned-query-index-repair";
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let snapshot = snapshot(source_scope);
    let session = session_from_snapshot(&snapshot);
    let queued = store
        .queue_code_index_task(task_seed(source_scope))
        .await
        .expect("fenced full task should queue");
    let task = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "repair-worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("fenced claim should run")
        .expect("fenced task should claim");
    let fence = publication_fence(&task, "repair-worker");
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced session should begin");
    store
        .apply_code_index_batch_with_fence(batch_from_snapshot(snapshot), fence.clone())
        .await
        .expect("fenced batch should persist");
    store
        .finalize_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced facts should finalize");
    store
        .refresh_software_global_projection_with_fence(source_scope.to_owned(), fence.clone())
        .await
        .expect("software projection should reach raw partitioned handoff");
    let shard = store
        .catalog
        .checkpoint_repository_store("repo".to_owned())
        .await
        .expect("staged shard should resolve")
        .expect("staged shard should exist");
    shard
        .run(|connection| {
            connection.execute(
                "DROP INDEX code_repository_imports_scope_path_line_lookup",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("appended query index should be removable for upgrade fixture");
    let target = CodeIndexPublicationTarget {
        task_id: task.task_id.clone(),
        repository_id: task.repository_id.clone(),
        source_scope: task.source_scope.clone(),
        resolved_commit_sha: task.resolved_commit_sha.clone(),
        tree_hash: task.tree_hash.clone(),
        path_filters: task.path_filters.clone(),
        language_filters: task.language_filters.clone(),
    };
    assert!(
        store
            .code_index_publication_receipt(
                task.task_id.clone(),
                task.repository_id.clone(),
                task.source_scope.clone(),
                now_millis(),
            )
            .await
            .expect("the earlier catalog publication should leave a receipt"),
        "the repair gate must override an already durable publication receipt"
    );
    assert!(
        !store
            .reconcile_code_index_publication_with_fence(target.clone(), fence.clone())
            .await
            .expect("missing current query index should remain repairable")
    );
    assert_eq!(
        store
            .code_index_checkpoint(source_scope.to_owned())
            .await
            .expect("public checkpoint should load")
            .expect("public checkpoint should exist")
            .state,
        "finalizing:partitioned_publish"
    );

    let first = store
        .advance_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("one fenced repair quantum should commit");
    let CodeIndexFinalizationStep::Pending { checkpoint_state } = first else {
        panic!("one repair quantum should remain pending");
    };
    assert_eq!(
        checkpoint_state,
        "finalizing:query_index_repair:v3:16:resume:10"
    );
    assert!(code_query_index_repair(&checkpoint_state).is_some());
    drop(shard);
    drop(store);

    let store = PartitionedSqliteKnowledgeStore::open(control_path, paths)
        .expect("partitioned store should reopen");
    let restored = store
        .advance_code_index_session_with_fence(session, fence.clone())
        .await
        .expect("durable repair token should restore raw partitioned handoff");
    assert!(matches!(
        restored,
        CodeIndexFinalizationStep::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:partitioned_publish"
    ));
    assert!(
        store
            .reconcile_code_index_publication_with_fence(target, fence)
            .await
            .expect("complete query plan should publish catalog handoff")
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
}

#[tokio::test]
async fn unfinished_partitioned_receipt_requires_fresh_projection_and_skips_republication() {
    let (store, control_path, _paths) =
        partitioned_store_with_paths("partitioned-stale-projection-repair");
    let source_scope = "scope-partitioned-stale-projection-repair";
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let snapshot = snapshot(source_scope);
    let session = session_from_snapshot(&snapshot);
    let queued = store
        .queue_code_index_task(task_seed(source_scope))
        .await
        .expect("fenced full task should queue");
    let task = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "projection-repair-worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("fenced claim should run")
        .expect("fenced task should claim");
    let fence = publication_fence(&task, "projection-repair-worker");
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced session should begin");
    store
        .apply_code_index_batch_with_fence(batch_from_snapshot(snapshot), fence.clone())
        .await
        .expect("fenced batch should persist");
    store
        .finalize_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced facts should finalize");
    store
        .refresh_software_global_projection_with_fence(source_scope.to_owned(), fence.clone())
        .await
        .expect("initial fenced projection should publish");
    assert_eq!(
        store
            .catalog
            .active_repository_for_scope(source_scope.to_owned())
            .await
            .expect("active catalog route should load")
            .as_deref(),
        Some(task.repository_id.as_str()),
        "the receipt regression requires an already active catalog target"
    );
    assert!(
        store
            .code_index_task(task.task_id.clone())
            .await
            .expect("task should load")
            .expect("task should exist")
            .state
            .is_unfinished(),
        "publication must leave this worker task unfinished until task completion"
    );
    assert!(
        store
            .code_index_publication_receipt(
                task.task_id.clone(),
                task.repository_id.clone(),
                task.source_scope.clone(),
                now_millis(),
            )
            .await
            .expect("published task receipt should load")
    );
    let shard = store
        .catalog
        .checkpoint_repository_store(task.repository_id.clone())
        .await
        .expect("published shard should resolve")
        .expect("published shard should exist");
    shard
        .run(move |connection| {
            let changed = connection.execute(
                "UPDATE software_global_status
                 SET stale = 1, last_error = 'projection schema migration'
                 WHERE source_scope = ?1",
                [source_scope],
            )?;
            if changed != 1 {
                return Err(crate::storage::StorageError::Invariant(
                    "stale projection fixture did not update one status".to_owned(),
                ));
            }
            Ok(())
        })
        .await
        .expect("migration should mark the published projection stale");
    let target = CodeIndexPublicationTarget {
        task_id: task.task_id.clone(),
        repository_id: task.repository_id.clone(),
        source_scope: task.source_scope.clone(),
        resolved_commit_sha: task.resolved_commit_sha.clone(),
        tree_hash: task.tree_hash.clone(),
        path_filters: task.path_filters.clone(),
        language_filters: task.language_filters.clone(),
    };

    assert!(
        !store
            .reconcile_code_index_publication_with_fence(target.clone(), fence.clone())
            .await
            .expect("stale projection should remain repairable despite its receipt")
    );
    let public_checkpoint = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("public repair checkpoint should load")
        .expect("public repair checkpoint should exist");
    assert_eq!(public_checkpoint.state, "completed");
    assert_eq!(
        shard
            .code_index_checkpoint(source_scope.to_owned())
            .await
            .expect("raw repair checkpoint should load")
            .expect("raw repair checkpoint should exist")
            .state,
        "finalizing:partitioned_publish",
        "an active partition must retain its raw catalog-handoff state"
    );
    let materialized = store
        .begin_code_index_session_at_checkpoint_with_fence(
            session,
            Some(public_checkpoint),
            fence.clone(),
        )
        .await
        .expect("worker exact begin should materialize the active public checkpoint");
    assert_eq!(materialized.state, "completed");
    assert_eq!(
        shard
            .code_index_checkpoint(source_scope.to_owned())
            .await
            .expect("materialized raw checkpoint should load")
            .expect("materialized raw checkpoint should exist")
            .state,
        "completed"
    );
    assert_eq!(
        store
            .catalog
            .active_repository_for_scope(source_scope.to_owned())
            .await
            .expect("materialized catalog route should load")
            .as_deref(),
        Some(task.repository_id.as_str()),
        "exact begin must preserve the already active repair route"
    );
    let repaired = store
        .refresh_software_global_projection_with_fence(source_scope.to_owned(), fence.clone())
        .await
        .expect("worker fenced refresh should repair the stale projection");
    assert!(!repaired.status.stale);
    assert_eq!(repaired.status.last_error, None);
    let control_observer = Connection::open(control_path).expect("control observer should open");
    assert_eq!(
        control_observer
            .execute(
                "UPDATE storage_repository_shard_scopes SET updated_at_ms = 101
             WHERE repository_id = ?1 AND source_scope = ?2",
                params![&task.repository_id, source_scope],
            )
            .expect("route marker should become deterministic"),
        1
    );
    assert_eq!(
        control_observer
            .execute(
                "UPDATE storage_repository_shards SET updated_at_ms = 102
             WHERE repository_id = ?1",
                [&task.repository_id],
            )
            .expect("shard marker should become deterministic"),
        1
    );
    assert_eq!(
        control_observer
            .execute(
                "UPDATE code_repository_publication_receipts SET published_at_ms = 103
             WHERE task_id = ?1 AND publication_generation = ?2",
                params![&task.task_id, task.publication_generation],
            )
            .expect("receipt marker should become deterministic"),
        1
    );
    let publication_before = catalog_publication_state(
        &control_observer,
        &task.task_id,
        &task.repository_id,
        source_scope,
    );

    assert!(
        store
            .reconcile_code_index_publication_with_fence(target, fence)
            .await
            .expect("fresh repaired receipt should recover after all eligibility gates")
    );
    let publication_after = catalog_publication_state(
        &control_observer,
        &task.task_id,
        &task.repository_id,
        source_scope,
    );
    assert_eq!(
        publication_after, publication_before,
        "an eligible durable receipt must preserve exact catalog publication state"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct CatalogPublicationState {
    route_state: String,
    route_staged_task_id: Option<String>,
    route_updated_at_ms: i64,
    shard_state: String,
    shard_updated_at_ms: i64,
    receipt_generation: i64,
    receipt_published_at_ms: i64,
    repository_scope: Option<String>,
    repository_commit: Option<String>,
    repository_tree: Option<String>,
    repository_state: String,
    repository_stale: i64,
    scope_commit: String,
    scope_tree: String,
    scope_stale: i64,
}

fn catalog_publication_state(
    connection: &Connection,
    task_id: &str,
    repository_id: &str,
    source_scope: &str,
) -> CatalogPublicationState {
    connection
        .query_row(
            "SELECT route.state, route.staged_task_id, route.updated_at_ms,
                    shard.state, shard.updated_at_ms,
                    receipt.publication_generation, receipt.published_at_ms,
                    repository.last_indexed_scope_id, repository.last_indexed_commit,
                    repository.tree_hash, repository.state, repository.stale,
                    scope.resolved_commit_sha, scope.tree_hash, scope.stale
             FROM storage_repository_shard_scopes route
             JOIN storage_repository_shards shard
               ON shard.repository_id = route.repository_id
             JOIN code_repository_publication_receipts receipt
               ON receipt.repository_id = route.repository_id
              AND receipt.source_scope = route.source_scope
              AND receipt.task_id = ?1
             JOIN code_repositories repository
               ON repository.repository_id = route.repository_id
             JOIN code_repository_scopes scope
               ON scope.repository_id = route.repository_id
              AND scope.source_scope = route.source_scope
             WHERE route.repository_id = ?2 AND route.source_scope = ?3",
            params![task_id, repository_id, source_scope],
            |row| {
                Ok(CatalogPublicationState {
                    route_state: row.get(0)?,
                    route_staged_task_id: row.get(1)?,
                    route_updated_at_ms: row.get(2)?,
                    shard_state: row.get(3)?,
                    shard_updated_at_ms: row.get(4)?,
                    receipt_generation: row.get(5)?,
                    receipt_published_at_ms: row.get(6)?,
                    repository_scope: row.get(7)?,
                    repository_commit: row.get(8)?,
                    repository_tree: row.get(9)?,
                    repository_state: row.get(10)?,
                    repository_stale: row.get(11)?,
                    scope_commit: row.get(12)?,
                    scope_tree: row.get(13)?,
                    scope_stale: row.get(14)?,
                })
            },
        )
        .expect("exact catalog publication state should load")
}

#[tokio::test]
async fn inactive_completed_shard_reopens_exactly_for_repair_before_catalog_publication() {
    let (store, control_path, paths) =
        partitioned_store_with_paths("partitioned-completed-query-index-repair");
    let source_scope = "scope-partitioned-completed-query-index-repair";
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    let snapshot = snapshot(source_scope);
    let session = session_from_snapshot(&snapshot);
    let queued = store
        .queue_code_index_task(task_seed(source_scope))
        .await
        .expect("fenced full task should queue");
    let task = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "completed-repair-worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("fenced claim should run")
        .expect("fenced task should claim");
    let fence = publication_fence(&task, "completed-repair-worker");
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced session should begin");
    store
        .apply_code_index_batch_with_fence(batch_from_snapshot(snapshot), fence.clone())
        .await
        .expect("fenced batch should persist");
    store
        .finalize_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced facts should finalize");
    let shard = store
        .catalog
        .checkpoint_repository_store("repo".to_owned())
        .await
        .expect("staged shard should resolve")
        .expect("staged shard should exist");
    shard
        .refresh_software_global_projection_with_fence(source_scope.to_owned(), fence.clone())
        .await
        .expect("shard software projection should reach partitioned handoff");
    assert!(
        store
            .catalog
            .active_repository_for_scope(source_scope.to_owned())
            .await
            .expect("catalog activity should load")
            .is_none(),
        "direct shard projection must leave the catalog target staged"
    );
    shard
        .run(move |connection| {
            let changed = connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'completed'
                 WHERE source_scope = ?1 AND state = 'finalizing:partitioned_publish'",
                [source_scope],
            )?;
            if changed != 1 {
                return Err(crate::storage::StorageError::Invariant(
                    "legacy inactive completed fixture did not replace one raw checkpoint"
                        .to_owned(),
                ));
            }
            connection.execute(
                "DROP INDEX code_repository_imports_scope_path_line_lookup",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("legacy inactive completed fixture should persist");
    let public_checkpoint = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("public checkpoint should load")
        .expect("public checkpoint should exist");
    assert_eq!(public_checkpoint.state, "finalizing:partitioned_publish");

    let reopened = store
        .begin_code_index_session_at_checkpoint_with_fence(
            session.clone(),
            Some(public_checkpoint),
            fence.clone(),
        )
        .await
        .expect("exact fenced begin should reopen only the staged completed repair candidate");
    assert_eq!(reopened.state, "finalizing:partitioned_publish");
    let first = store
        .advance_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("one repair DDL and cursor CAS should commit");
    assert!(matches!(
        first,
        CodeIndexFinalizationStep::Pending { checkpoint_state }
            if code_query_index_repair(&checkpoint_state).is_some()
    ));
    drop(shard);
    drop(store);

    let store = PartitionedSqliteKnowledgeStore::open(control_path, paths)
        .expect("partitioned store should reopen");
    let restored = store
        .advance_code_index_session_with_fence(session, fence.clone())
        .await
        .expect("reopen should restore the exact raw partitioned handoff");
    assert!(matches!(
        restored,
        CodeIndexFinalizationStep::Pending { checkpoint_state }
            if checkpoint_state == "finalizing:partitioned_publish"
    ));
    let target = CodeIndexPublicationTarget {
        task_id: task.task_id.clone(),
        repository_id: task.repository_id.clone(),
        source_scope: task.source_scope.clone(),
        resolved_commit_sha: task.resolved_commit_sha.clone(),
        tree_hash: task.tree_hash.clone(),
        path_filters: task.path_filters.clone(),
        language_filters: task.language_filters.clone(),
    };
    assert!(
        store
            .reconcile_code_index_publication_with_fence(target, fence)
            .await
            .expect("repaired staged shard should publish")
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
}
