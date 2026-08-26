use std::time::{SystemTime, UNIX_EPOCH};

use super::super::retention::{RETAIN_FAILED_TASK_AUDIT_ROWS, RETAIN_SUCCEEDED_TASK_AUDIT_ROWS};
use super::super::{
    queue_task,
    test_support::{
        claim_task_at_request_time as claim_task, complete_task_at_request_time as complete_task,
        fail_task_at_request_time as fail_task,
    },
};
use super::publication_receipt;
use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexSnapshot,
        CodeIndexTaskRecord, CodeIndexTaskState, CodeMonorepoWorkspaceFormat,
        CodeRepositoryRegistration, CodeWorkspaceDetectionConfig,
        code_snapshot_scope_id_with_workspace_detection,
    },
    storage::{
        CodeIndexPublicationTarget, CodeIndexTaskClaimRequest, CodeIndexTaskCompletion,
        CodeIndexTaskFailure, CodeIndexTaskSeed, CodeRepositoryStore, SqliteGraphStore,
    },
};

#[path = "completion_test_support.rs"]
mod completion_test_support;

use completion_test_support::publish_task_target;

#[tokio::test]
async fn completion_transitions_require_active_lease_and_bound_retry_state() {
    let store = registered_store().await;

    let succeeded = claim(&store, "fp-success", "scope-success", 10).await;
    publish_task_target(&store, &succeeded, false).await;
    let wrong_owner = store
        .run({
            let task_id = succeeded.task_id.clone();
            let publication_generation = succeeded.publication_generation;
            move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id,
                        lease_owner: "other-worker".to_owned(),
                        attempt_count: 1,
                        publication_generation,
                        now_ms: 30,
                    },
                )
            }
        })
        .await
        .expect_err("wrong lease owner should fail");
    assert!(wrong_owner.to_string().contains("active lease"));

    let succeeded = store
        .run({
            let task_id = succeeded.task_id;
            let publication_generation = succeeded.publication_generation;
            move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id,
                        lease_owner: "worker".to_owned(),
                        attempt_count: 1,
                        publication_generation,
                        now_ms: 30,
                    },
                )
            }
        })
        .await
        .expect("active lease should complete");
    assert_eq!(succeeded.state, CodeIndexTaskState::Succeeded);
    assert!(succeeded.lease_owner.is_none());

    let retrying = claim(&store, "fp-retry", "scope-retry", 40).await;
    let retrying = fail(&store, retrying, 3, 50).await;
    assert_eq!(retrying.state, CodeIndexTaskState::Retrying);
    assert_eq!(retrying.next_retry_at_ms, 60);
    assert_eq!(retrying.last_error_kind.as_deref(), Some("fixture"));

    // A retrying predecessor intentionally holds this repository's FIFO lane.
    // Exercise the independent dead-letter transition in an isolated store
    // instead of bypassing that ordering invariant in the fixture.
    let dead_letter_store = registered_store().await;
    let dead_letter = claim(&dead_letter_store, "fp-dead", "scope-dead", 60).await;
    let dead_letter = fail(&dead_letter_store, dead_letter, 1, 70).await;
    assert_eq!(dead_letter.state, CodeIndexTaskState::DeadLetter);
    assert!(dead_letter.lease_owner.is_none());
}

#[tokio::test]
async fn code_index_task_completion_and_dead_letter_keep_audit_history_bounded() {
    let store = registered_store().await;
    for index in 0..RETAIN_SUCCEEDED_TASK_AUDIT_ROWS + 20 {
        let task = claim(
            &store,
            &format!("success-{index}"),
            "success-audit-scope",
            100 + index as u64 * 3,
        )
        .await;
        publish_task_target(&store, &task, false).await;
        store
            .run(move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id: task.task_id,
                        lease_owner: task.lease_owner.expect("task should have lease owner"),
                        attempt_count: task.attempt_count,
                        publication_generation: task.publication_generation,
                        now_ms: task.updated_at_ms.saturating_add(1),
                    },
                )
            })
            .await
            .expect("task should complete");
    }
    for index in 0..RETAIN_FAILED_TASK_AUDIT_ROWS + 20 {
        let task = claim(
            &store,
            &format!("dead-{index}"),
            &format!("dead-scope-{index}"),
            10_000 + index as u64 * 3,
        )
        .await;
        fail(&store, task, 1, 10_002 + index as u64 * 3).await;
    }

    let (succeeded, dead_letter, receipts) = store
        .run(|connection| {
            let count = |state: &str| {
                connection.query_row(
                    "SELECT COUNT(*) FROM code_repository_index_tasks
                     WHERE repository_id = 'repo' AND state = ?1",
                    [state],
                    |row| row.get::<_, usize>(0),
                )
            };
            let receipts = connection.query_row(
                "SELECT COUNT(*) FROM code_repository_publication_receipts",
                [],
                |row| row.get::<_, usize>(0),
            )?;
            Ok((count("succeeded")?, count("dead_letter")?, receipts))
        })
        .await
        .expect("audit counts should load");

    assert!(succeeded <= RETAIN_SUCCEEDED_TASK_AUDIT_ROWS);
    assert!(dead_letter <= RETAIN_FAILED_TASK_AUDIT_ROWS);
    assert!(receipts <= RETAIN_SUCCEEDED_TASK_AUDIT_ROWS);
}

