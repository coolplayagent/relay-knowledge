//! Safety boundaries for retiring only an attempt's provisional local-source facts.

use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexSession,
        CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexPublicationStore, CodeIndexTaskClaimRequest, CodeIndexTaskSeed,
        CodeIndexTaskStore, RepositoryCatalogStore, SqliteGraphStore, StorageError,
    },
};

async fn fixture() -> (SqliteGraphStore, String, CodeIndexPublicationFence) {
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/source-replan", vec![], vec![])
            .unwrap();
    let store = SqliteGraphStore::open_in_memory().unwrap();
    store.upsert_code_repository(registration).await.unwrap();
    let identity = "filesystem:0123456789abcdef";
    let session = CodeIndexSession {
        repository_id: "repo".into(),
        source_scope: crate::domain::code_snapshot_scope_id("repo", identity, &[], &[]),
        base_resolved_commit_sha: None,
        resolved_commit_sha: identity.into(),
        tree_hash: identity.into(),
        path_filters: vec![],
        language_filters: vec![],
        full_replace: true,
        total_path_count: 1,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: vec![],
        changed_paths: vec![],
        tombstones: vec![],
        workspaces: vec![],
        resource_budget: CodeIndexResourceBudget::new(1, 1024 * 1024, 1000).unwrap(),
    };
    let now = crate::clock::system_now_millis().unwrap();
    let task = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: session.repository_id.clone(),
            alias: "fixture".into(),
            ref_selector: "HEAD".into(),
            resolved_commit_sha: session.resolved_commit_sha.clone(),
            tree_hash: session.tree_hash.clone(),
            source_scope: session.source_scope.clone(),
            path_filters: vec![],
            language_filters: vec![],
            mode: CodeIndexMode::Full,
            input_fingerprint: "safety".into(),
            resource_budget: session.resource_budget,
            payload_json: "{}".into(),
            now_ms: now,
        })
        .await
        .unwrap();
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(task.task_id),
            lease_owner: "worker".into(),
            lease_duration_ms: 60000,
            max_attempts: 3,
            now_ms: now,
        })
        .await
        .unwrap()
        .unwrap();
    let fence = CodeIndexPublicationFence {
        repository_id: running.repository_id,
        task_id: running.task_id,
        lease_owner: "worker".into(),
        attempt_count: running.attempt_count,
        generation: running.publication_generation,
    };
    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .unwrap();
    (store, session.source_scope, fence)
}

#[tokio::test]
async fn source_io_replan_rejects_git_finalizing_completed_and_published_scopes() {
    for alteration in [
        "UPDATE code_repository_index_checkpoints SET resolved_commit_sha = 'git-commit'",
        "UPDATE code_repository_index_checkpoints SET state = 'finalizing:refresh_dependencies'",
        "UPDATE code_repository_index_checkpoints SET state = 'completed'",
        "INSERT INTO code_repository_scopes (source_scope, repository_id, resolved_commit_sha, tree_hash, path_filters_json, language_filters_json, indexed_file_count, symbol_count, reference_count, chunk_count, stale) SELECT source_scope, repository_id, resolved_commit_sha, tree_hash, path_filters_json, language_filters_json, 0, 0, 0, 0, 0 FROM code_repository_index_checkpoints",
    ] {
        let (store, scope, fence) = fixture().await;
        store
            .run(move |connection| {
                connection.execute(alteration, [])?;
                Ok(())
            })
            .await
            .unwrap();
        let error = store
            .cleanup_source_replan_with_fence(scope.clone(), fence, false)
            .await
            .unwrap_err();
        assert!(matches!(error, StorageError::Invariant(_)), "{error}");
        assert!(store.code_index_checkpoint(scope).await.unwrap().is_some());
        let jobs = store
            .run(|connection| {
                Ok(connection.query_row(
                    "SELECT COUNT(*) FROM code_repository_scope_gc_jobs",
                    [],
                    |row| row.get::<_, usize>(0),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(jobs, 0, "protected snapshots must never be marked retiring");
    }
}

#[tokio::test]
async fn source_io_replan_does_not_take_another_task_or_regular_retention_ownership() {
    for owner in [Some("another-task"), None] {
        let (store, scope, fence) = fixture().await;
        assert!(
            !store
                .cleanup_source_replan_with_fence(scope.clone(), fence.clone(), false)
                .await
                .unwrap()
        );
        store
            .run(move |connection| {
                connection.execute(
                    "UPDATE code_repository_scope_gc_jobs SET source_replan_task_id = ?1",
                    [owner],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let error = store
            .cleanup_source_replan_with_fence(scope.clone(), fence, true)
            .await
            .unwrap_err();
        assert!(matches!(error, StorageError::Invariant(_)), "{error}");
        assert_eq!(
            store
                .code_index_checkpoint(scope)
                .await
                .unwrap()
                .unwrap()
                .state,
            "abandoning_source_io"
        );
    }
}

#[tokio::test]
async fn source_io_replan_requires_current_target_and_unexpired_lease() {
    let (store, scope, fence) = fixture().await;
    let error = store
        .cleanup_source_replan_with_fence("another-scope".into(), fence.clone(), false)
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::InvalidInput(_)));
    assert!(
        !store
            .cleanup_source_replan_with_fence(scope.clone(), fence.clone(), false)
            .await
            .unwrap()
    );
    store
        .run(|connection| {
            connection.execute(
                "UPDATE code_repository_index_tasks SET lease_expires_at_ms = 0",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        store
            .cleanup_source_replan_with_fence(scope.clone(), fence, true)
            .await
            .is_err()
    );
    assert_eq!(
        store
            .code_index_checkpoint(scope)
            .await
            .unwrap()
            .unwrap()
            .state,
        "abandoning_source_io"
    );
}

#[tokio::test]
async fn source_io_replan_propagates_storage_failure_without_losing_retirement_progress() {
    let (store, scope, fence) = fixture().await;
    assert!(
        !store
            .cleanup_source_replan_with_fence(scope.clone(), fence.clone(), false)
            .await
            .unwrap()
    );
    store
        .run(|connection| {
            connection.execute(
                "UPDATE code_repository_scope_gc_jobs SET phase = 'business_terms'",
                [],
            )?;
            connection.execute("DROP TABLE business_terms", [])?;
            Ok(())
        })
        .await
        .unwrap();
    let error = store
        .cleanup_source_replan_with_fence(scope, fence, true)
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::Sqlite(_)), "{error}");
    let phase = store
        .run(|connection| {
            Ok(connection.query_row(
                "SELECT phase FROM code_repository_scope_gc_jobs",
                [],
                |row| row.get::<_, String>(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(phase, "business_terms");
}

#[tokio::test]
async fn source_io_replan_without_a_checkpoint_is_an_idempotent_noop() {
    let (store, scope, fence) = fixture().await;
    assert!(
        store
            .cleanup_source_replan_with_fence(scope.clone(), fence.clone(), true)
            .await
            .unwrap()
    );
    store
        .run(|connection| {
            connection.execute("DELETE FROM code_repository_index_checkpoints", [])?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        store
            .cleanup_source_replan_with_fence(scope, fence, false)
            .await
            .unwrap()
    );
}
