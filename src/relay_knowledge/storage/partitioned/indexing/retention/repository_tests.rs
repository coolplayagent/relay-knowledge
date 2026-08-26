//! Partitioned repository-retention integration tests.

use super::*;

#[tokio::test]
async fn code_index_task_partitioned_catalog_route_holds_scope_slot_until_final_release() {
    let store = partitioned_store("retention-catalog-capacity-reservation");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    store
        .control
        .run(|connection| {
            for index in 0..crate::storage::sqlite::code::MAX_SCOPE_SLOTS_PER_REPOSITORY - 1 {
                connection.execute(
                    "INSERT INTO code_repository_scopes (
                         source_scope, repository_id, resolved_commit_sha, tree_hash,
                         path_filters_json, language_filters_json, indexed_file_count,
                         symbol_count, reference_count, chunk_count, stale, degraded_reason
                     ) VALUES (?1, 'repo', ?2, ?3, '[]', '[]', 0, 0, 0, 0, 0, NULL)",
                    rusqlite::params![
                        format!("scope-filler-{index:03}"),
                        format!("commit-filler-{index:03}"),
                        format!("tree-filler-{index:03}"),
                    ],
                )?;
            }
            Ok(())
        })
        .await
        .expect("published scope fillers should persist");
    store
        .catalog
        .stage_scope("repo".to_owned(), "scope-retiring-route".to_owned())
        .await
        .expect("catalog route should reserve the final scope slot");

    let error = store
        .queue_code_index_task(task_seed("scope-next"))
        .await
        .expect_err("a retained catalog route must keep the next target behind backpressure");
    assert!(matches!(error, StorageError::CapacityExceeded(_)));

    let removed = store
        .catalog
        .remove_scope_route("repo".to_owned(), "scope-retiring-route".to_owned())
        .await
        .expect("final-phase coordinator should release the catalog route");
    assert_eq!(removed, 1);
    let queued = store
        .queue_code_index_task(task_seed("scope-next"))
        .await
        .expect("the released final slot should admit the next target");
    assert_eq!(queued.source_scope, "scope-next");
}

#[tokio::test]
async fn code_index_task_partitioned_retention_removes_retired_scope_catalog_route() {
    let store = partitioned_store("retention-catalog-route");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    publish_snapshot(&store, "scope-old").await;
    publish_snapshot(&store, "scope-active").await;
    assert_eq!(
        store
            .catalog
            .repository_for_scope("scope-old".to_owned())
            .await
            .expect("old catalog route should load")
            .as_deref(),
        Some("repo")
    );

    for _ in 0..160 {
        let pass = store
            .prune_code_repository_scopes(CodeScopeRetentionRequest {
                repository_id: "repo".to_owned(),
                active_scope: "scope-active".to_owned(),
                retain_recent_successful_scopes: 0,
                repository_retention_cutoff_ms: None,
                repository_retention_cutoff_generation: None,
                repository_retention_initial_scope: None,
            })
            .await
            .expect("partitioned retention pass should run");
        if !pass.maintenance_pending {
            break;
        }
    }

    assert!(
        store
            .catalog
            .repository_for_scope("scope-old".to_owned())
            .await
            .expect("retired catalog route should load")
            .is_none()
    );
    assert!(
        store
            .catalog
            .repository_for_scope("scope-active".to_owned())
            .await
            .expect("active catalog route should load")
            .is_some()
    );
}
#[tokio::test]
async fn partitioned_repository_retention_drains_control_and_shard_before_completion() {
    let store = partitioned_store("repository-retention");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    publish_snapshot(&store, "scope-active").await;
    let cutoff_ms = now_millis().saturating_add(1);
    store
        .control
        .run(move |connection| {
            connection.execute(
                "INSERT INTO code_repository_retention_jobs (
                     repository_id, initial_scope, cutoff_ms, phase,
                     created_at_ms, updated_at_ms, last_error
                 ) VALUES ('repo', 'scope-active', ?1, 'retiring_scopes', ?1, ?1, NULL)",
                [cutoff_ms],
            )?;
            Ok(())
        })
        .await
        .expect("repository retention job should insert");

    for _ in 0..160 {
        let pass = store
            .prune_code_repository_scopes(CodeScopeRetentionRequest {
                repository_id: "repo".to_owned(),
                active_scope: "scope-active".to_owned(),
                retain_recent_successful_scopes: 2,
                repository_retention_cutoff_ms: None,
                repository_retention_cutoff_generation: None,
                repository_retention_initial_scope: None,
            })
            .await
            .expect("partitioned repository retention should advance");
        if !pass.maintenance_pending {
            break;
        }
    }

    assert!(
        store
            .catalog
            .repository_for_scope("scope-active".to_owned())
            .await
            .expect("scope route should load")
            .is_none()
    );
    let status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("repository status should load")
        .expect("repository registration should remain");
    assert!(status.last_indexed_scope_id.is_none());
    assert_eq!(status.state, "registered");
    assert!(
        store
            .control
            .code_scope_retention("repo".to_owned())
            .await
            .expect("control retention status should load")
            .repository_retention_job
            .is_none()
    );
}