#[tokio::test]
async fn completion_requires_receipt_and_completed_optional_checkpoint() {
    let store = registered_store().await;
    let without_receipt = claim(&store, "no-receipt", "scope-no-receipt", 10).await;
    let error = store
        .run({
            let task = without_receipt.clone();
            move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id: task.task_id,
                        lease_owner: "worker".to_owned(),
                        attempt_count: task.attempt_count,
                        publication_generation: task.publication_generation,
                        now_ms: 20,
                    },
                )
            }
        })
        .await
        .expect_err("missing publication receipt must block completion");
    assert!(error.to_string().contains("durably published"));

    publish_task_target(&store, &without_receipt, true).await;
    let error = store
        .run({
            let task = without_receipt.clone();
            move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id: task.task_id,
                        lease_owner: "worker".to_owned(),
                        attempt_count: task.attempt_count,
                        publication_generation: task.publication_generation,
                        now_ms: 21,
                    },
                )
            }
        })
        .await
        .expect_err("indexing checkpoint must block completion");
    assert!(error.to_string().contains("durably published"));

    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "wrong-repository",
                "wrong-fixture",
                "/tmp/wrong-repository",
                Vec::new(),
                Vec::new(),
            )
            .expect("mismatch repository should validate"),
        )
        .await
        .expect("mismatch repository should persist");
    store
        .run(|connection| {
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET repository_id = 'wrong-repository', state = 'completed'
                 WHERE source_scope = 'scope-no-receipt'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("mismatched checkpoint identity should persist in the fixture");
    let mismatch = store
        .run({
            let task = without_receipt.clone();
            move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id: task.task_id,
                        lease_owner: "worker".to_owned(),
                        attempt_count: task.attempt_count,
                        publication_generation: task.publication_generation,
                        now_ms: 22,
                    },
                )
            }
        })
        .await
        .expect_err("checkpoint repository mismatch must block completion");
    assert!(mismatch.to_string().contains("durably published"));
    store
        .run(|connection| {
            connection.execute(
                "UPDATE code_repository_index_checkpoints SET repository_id = 'repo'
                 WHERE source_scope = 'scope-no-receipt'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("checkpoint identity should be repaired");
    store
        .run({
            let task = without_receipt;
            move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id: task.task_id,
                        lease_owner: "worker".to_owned(),
                        attempt_count: task.attempt_count,
                        publication_generation: task.publication_generation,
                        now_ms: 23,
                    },
                )
            }
        })
        .await
        .expect("completed checkpoint should allow terminal transition");
}

#[tokio::test]
async fn reclaimed_attempt_accepts_the_same_tasks_older_generation_receipt() {
    let modes = [
        CodeIndexMode::Full,
        CodeIndexMode::incremental("base", "head").expect("mode should validate"),
        CodeIndexMode::WorktreeOverlay,
    ];
    for (index, mode) in modes.into_iter().enumerate() {
        let store = registered_store().await;
        let scope = format!("scope-reclaimed-{index}");
        let first =
            claim_with_mode(&store, &format!("fp-reclaimed-{index}"), &scope, 10, mode).await;
        let task_id = first.task_id.clone();
        let old_generation = first.publication_generation;
        publish_task_target(&store, &first, false).await;
        fail(&store, first, 3, 20).await;
        let reclaimed = store
            .run({
                let task_id = task_id.clone();
                move |connection| {
                    claim_task(
                        connection,
                        CodeIndexTaskClaimRequest {
                            task_id: Some(task_id),
                            lease_owner: "worker-reclaimed".to_owned(),
                            lease_duration_ms: 100,
                            max_attempts: 3,
                            now_ms: 31,
                        },
                    )
                }
            })
            .await
            .expect("retry claim should run")
            .expect("retrying task should be reclaimed");
        assert!(reclaimed.publication_generation > old_generation);
        let has_receipt = store
            .run({
                let task_id = task_id.clone();
                let scope = scope.clone();
                move |connection| publication_receipt(connection, &task_id, "repo", &scope, 32)
            })
            .await
            .expect("historical receipt lookup should run");
        assert!(
            has_receipt,
            "mode {index} should reuse its first attempt receipt"
        );
        store
            .run({
                let task_id = task_id.clone();
                move |connection| {
                    complete_task(
                        connection,
                        CodeIndexTaskCompletion {
                            task_id,
                            lease_owner: "worker-reclaimed".to_owned(),
                            attempt_count: reclaimed.attempt_count,
                            publication_generation: reclaimed.publication_generation,
                            now_ms: 32,
                        },
                    )
                }
            })
            .await
            .expect("reclaimed attempt should complete from the durable receipt");
        let generations = store
            .run(move |connection| {
                connection
                    .query_row(
                        "SELECT COUNT(*), MAX(publication_generation)
                         FROM code_repository_publication_receipts WHERE task_id = ?1",
                        [task_id],
                        |row| Ok((row.get::<_, usize>(0)?, row.get::<_, u64>(1)?)),
                    )
                    .map_err(crate::storage::StorageError::from)
            })
            .await
            .expect("receipt audit generations should load");
        assert_eq!(generations, (1, old_generation));
    }
}

#[tokio::test]
async fn stale_old_commit_receipt_cannot_complete_a_retargeted_task() {
    let store = registered_store().await;
    let task = claim(&store, "stale-receipt", "scope-stale-receipt", 10).await;
    publish_task_target(&store, &task, false).await;
    store
        .run({
            let task_id = task.task_id.clone();
            move |connection| {
                connection.execute(
                    "UPDATE code_repository_index_tasks
                     SET resolved_commit_sha = 'commit-retargeted'
                     WHERE task_id = ?1",
                    [&task_id],
                )?;
                Ok(())
            }
        })
        .await
        .expect("retargeted task fixture should persist");

    let has_receipt = store
        .run({
            let task_id = task.task_id.clone();
            let source_scope = task.source_scope.clone();
            move |connection| publication_receipt(connection, &task_id, "repo", &source_scope, 20)
        })
        .await
        .expect("stale receipt lookup should run");
    assert!(!has_receipt);
    let error = store
        .run({
            let task_id = task.task_id.clone();
            move |connection| {
                complete_task(
                    connection,
                    CodeIndexTaskCompletion {
                        task_id,
                        lease_owner: "worker".to_owned(),
                        attempt_count: task.attempt_count,
                        publication_generation: task.publication_generation,
                        now_ms: 20,
                    },
                )
            }
        })
        .await
        .expect_err("an old-commit receipt must not complete a retargeted task");
    assert!(error.to_string().contains("durably published"));
    let state = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT state FROM code_repository_index_tasks WHERE task_id = ?1",
                    [task.task_id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("rolled-back task state should load");
    assert_eq!(state, "running");
}

#[tokio::test]
async fn receipt_requires_a_fresh_software_projection() {
    let store = registered_store().await;
    let task = claim(&store, "stale-software", "scope-stale-software", 10).await;
    publish_task_target(&store, &task, false).await;
    store
        .run({
            let source_scope = task.source_scope.clone();
            move |connection| {
                connection.execute(
                    "UPDATE software_global_status SET stale = 1 WHERE source_scope = ?1",
                    [&source_scope],
                )?;
                Ok(())
            }
        })
        .await
        .expect("stale software fixture should persist");

    let has_receipt = store
        .run({
            let task_id = task.task_id.clone();
            let source_scope = task.source_scope.clone();
            move |connection| publication_receipt(connection, &task_id, "repo", &source_scope, 20)
        })
        .await
        .expect("software-gated receipt lookup should run");
    assert!(!has_receipt);
    let error = store
        .run(move |connection| {
            complete_task(
                connection,
                CodeIndexTaskCompletion {
                    task_id: task.task_id,
                    lease_owner: "worker".to_owned(),
                    attempt_count: task.attempt_count,
                    publication_generation: task.publication_generation,
                    now_ms: 20,
                },
            )
        })
        .await
        .expect_err("stale software must block task completion");
    assert!(error.to_string().contains("durably published"));
}

#[tokio::test]
async fn receipt_accepts_the_current_target_through_a_commit_scope_alias() {
    let store = registered_store().await;
    let task = claim(&store, "aliased-target", "scope-aliased-target", 10).await;
    publish_task_target(&store, &task, false).await;
    store
        .run({
            let task = task.clone();
            move |connection| {
                connection.execute(
                    "UPDATE code_repository_scopes
                     SET resolved_commit_sha = 'commit-original'
                     WHERE source_scope = ?1",
                    [&task.source_scope],
                )?;
                connection.execute(
                    "INSERT INTO code_repository_commit_scopes (
                        repository_id, resolved_commit_sha, source_scope, published_sequence
                     ) VALUES (?1, 'commit-original', ?2, 1), (?1, ?3, ?2, 2)",
                    rusqlite::params![
                        task.repository_id,
                        task.source_scope,
                        task.resolved_commit_sha
                    ],
                )?;
                Ok(())
            }
        })
        .await
        .expect("commit alias fixture should persist");

    let has_receipt = store
        .run({
            let task_id = task.task_id.clone();
            let source_scope = task.source_scope.clone();
            move |connection| publication_receipt(connection, &task_id, "repo", &source_scope, 20)
        })
        .await
        .expect("aliased receipt lookup should run");
    assert!(has_receipt);
    store
        .run(move |connection| {
            complete_task(
                connection,
                CodeIndexTaskCompletion {
                    task_id: task.task_id,
                    lease_owner: "worker".to_owned(),
                    attempt_count: task.attempt_count,
                    publication_generation: task.publication_generation,
                    now_ms: 20,
                },
            )
        })
        .await
        .expect("an aliased current target should complete");
}