#[tokio::test]
async fn partitioned_repository_retention_preserves_same_millisecond_republication() {
    let store = partitioned_store("repository-retention-republication");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    publish_snapshot(&store, "scope-active").await;
    store
        .control
        .run(|connection| {
            connection.execute(
                "INSERT INTO code_repository_retention_jobs (
                     repository_id, initial_scope, cutoff_ms,
                     cutoff_publication_generation, phase,
                     created_at_ms, updated_at_ms, last_error
                 ) VALUES ('repo', 'scope-active', 100, 1,
                           'retiring_scopes', 100, 100, NULL)",
                [],
            )?;
            connection.execute(
                "INSERT INTO code_repository_index_tasks (
                     task_id, repository_id, alias, ref_selector,
                     resolved_commit_sha, tree_hash, source_scope,
                     path_filters_json, language_filters_json, mode_json, state,
                     attempt_count, next_retry_at_ms, input_fingerprint,
                     resource_budget_json, payload_json, publication_generation,
                     created_at_ms, updated_at_ms
                 ) VALUES ('republished', 'repo', 'fixture', 'HEAD',
                           'commit-scope-active', 'tree-scope-active', 'scope-active',
                           '[]', '[]', '\"full\"', 'succeeded', 1, 0, 'republished',
                           ?1, '{}', 2, 100, 100)",
                [serde_json::to_string(&CodeIndexResourceBudget::default())
                    .map_err(|error| StorageError::InvalidInput(error.to_string()))?],
            )?;
            Ok(())
        })
        .await
        .expect("same-millisecond republication should insert");

    for _ in 0..160 {
        let pass = store
            .prune_code_repository_scopes(CodeScopeRetentionRequest {
                repository_id: "repo".to_owned(),
                active_scope: "scope-active".to_owned(),
                retain_recent_successful_scopes: 2,
                repository_retention_cutoff_ms: None,
                repository_retention_cutoff_generation: None,
                repository_retention_initial_scope: None,
            })
            .await
            .expect("partitioned repository retention should advance");
        if !pass.maintenance_pending {
            break;
        }
    }

    assert!(
        store
            .catalog
            .repository_for_scope("scope-active".to_owned())
            .await
            .expect("republished scope route should load")
            .is_some()
    );
    let status = store
        .code_repository_status("fixture".to_owned())
        .await
        .expect("repository status should load")
        .expect("repository should remain indexed");
    assert_eq!(
        status.last_indexed_scope_id.as_deref(),
        Some("scope-active")
    );
}