#[tokio::test]
async fn receipt_rejects_an_old_active_repository_commit_even_with_a_task_alias() {
    let store = registered_store().await;
    let task = claim(&store, "old-active-commit", "scope-old-active-commit", 10).await;
    publish_task_target(&store, &task, false).await;
    store
        .run({
            let task = task.clone();
            move |connection| {
                connection.execute(
                    "UPDATE code_repository_scopes
                     SET resolved_commit_sha = 'commit-original'
                     WHERE source_scope = ?1",
                    [&task.source_scope],
                )?;
                connection.execute(
                    "UPDATE code_repositories
                     SET last_indexed_commit = 'commit-original'
                     WHERE repository_id = ?1",
                    [&task.repository_id],
                )?;
                connection.execute(
                    "INSERT INTO code_repository_commit_scopes (
                        repository_id, resolved_commit_sha, source_scope, published_sequence
                     ) VALUES (?1, 'commit-original', ?2, 1), (?1, ?3, ?2, 2)",
                    rusqlite::params![
                        task.repository_id,
                        task.source_scope,
                        task.resolved_commit_sha
                    ],
                )?;
                Ok(())
            }
        })
        .await
        .expect("old active commit fixture should persist");

    let has_receipt = store
        .run({
            let task_id = task.task_id.clone();
            let source_scope = task.source_scope.clone();
            move |connection| publication_receipt(connection, &task_id, "repo", &source_scope, 20)
        })
        .await
        .expect("old active commit receipt lookup should run");
    assert!(!has_receipt);
    let error = store
        .run(move |connection| {
            complete_task(
                connection,
                CodeIndexTaskCompletion {
                    task_id: task.task_id,
                    lease_owner: "worker".to_owned(),
                    attempt_count: task.attempt_count,
                    publication_generation: task.publication_generation,
                    now_ms: 20,
                },
            )
        })
        .await
        .expect_err("an old active repository commit must block completion");
    assert!(error.to_string().contains("durably published"));
}