#[tokio::test]
async fn partitioned_repository_retention_stops_after_repository_joins_user_set() {
    let store = partitioned_store("repository-retention-user-set");
    store
        .upsert_code_repository(registration())
        .await
        .expect("repository should register");
    for scope in ["scope-old", "scope-recent", "scope-active"] {
        publish_snapshot(&store, scope).await;
    }
    let shard = store
        .catalog
        .checkpoint_repository_store("repo".to_owned())
        .await
        .expect("repository shard should load")
        .expect("repository shard should exist");
    shard
        .run(|connection| {
            for (index, scope) in ["scope-old", "scope-recent", "scope-active"]
                .into_iter()
                .enumerate()
            {
                connection.execute(
                    "INSERT INTO code_repository_index_tasks (
                         task_id, repository_id, alias, ref_selector,
                         resolved_commit_sha, tree_hash, source_scope,
                         path_filters_json, language_filters_json, mode_json, state,
                         attempt_count, next_retry_at_ms, input_fingerprint,
                         resource_budget_json, payload_json, created_at_ms, updated_at_ms
                     ) VALUES (?1, 'repo', 'fixture', ?2, ?2, ?3, ?4,
                               '[]', '[]', '\"full\"', 'succeeded', 1, 0, ?1,
                               ?5, '{}', ?6, ?6)",
                    rusqlite::params![
                        format!("task-{scope}"),
                        format!("commit-{scope}"),
                        format!("tree-{scope}"),
                        scope,
                        serde_json::to_string(&CodeIndexResourceBudget::default())
                            .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
                        10 + index as u64,
                    ],
                )?;
            }
            Ok(())
        })
        .await
        .expect("shard publication history should insert");
    store
        .control
        .run(|connection| {
            connection.execute(
                "INSERT INTO code_repository_retention_jobs (
                     repository_id, initial_scope, cutoff_ms, phase,
                     created_at_ms, updated_at_ms, last_error
                 ) VALUES ('repo', 'scope-active', 100, 'retiring_scopes', 100, 100, NULL)",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("repository retention job should insert");
    store
        .create_code_repository_set(CodeRepositorySetSeed {
            alias: "protected".to_owned(),
            description: None,
            default_ref_policy_json: "{}".to_owned(),
            now_ms: 101,
        })
        .await
        .expect("user set should create");
    store
        .add_code_repository_set_member(CodeRepositorySetMemberSeed {
            set_alias: "protected".to_owned(),
            repository_id: "repo".to_owned(),
            repository_alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: "commit-scope-active".to_owned(),
            source_scope: "scope-active".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            priority: 0,
        })
        .await
        .expect("repository should join user set");

    for _ in 0..160 {
        let pass = store
            .prune_code_repository_scopes(CodeScopeRetentionRequest {
                repository_id: "repo".to_owned(),
                active_scope: "scope-active".to_owned(),
                retain_recent_successful_scopes: 2,
                repository_retention_cutoff_ms: None,
                repository_retention_cutoff_generation: None,
                repository_retention_initial_scope: None,
            })
            .await
            .expect("partitioned retention should advance");
        if !pass.maintenance_pending {
            break;
        }
    }

    assert!(
        store
            .catalog
            .repository_for_scope("scope-recent".to_owned())
            .await
            .expect("recent route should load")
            .is_some()
    );
    assert!(
        store
            .catalog
            .repository_for_scope("scope-active".to_owned())
            .await
            .expect("active route should load")
            .is_some()
    );
    assert!(
        store
            .control
            .code_scope_retention("repo".to_owned())
            .await
            .expect("control retention status should load")
            .repository_retention_job
            .is_none()
    );
}

async fn publish_snapshot(store: &PartitionedSqliteKnowledgeStore, source_scope: &str) {
    let resolved_commit_sha = format!("commit-{source_scope}");
    let tree_hash = format!("tree-{source_scope}");
    let mut publication = snapshot(source_scope);
    publication.resolved_commit_sha = resolved_commit_sha.clone();
    publication.tree_hash = tree_hash.clone();
    let mut seed = task_seed(source_scope);
    seed.ref_selector = resolved_commit_sha.clone();
    seed.resolved_commit_sha = resolved_commit_sha;
    seed.tree_hash = tree_hash;
    seed.input_fingerprint = format!("partitioned-retention-{source_scope}");
    seed.now_ms = now_millis();
    let queued = store
        .queue_code_index_task(seed)
        .await
        .expect("partitioned retention fixture task should queue");
    let lease_owner = format!("retention-worker-{source_scope}");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: lease_owner.clone(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: now_millis(),
        })
        .await
        .expect("partitioned retention fixture task should claim")
        .expect("partitioned retention fixture task should be runnable");
    let fence = CodeIndexPublicationFence {
        repository_id: running.repository_id.clone(),
        task_id: running.task_id.clone(),
        lease_owner: lease_owner.clone(),
        attempt_count: running.attempt_count,
        generation: running.publication_generation,
    };
    let summary = store
        .apply_code_index_snapshot_with_fence(publication, fence.clone())
        .await
        .expect("partitioned retention fixture snapshot should publish");
    crate::storage::stage_empty_business_projection_with_fence_for_test(
        store,
        summary.repository_id.clone(),
        summary.source_scope.clone(),
        summary.resolved_commit_sha.clone(),
        fence.clone(),
    )
    .await
    .expect("partitioned retention fixture business projection should stage");
    store
        .refresh_software_global_projection_with_fence(summary.source_scope, fence)
        .await
        .expect("partitioned retention fixture projection should publish");
    store
        .complete_code_index_task(CodeIndexTaskCompletion {
            task_id: running.task_id,
            lease_owner,
            attempt_count: running.attempt_count,
            publication_generation: running.publication_generation,
            now_ms: now_millis(),
        })
        .await
        .expect("partitioned retention fixture task should complete");
}