#[tokio::test]
async fn receipt_uses_scope_filters_when_the_task_narrows_registration_filters() {
    let store = registered_store_with_filters(Vec::new(), Vec::new()).await;
    let task = claim(&store, "narrowed-filters", "scope-narrowed-filters", 10).await;
    publish_task_target(&store, &task, false).await;

    let has_receipt = store
        .run({
            let task_id = task.task_id.clone();
            let source_scope = task.source_scope.clone();
            move |connection| publication_receipt(connection, &task_id, "repo", &source_scope, 20)
        })
        .await
        .expect("narrowed-filter receipt lookup should run");
    assert!(has_receipt);
    store
        .run(move |connection| {
            complete_task(
                connection,
                CodeIndexTaskCompletion {
                    task_id: task.task_id,
                    lease_owner: "worker".to_owned(),
                    attempt_count: task.attempt_count,
                    publication_generation: task.publication_generation,
                    now_ms: 20,
                },
            )
        })
        .await
        .expect("scope filters should remain authoritative for completion");
}

#[tokio::test]
async fn a_new_worktree_task_adopts_an_exact_config_aware_publication_without_mutation() {
    let store = registered_store().await;
    let workspace_detection = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![CodeMonorepoWorkspaceFormat::CargoWorkspace],
    };
    let tree_hash = "worktree:0123456789abcdef";
    let source_scope = code_snapshot_scope_id_with_workspace_detection(
        "repo",
        tree_hash,
        &["src".to_owned()],
        &["rust".to_owned()],
        &workspace_detection,
    );
    store
        .apply_code_index_snapshot(CodeIndexSnapshot {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.clone(),
            base_resolved_commit_sha: Some("base".to_owned()),
            resolved_commit_sha: "worktree:base:0123456789abcdef".to_owned(),
            tree_hash: tree_hash.to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
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
        })
        .await
        .expect("exact worktree scope should publish");
    crate::storage::publish_empty_business_projection_for_test(
        &store,
        "repo",
        source_scope.clone(),
        "worktree:base:0123456789abcdef",
    )
    .await
    .expect("exact worktree business projection should publish");
    store
        .refresh_software_global_projection(source_scope.clone())
        .await
        .expect("exact worktree software projection should publish");
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_millis() as u64;
    insert_compatible_base_scope(&store, "base", "base-scope-exact-worktree").await;
    let mut task_seed = seed("new-worktree-exact-target", &source_scope, now_ms);
    task_seed.mode = CodeIndexMode::WorktreeOverlay;
    task_seed.resolved_commit_sha = "worktree:base:0123456789abcdef".to_owned();
    task_seed.tree_hash = tree_hash.to_owned();
    let queued = store
        .run(move |connection| queue_task(connection, task_seed))
        .await
        .expect("exact worktree task should queue");
    let task = store
        .run(move |connection| {
            claim_task(
                connection,
                CodeIndexTaskClaimRequest {
                    task_id: Some(queued.task_id),
                    lease_owner: "worker".to_owned(),
                    lease_duration_ms: 60_000,
                    max_attempts: 3,
                    now_ms: now_ms.saturating_add(1),
                },
            )
        })
        .await
        .expect("exact worktree task should claim")
        .expect("exact worktree task should be claimable");
    let target = CodeIndexPublicationTarget {
        task_id: task.task_id.clone(),
        repository_id: task.repository_id.clone(),
        source_scope: task.source_scope.clone(),
        resolved_commit_sha: "worktree:base:0123456789abcdef".to_owned(),
        tree_hash: tree_hash.to_owned(),
        path_filters: task.path_filters.clone(),
        language_filters: task.language_filters.clone(),
    };
    let fence = CodeIndexPublicationFence {
        repository_id: task.repository_id.clone(),
        task_id: task.task_id.clone(),
        lease_owner: task.lease_owner.clone().expect("task should own a lease"),
        attempt_count: task.attempt_count,
        generation: task.publication_generation,
    };

    assert!(
        store
            .reconcile_code_index_publication_with_fence(target, fence)
            .await
            .expect("exact active target should be adopted")
    );
    assert!(
        store
            .code_index_publication_receipt(
                task.task_id,
                task.repository_id,
                source_scope.clone(),
                now_ms.saturating_add(2),
            )
            .await
            .expect("adoption receipt should load")
    );
    let state = store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT repository.last_indexed_scope_id, repository.stale, scope.stale,
                            software.stale
                     FROM code_repositories repository
                     JOIN code_repository_scopes scope
                       ON scope.source_scope = repository.last_indexed_scope_id
                     JOIN software_global_status software
                       ON software.source_scope = scope.source_scope
                     WHERE repository.repository_id = 'repo'",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, bool>(1)?,
                            row.get::<_, bool>(2)?,
                            row.get::<_, bool>(3)?,
                        ))
                    },
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("adopted publication should remain queryable");
    assert_eq!(state, (source_scope, false, false, false));
}

async fn registered_store() -> SqliteGraphStore {
    registered_store_with_filters(vec!["src".to_owned()], vec!["rust".to_owned()]).await
}

async fn registered_store_with_filters(
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo",
                "fixture",
                "/tmp/repo",
                path_filters,
                language_filters,
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
}

async fn insert_compatible_base_scope(store: &SqliteGraphStore, commit: &str, source_scope: &str) {
    let commit = commit.to_owned();
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection.execute(
                "INSERT INTO code_repository_scopes (
                    source_scope, repository_id, resolved_commit_sha, tree_hash,
                    path_filters_json, language_filters_json, indexed_file_count,
                    symbol_count, reference_count, chunk_count, stale,
                    degraded_reason, retiring
                 ) VALUES (?1, 'repo', ?2, ?3, '[\"src\"]', '[\"rust\"]',
                           1, 0, 0, 0, 0, NULL, 0)",
                rusqlite::params![source_scope, commit, format!("tree-{commit}")],
            )?;
            Ok(())
        })
        .await
        .expect("compatible base scope should persist");
}

async fn claim(
    store: &SqliteGraphStore,
    fingerprint: &str,
    scope: &str,
    now_ms: u64,
) -> CodeIndexTaskRecord {
    claim_with_mode(store, fingerprint, scope, now_ms, CodeIndexMode::Full).await
}

async fn claim_with_mode(
    store: &SqliteGraphStore,
    fingerprint: &str,
    scope: &str,
    now_ms: u64,
    mode: CodeIndexMode,
) -> CodeIndexTaskRecord {
    let mut seed = seed(fingerprint, scope, now_ms);
    match &mode {
        CodeIndexMode::Full => {}
        CodeIndexMode::Incremental { base_ref, .. } => {
            insert_compatible_base_scope(store, base_ref, &format!("base-{fingerprint}")).await;
        }
        CodeIndexMode::WorktreeOverlay => {
            let base_commit = format!("base-{fingerprint}");
            insert_compatible_base_scope(store, &base_commit, &format!("base-scope-{fingerprint}"))
                .await;
            let pending_identity = format!("worktree:pending:{base_commit}");
            seed.ref_selector = base_commit;
            seed.resolved_commit_sha = pending_identity.clone();
            seed.tree_hash = pending_identity;
        }
    }
    seed.mode = mode;
    let queued = store
        .run(move |connection| queue_task(connection, seed))
        .await
        .expect("task should queue");
    store
        .run(move |connection| {
            claim_task(
                connection,
                CodeIndexTaskClaimRequest {
                    task_id: Some(queued.task_id),
                    lease_owner: "worker".to_owned(),
                    lease_duration_ms: 100,
                    max_attempts: 3,
                    now_ms: now_ms.saturating_add(1),
                },
            )
        })
        .await
        .expect("task should claim")
        .expect("queued task should be claimable")
}

async fn fail(
    store: &SqliteGraphStore,
    task: CodeIndexTaskRecord,
    max_attempts: u32,
    now_ms: u64,
) -> CodeIndexTaskRecord {
    store
        .run(move |connection| {
            fail_task(
                connection,
                CodeIndexTaskFailure {
                    task_id: task.task_id,
                    lease_owner: task.lease_owner.expect("task should have lease owner"),
                    attempt_count: task.attempt_count,
                    publication_generation: task.publication_generation,
                    error_kind: "fixture".to_owned(),
                    error_message: "fixture failure".to_owned(),
                    retry_backoff_ms: 10,
                    max_attempts,
                    now_ms,
                },
            )
        })
        .await
        .expect("task failure should persist")
}

fn seed(fingerprint: &str, scope: &str, now_ms: u64) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: format!("commit-{scope}"),
        tree_hash: format!("tree-{scope}"),
        source_scope: scope.to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        mode: CodeIndexMode::Full,
        input_fingerprint: fingerprint.to_owned(),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        now_ms,
    }
}
