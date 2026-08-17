use rusqlite::params;

use super as tasks;
use crate::{
    domain::{CodeIndexMode, CodeIndexResourceBudget, CodeRepositoryRegistration},
    storage::{
        CodeIndexTaskSeed, CodeRepositoryStore, CodeScopeRetentionRequest, SqliteGraphStore,
        sqlite::code::workspace,
    },
};

#[tokio::test]
async fn retention_prunes_auto_workspace_members_without_pruning_user_set_members() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-active", "scope-auto", "scope-user"] {
                insert_scope(connection, scope)?;
            }
            connection.execute(
                "
                UPDATE code_repositories
                SET last_indexed_scope_id = 'scope-active',
                    last_indexed_commit = 'commit-active',
                    tree_hash = 'tree-active'
                WHERE repository_id = 'repo'
                ",
                [],
            )?;
            insert_set_member(connection, "user-set", "workspace", "scope-user")?;
            insert_set_member(
                connection,
                &workspace::workspace_set_id("repo"),
                "repo-auto-workspace",
                "scope-auto",
            )?;
            insert_set_member(
                connection,
                &workspace::workspace_set_id("repo"),
                "repo-auto-workspace",
                "scope-active",
            )?;
            insert_cross_edge(connection, "outgoing", "scope-auto", Some("scope-active"))?;
            insert_cross_edge(connection, "incoming", "scope-active", Some("scope-auto"))?;
            Ok(())
        })
        .await
        .expect("fixtures should insert");

    let pruned = drain_retention(&store, "scope-active", 0).await;

    assert_eq!(pruned.pruned_scopes, ["scope-auto"]);
    assert!(pruned.retained_scopes.contains(&"scope-user".to_owned()));
    let stale_edge_count = store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM code_repository_cross_edges
                     WHERE from_source_scope = 'scope-auto' OR to_source_scope = 'scope-auto'",
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("stale workspace edges should query");
    assert_eq!(stale_edge_count, 0);
}

#[tokio::test]
async fn retention_status_bounds_scope_lists_and_reports_truncation() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for index in 0..=64 {
                insert_scope(connection, &format!("scope-{index:03}"))?;
            }
            Ok(())
        })
        .await
        .expect("scope backlog should insert");

    let status = store
        .code_scope_retention("repo".to_owned())
        .await
        .expect("bounded retention status should query");

    assert!(status.scope_listing_truncated);
    assert_eq!(status.prunable_scopes.len(), 64);
    assert_eq!(status.prunable_scope_count, 64);
    assert!(status.maintenance_pending);
}

#[tokio::test]
async fn repository_retention_uses_phased_gc_and_preserves_concurrent_work() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-old", "scope-new", "scope-queued"] {
                insert_scope(connection, scope)?;
            }
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-old',
                     last_indexed_commit = 'commit-old',
                     tree_hash = 'tree-old', state = 'fresh', stale = 0
                 WHERE repository_id = 'repo'",
                [],
            )?;
            insert_terminal_task(connection, "old", "scope-old", "succeeded", 10)?;
            insert_terminal_task(connection, "new", "scope-new", "succeeded", 200)?;
            insert_terminal_task(connection, "queued", "scope-queued", "queued", 90)?;
            insert_repository_retention_job(connection, "scope-old", 100)?;
            Ok(())
        })
        .await
        .expect("repository retention fixtures should insert");

    let first_pass = retention_pass(&store, "scope-old", 2).await;
    assert!(first_pass.pruned_scopes.is_empty());
    assert_eq!(first_pass.retiring_job_count, 1);
    assert_eq!(first_pass.retiring_jobs[0].source_scope, "scope-old");
    assert!(first_pass.repository_retention_job.is_some());
    let (old_scope_count, active_scope, alias, queued_task_count) = store
        .run(|connection| {
            Ok((
                connection.query_row(
                    "SELECT COUNT(*) FROM code_repository_scopes
                     WHERE source_scope = 'scope-old' AND retiring = 1",
                    [],
                    |row| row.get::<_, usize>(0),
                )?,
                connection.query_row(
                    "SELECT last_indexed_scope_id FROM code_repositories
                     WHERE repository_id = 'repo'",
                    [],
                    |row| row.get::<_, Option<String>>(0),
                )?,
                connection.query_row(
                    "SELECT alias FROM code_repositories WHERE repository_id = 'repo'",
                    [],
                    |row| row.get::<_, String>(0),
                )?,
                connection.query_row(
                    "SELECT COUNT(*) FROM code_repository_index_tasks
                     WHERE source_scope = 'scope-queued' AND state = 'queued'",
                    [],
                    |row| row.get::<_, usize>(0),
                )?,
            ))
        })
        .await
        .expect("first repository retention phase should query");
    assert_eq!(old_scope_count, 1);
    assert_eq!(active_scope, None);
    assert_eq!(alias, "fixture");
    assert_eq!(queued_task_count, 1);

    let completed = drain_retention(&store, "", 2).await;
    assert!(completed.pruned_scopes.contains(&"scope-old".to_owned()));
    let (old_scope_count, new_scope_count, queued_scope_count, parent_job_count) = store
        .run(|connection| {
            Ok((
                scope_count(connection, "scope-old")?,
                scope_count(connection, "scope-new")?,
                scope_count(connection, "scope-queued")?,
                connection.query_row(
                    "SELECT COUNT(*) FROM code_repository_retention_jobs
                     WHERE repository_id = 'repo'",
                    [],
                    |row| row.get::<_, usize>(0),
                )?,
            ))
        })
        .await
        .expect("completed repository retention should query");
    assert_eq!(old_scope_count, 0);
    assert_eq!(new_scope_count, 1);
    assert_eq!(queued_scope_count, 1);
    assert_eq!(parent_job_count, 0);
}

#[tokio::test]
async fn repository_retention_preserves_initial_scope_republished_after_cutoff() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-initial", "scope-stale", "scope-base"] {
                insert_scope(connection, scope)?;
            }
            update_scope_commit(connection, "scope-base", "base-commit", "base-tree")?;
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-initial',
                     last_indexed_commit = 'commit-initial', tree_hash = 'tree-initial'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            insert_terminal_task(connection, "initial", "scope-initial", "succeeded", 50)?;
            insert_successful_incremental_task(
                connection,
                "republish",
                "scope-initial",
                "base-commit",
                "commit-initial",
                90,
            )?;
            connection.execute(
                "UPDATE code_repository_index_tasks
                 SET state = CASE task_id
                         WHEN 'task-republish' THEN 'running' ELSE state END,
                     publication_generation = CASE task_id
                         WHEN 'task-initial' THEN 1 ELSE publication_generation END",
                [],
            )?;
            insert_repository_retention_job(connection, "scope-initial", 100)?;
            connection.execute(
                "UPDATE code_repository_retention_jobs
                 SET cutoff_publication_generation = 1
                 WHERE repository_id = 'repo'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("concurrent republish fixtures should insert");

    let running = retention_pass(&store, "scope-initial", 2).await;
    assert!(
        running
            .retained_scopes
            .contains(&"scope-initial".to_owned())
    );
    assert!(running.retained_scopes.contains(&"scope-base".to_owned()));
    assert!(
        running
            .retiring_jobs
            .iter()
            .all(|job| !matches!(job.source_scope.as_str(), "scope-initial" | "scope-base"))
    );
    assert!(running.repository_retention_job.is_some());

    store
        .run(|connection| {
            connection.execute(
                "UPDATE code_repository_index_tasks
                 SET state = 'succeeded', publication_generation = 2, updated_at_ms = 200
                 WHERE task_id = 'task-republish'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("republished task should complete");

    let published = retention_pass(&store, "scope-initial", 2).await;

    assert!(
        published
            .retained_scopes
            .contains(&"scope-initial".to_owned())
    );
    assert!(published.retained_scopes.contains(&"scope-base".to_owned()));
    assert!(
        published
            .retiring_jobs
            .iter()
            .all(|job| !matches!(job.source_scope.as_str(), "scope-initial" | "scope-base"))
    );
    let initial_scope_retiring = store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT retiring FROM code_repository_scopes
                     WHERE source_scope = 'scope-initial'",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(crate::storage::StorageError::from)
        })
        .await
        .expect("republished scope should query");
    assert!(!initial_scope_retiring);
}

#[tokio::test]
async fn repository_retention_stops_when_repository_joins_a_user_set() {
    let store = registered_store().await;
    store
        .run(|connection| {
            insert_scope(connection, "scope-active")?;
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-active',
                     last_indexed_commit = 'commit-active', tree_hash = 'tree-active'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            insert_terminal_task(connection, "active", "scope-active", "succeeded", 10)?;
            insert_repository_retention_job(connection, "scope-active", 100)?;
            insert_set_member(connection, "user-set", "team", "scope-active")?;
            Ok(())
        })
        .await
        .expect("user-set repository retention fixtures should insert");

    let pass = retention_pass(&store, "scope-active", 2).await;

    assert!(!pass.maintenance_pending);
    assert!(pass.repository_retention_job.is_none());
    assert!(pass.retiring_jobs.is_empty());
    let (scope_count, parent_job_count) = store
        .run(|connection| {
            Ok((
                scope_count(connection, "scope-active")?,
                connection.query_row(
                    "SELECT COUNT(*) FROM code_repository_retention_jobs
                     WHERE repository_id = 'repo'",
                    [],
                    |row| row.get::<_, usize>(0),
                )?,
            ))
        })
        .await
        .expect("protected repository should query");
    assert_eq!(scope_count, 1);
    assert_eq!(parent_job_count, 0);
}

#[tokio::test]
async fn repository_retention_protects_same_millisecond_incremental_base() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-old", "scope-new", "scope-base"] {
                insert_scope(connection, scope)?;
            }
            update_scope_commit(connection, "scope-base", "base-commit", "base-tree")?;
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-new',
                     last_indexed_commit = 'head-commit', tree_hash = 'head-tree'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            insert_successful_incremental_task(
                connection,
                "same-millisecond",
                "scope-new",
                "base-commit",
                "head-commit",
                100,
            )?;
            insert_repository_retention_job(connection, "scope-old", 100)?;
            Ok(())
        })
        .await
        .expect("same-millisecond incremental fixtures should insert");

    let pass = retention_pass(&store, "scope-old", 2).await;

    assert!(pass.retained_scopes.contains(&"scope-new".to_owned()));
    assert!(pass.retained_scopes.contains(&"scope-base".to_owned()));
    assert_eq!(pass.retiring_jobs[0].source_scope, "scope-old");
}

#[tokio::test]
async fn repository_retention_deduplicates_publication_sources_before_bounding() {
    let store = registered_store().await;
    store
        .run(|connection| {
            insert_scope(connection, "scope-old")?;
            insert_terminal_task(connection, "old", "scope-old", "succeeded", 10)?;
            for index in 0..33 {
                let scope = format!("scope-new-{index:02}");
                insert_scope(connection, &scope)?;
                insert_terminal_task(
                    connection,
                    &format!("new-{index:02}"),
                    &scope,
                    "succeeded",
                    200 + index,
                )?;
                insert_checkpoint(connection, &scope, 200 + index)?;
            }
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-new-32',
                     last_indexed_commit = 'commit-scope-new-32',
                     tree_hash = 'tree-scope-new-32'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            insert_repository_retention_job(connection, "scope-old", 100)?;
            Ok(())
        })
        .await
        .expect("duplicate publication fixtures should insert");

    let pass = retention_pass(&store, "scope-old", 2).await;

    assert!(!pass.scope_listing_truncated);
    assert_eq!(pass.retiring_jobs[0].source_scope, "scope-old");
    assert_eq!(pass.retained_scope_count, 33);
}

#[tokio::test]
async fn repository_retention_keeps_parent_when_publication_history_is_incomplete() {
    let store = registered_store().await;
    store
        .run(|connection| {
            insert_scope(connection, "scope-initial")?;
            insert_terminal_task(connection, "initial", "scope-initial", "succeeded", 10)?;
            for index in 0..=64 {
                let scope = format!("scope-new-{index:02}");
                insert_scope(connection, &scope)?;
                insert_terminal_task(
                    connection,
                    &format!("new-{index:02}"),
                    &scope,
                    "succeeded",
                    200 + index,
                )?;
            }
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-new-64',
                     last_indexed_commit = 'commit-scope-new-64',
                     tree_hash = 'tree-scope-new-64'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            insert_repository_retention_job(connection, "scope-initial", 100)?;
            Ok(())
        })
        .await
        .expect("large publication history fixtures should insert");

    let pass = retention_pass(&store, "scope-new-64", 2).await;

    assert!(pass.scope_listing_truncated);
    assert!(pass.maintenance_pending);
    assert!(pass.retiring_jobs.is_empty());
    assert!(pass.repository_retention_job.is_some());
}

#[tokio::test]
async fn repository_retention_protects_all_higher_generations_in_the_cutoff_millisecond() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-old", "scope-intermediate", "scope-current"] {
                insert_scope(connection, scope)?;
                insert_terminal_task(connection, scope, scope, "succeeded", 100)?;
            }
            connection.execute(
                "UPDATE code_repository_index_tasks
                 SET publication_generation = CASE source_scope
                     WHEN 'scope-old' THEN 1
                     WHEN 'scope-intermediate' THEN 2
                     WHEN 'scope-current' THEN 3
                 END",
                [],
            )?;
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-current',
                     last_indexed_commit = 'commit-scope-current',
                     tree_hash = 'tree-scope-current'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            insert_repository_retention_job(connection, "scope-old", 100)?;
            connection.execute(
                "UPDATE code_repository_retention_jobs
                 SET cutoff_publication_generation = 1
                 WHERE repository_id = 'repo'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("same-millisecond publication fixtures should insert");

    let pass = retention_pass(&store, "scope-old", 2).await;

    assert!(
        pass.retained_scopes
            .contains(&"scope-intermediate".to_owned())
    );
    assert!(pass.retained_scopes.contains(&"scope-current".to_owned()));
    assert_eq!(pass.retiring_jobs[0].source_scope, "scope-old");
}

#[tokio::test]
async fn repository_retention_uses_time_cutoff_for_migrated_zero_generation_jobs() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for (scope, timestamp, generation) in [("scope-old", 10, 1), ("scope-current", 200, 2)]
            {
                insert_scope(connection, scope)?;
                insert_terminal_task(connection, scope, scope, "succeeded", timestamp)?;
                connection.execute(
                    "UPDATE code_repository_index_tasks
                     SET publication_generation = ?2
                     WHERE repository_id = 'repo' AND source_scope = ?1",
                    params![scope, generation],
                )?;
            }
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-current',
                     last_indexed_commit = 'commit-scope-current',
                     tree_hash = 'tree-scope-current'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            insert_repository_retention_job(connection, "scope-old", 100)?;
            Ok(())
        })
        .await
        .expect("migrated repository retention fixtures should insert");

    let pass = retention_pass(&store, "scope-old", 2).await;

    assert!(pass.retained_scopes.contains(&"scope-current".to_owned()));
    assert_eq!(pass.retiring_jobs[0].source_scope, "scope-old");
}

#[tokio::test]
async fn repository_retention_reports_child_gc_phase_and_error() {
    let store = registered_store().await;
    store
        .run(|connection| {
            insert_scope(connection, "scope-old")?;
            insert_repository_retention_job(connection, "scope-old", 100)?;
            Ok(())
        })
        .await
        .expect("repository retention fixtures should insert");
    let scheduled = retention_pass(&store, "scope-old", 2).await;
    assert_eq!(
        scheduled
            .repository_retention_job
            .as_ref()
            .map(|job| job.phase.as_str()),
        Some("scope_gc:workspace_edges")
    );
    store
        .run(|connection| {
            connection.execute(
                "UPDATE code_repository_scope_gc_jobs
                 SET phase = 'invalid-test-phase'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("invalid test phase should persist");

    let failed = retention_pass(&store, "", 2).await;
    let parent = failed
        .repository_retention_job
        .expect("repository retention should remain pending");
    assert_eq!(parent.phase, "scope_gc:invalid-test-phase");
    assert!(parent.last_error.is_some());
    assert!(parent.updated_at_ms > parent.created_at_ms);
}

#[tokio::test]
async fn retention_keeps_active_worktree_overlay_base_scope() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-base", "scope-worktree", "scope-old"] {
                insert_scope(connection, scope)?;
            }
            update_scope_commit(connection, "scope-base", "base-commit", "base-tree")?;
            update_scope_commit(
                connection,
                "scope-worktree",
                "worktree:base-commit:overlay",
                "worktree:overlay",
            )?;
            Ok(())
        })
        .await
        .expect("fixtures should insert");

    let pruned = drain_retention(&store, "scope-worktree", 0).await;

    assert!(
        pruned
            .retained_scopes
            .contains(&"scope-worktree".to_owned())
    );
    assert!(pruned.retained_scopes.contains(&"scope-base".to_owned()));
    assert!(pruned.pruned_scopes.contains(&"scope-old".to_owned()));
}

#[tokio::test]
async fn retention_protects_active_worktree_base_alias_beyond_the_audit_window() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-base", "scope-worktree", "scope-old"] {
                insert_scope(connection, scope)?;
            }
            update_scope_commit(connection, "scope-base", "same-tree-newer", "base-tree")?;
            insert_commit_alias(connection, "base-commit", "scope-base", 1)?;
            for index in 0..(tasks::commit_scope::RETAIN_COMMIT_SCOPE_ALIAS_ROWS + 20) {
                insert_commit_alias(
                    connection,
                    &format!("newer-{index:03}"),
                    "scope-base",
                    index as u64 + 2,
                )?;
            }
            update_scope_commit(
                connection,
                "scope-worktree",
                "worktree:base-commit:overlay",
                "worktree:overlay",
            )?;
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-worktree',
                     last_indexed_commit = 'worktree:base-commit:overlay'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("active worktree alias fixtures should insert");

    let pruned = drain_retention(&store, "scope-worktree", 0).await;
    let base_alias_count = store
        .run(|connection| commit_alias_count_for(connection, "base-commit"))
        .await
        .expect("base alias should query");

    assert!(pruned.retained_scopes.contains(&"scope-base".to_owned()));
    assert!(pruned.pruned_scopes.contains(&"scope-old".to_owned()));
    assert_eq!(base_alias_count, 1);
}

#[tokio::test]
async fn retention_keeps_queued_incremental_target_and_pinned_base_scopes() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-active", "scope-base", "scope-target", "scope-old"] {
                insert_scope(connection, scope)?;
            }
            update_scope_commit(connection, "scope-base", "newer-commit", "base-tree")?;
            insert_commit_alias(connection, "base-commit", "scope-base", 1)?;
            Ok(())
        })
        .await
        .expect("fixtures should insert");
    store
        .queue_code_index_task(task_seed(
            "incremental",
            "scope-target",
            "head-commit",
            CodeIndexMode::incremental("base-commit", "head-commit")
                .expect("incremental mode should validate"),
            10,
        ))
        .await
        .expect("incremental task should queue");

    let pruned = prune(&store, "scope-active").await;

    assert!(pruned.retained_scopes.contains(&"scope-base".to_owned()));
    assert!(pruned.retained_scopes.contains(&"scope-target".to_owned()));
    assert!(pruned.pruned_scopes.contains(&"scope-old".to_owned()));
}

#[tokio::test]
async fn retention_keeps_the_latest_successful_incremental_predecessor() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-active", "scope-base", "scope-old"] {
                insert_scope(connection, scope)?;
            }
            update_scope_commit(connection, "scope-base", "same-tree-newer", "same-tree")?;
            insert_commit_alias(connection, "base-commit", "scope-base", 1)?;
            insert_successful_incremental_task(
                connection,
                "latest-incremental",
                "scope-active",
                "base-commit",
                "head-commit",
                500,
            )?;
            Ok(())
        })
        .await
        .expect("incremental predecessor fixture should insert");

    let pruned = prune(&store, "scope-active").await;

    assert!(pruned.retained_scopes.contains(&"scope-base".to_owned()));
    assert!(pruned.pruned_scopes.contains(&"scope-old".to_owned()));
}

#[tokio::test]
async fn retention_keeps_queued_worktree_overlay_pinned_base_scope() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-active", "scope-base", "scope-old"] {
                insert_scope(connection, scope)?;
            }
            update_scope_commit(connection, "scope-base", "base-commit", "base-tree")?;
            Ok(())
        })
        .await
        .expect("fixtures should insert");
    store
        .queue_code_index_task(task_seed(
            "overlay",
            "scope-overlay-pending",
            "base-commit",
            CodeIndexMode::WorktreeOverlay,
            10,
        ))
        .await
        .expect("worktree overlay task should queue");

    let pruned = prune(&store, "scope-active").await;

    assert!(pruned.retained_scopes.contains(&"scope-base".to_owned()));
    assert!(pruned.pruned_scopes.contains(&"scope-old".to_owned()));
}

#[tokio::test]
async fn retention_orders_recent_scopes_by_successful_task_publication() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-active", "scope-published", "scope-checkpoint"] {
                insert_scope(connection, scope)?;
            }
            insert_checkpoint(connection, "scope-published", 1)?;
            insert_checkpoint(connection, "scope-checkpoint", 200)?;
            insert_terminal_task(connection, "published", "scope-published", "succeeded", 300)?;
            Ok(())
        })
        .await
        .expect("fixtures should insert");

    let pruned = drain_retention(&store, "scope-active", 1).await;

    assert!(
        pruned
            .retained_scopes
            .contains(&"scope-published".to_owned())
    );
    assert!(
        pruned
            .pruned_scopes
            .contains(&"scope-checkpoint".to_owned())
    );
}

#[tokio::test]
async fn code_index_task_retention_orders_equal_timestamps_by_publication_generation() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in [
                "scope-active",
                "scope-generation-one",
                "scope-generation-two",
            ] {
                insert_scope(connection, scope)?;
            }
            insert_terminal_task(
                connection,
                "generation-z",
                "scope-generation-one",
                "succeeded",
                100,
            )?;
            insert_terminal_task(
                connection,
                "generation-a",
                "scope-generation-two",
                "succeeded",
                100,
            )?;
            connection.execute(
                "UPDATE code_repository_index_tasks
                 SET publication_generation = CASE source_scope
                     WHEN 'scope-generation-one' THEN 1 ELSE 2 END",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("generation fixtures should insert");

    let status = retention_pass(&store, "scope-active", 1).await;

    assert!(
        status
            .retained_scopes
            .contains(&"scope-generation-two".to_owned())
    );
    assert_eq!(status.retiring_jobs[0].source_scope, "scope-generation-one");
}

#[tokio::test]
async fn code_index_task_retention_default_window_excludes_failed_partial_scopes() {
    let store = registered_store().await;
    store
        .run(|connection| {
            insert_scope(connection, "scope-active")?;
            insert_scope(connection, "scope-success")?;
            insert_checkpoint(connection, "scope-success", 10)?;
            insert_checkpoint(connection, "scope-partial", 10_000)?;
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'failed', error_message = 'stopped'
                 WHERE source_scope = 'scope-partial'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("partial-scope fixtures should insert");

    let status = retention_pass(&store, "scope-active", 2).await;

    assert!(status.retained_scopes.contains(&"scope-success".to_owned()));
    assert_eq!(status.retiring_jobs[0].source_scope, "scope-partial");
}

#[tokio::test]
async fn retention_prunes_failed_checkpoint_scopes_and_partial_facts() {
    let store = registered_store().await;
    store
        .run(|connection| {
            insert_scope(connection, "scope-active")?;
            insert_checkpoint(connection, "scope-failed", 200)?;
            connection.execute(
                "UPDATE code_repository_index_checkpoints
                 SET state = 'failed', error_message = 'parser stopped'
                 WHERE source_scope = 'scope-failed'",
                [],
            )?;
            connection.execute(
                "
                INSERT INTO code_repository_files (
                    repository_id, source_scope, file_id, path, language_id, blob_hash,
                    byte_len, line_count, parse_status, degraded_reason
                )
                VALUES ('repo', 'scope-failed', 'failed-file', 'src/failed.rs',
                        'rust', 'failed-blob', 1, 1, 'parsed', NULL)
                ",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("failed checkpoint fixture should insert");

    let pruned = prune(&store, "scope-active").await;
    let remaining = store
        .run(|connection| {
            Ok((
                connection.query_row(
                    "SELECT COUNT(*) FROM code_repository_index_checkpoints
                     WHERE source_scope = 'scope-failed'",
                    [],
                    |row| row.get::<_, usize>(0),
                )?,
                connection.query_row(
                    "SELECT COUNT(*) FROM code_repository_files
                     WHERE source_scope = 'scope-failed'",
                    [],
                    |row| row.get::<_, usize>(0),
                )?,
            ))
        })
        .await
        .expect("failed scope rows should query");

    assert!(pruned.pruned_scopes.contains(&"scope-failed".to_owned()));
    assert_eq!(remaining, (0, 0));
}

#[tokio::test]
async fn code_index_task_retention_deletes_old_scopes_in_bounded_replayable_passes() {
    let store = registered_store().await;
    store
        .run(|connection| {
            insert_scope(connection, "scope-active")?;
            for index in 0..4 {
                insert_scope(connection, &format!("scope-old-{index:02}"))?;
            }
            Ok(())
        })
        .await
        .expect("scope backlog should insert");

    let first = retention_pass(&store, "scope-active", 0).await;
    assert_eq!(first.pruned_scope_count, 0);
    assert_eq!(first.prunable_scope_count, 4);
    assert_eq!(first.retiring_job_count, 1);

    let second = retention_pass(&store, "scope-active", 0).await;
    assert_eq!(second.pruned_scope_count, 0);
    let mut status = second;
    for _ in 0..200 {
        status = retention_pass(&store, "scope-active", 0).await;
        if !status.maintenance_pending {
            break;
        }
    }
    assert_eq!(status.prunable_scope_count, 0);
    assert_eq!(status.retiring_job_count, 0);
}

#[tokio::test]
async fn code_index_task_retention_rechecks_current_scope_with_stale_active_request() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-stale-request", "scope-current", "scope-victim"] {
                insert_scope(connection, scope)?;
            }
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-current',
                     last_indexed_commit = 'commit-scope-current',
                     tree_hash = 'tree-scope-current'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("concurrent publication fixture should insert");

    let pass = retention_pass(&store, "scope-stale-request", 0).await;

    assert!(pass.retained_scopes.contains(&"scope-current".to_owned()));
    assert!(
        pass.retained_scopes
            .contains(&"scope-stale-request".to_owned())
    );
    assert_eq!(pass.retiring_jobs[0].source_scope, "scope-victim");
}

#[tokio::test]
async fn code_index_task_retention_rechecks_current_worktree_base_with_stale_request() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in [
                "scope-stale-request",
                "scope-base",
                "scope-current-worktree",
                "scope-victim",
            ] {
                insert_scope(connection, scope)?;
            }
            update_scope_commit(connection, "scope-base", "base-commit", "base-tree")?;
            update_scope_commit(
                connection,
                "scope-current-worktree",
                "worktree:base-commit:overlay",
                "worktree:overlay",
            )?;
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-current-worktree',
                     last_indexed_commit = 'worktree:base-commit:overlay',
                     tree_hash = 'worktree:overlay'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("worktree publication fixture should insert");

    let pass = retention_pass(&store, "scope-stale-request", 0).await;

    assert!(pass.retained_scopes.contains(&"scope-base".to_owned()));
    assert_eq!(pass.retiring_jobs[0].source_scope, "scope-victim");
}

#[tokio::test]
async fn retention_bounds_terminal_task_history_even_when_commits_share_one_tree() {
    let store = registered_store().await;
    store
        .run(|connection| {
            insert_scope(connection, "scope-active")?;
            for index in 0..(tasks::RETAIN_SUCCEEDED_TASK_AUDIT_ROWS + 20) {
                insert_terminal_task(
                    connection,
                    &format!("success-{index:03}"),
                    "scope-active",
                    "succeeded",
                    index as u64,
                )?;
            }
            for index in 0..(tasks::RETAIN_FAILED_TASK_AUDIT_ROWS + 20) {
                insert_terminal_task(
                    connection,
                    &format!("dead-{index:03}"),
                    "scope-active",
                    "dead_letter",
                    index as u64,
                )?;
            }
            Ok(())
        })
        .await
        .expect("task history should insert");
    store
        .queue_code_index_task(task_seed(
            "unfinished",
            "scope-active",
            "commit-active",
            CodeIndexMode::Full,
            1_000,
        ))
        .await
        .expect("unfinished task should queue");

    let pruned = prune(&store, "scope-active").await;
    assert!(pruned.pruned_scopes.is_empty());
    let counts = store
        .run(|connection| {
            Ok((
                task_state_count(connection, "succeeded")?,
                task_state_count(connection, "dead_letter")?,
                task_state_count(connection, "queued")?,
            ))
        })
        .await
        .expect("task counts should load");

    assert_eq!(
        counts,
        (
            tasks::RETAIN_SUCCEEDED_TASK_AUDIT_ROWS,
            tasks::RETAIN_FAILED_TASK_AUDIT_ROWS,
            1,
        )
    );
}

#[tokio::test]
async fn retention_bounds_same_tree_commit_aliases_and_protects_unfinished_refs() {
    let store = registered_store().await;
    store
        .run(|connection| {
            insert_scope(connection, "scope-active")?;
            for index in 0..(tasks::commit_scope::RETAIN_COMMIT_SCOPE_ALIAS_ROWS + 20) {
                insert_commit_alias(
                    connection,
                    &format!("commit-{index:03}"),
                    "scope-active",
                    index as u64,
                )?;
            }
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-active',
                     last_indexed_commit = 'commit-275'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("same-tree aliases should insert");
    store
        .queue_code_index_task(task_seed(
            "protected-base",
            "scope-target",
            "head-commit",
            CodeIndexMode::incremental("commit-000", "head-commit")
                .expect("incremental mode should validate"),
            1_000,
        ))
        .await
        .expect("unfinished task should queue");

    prune(&store, "scope-active").await;
    let (count, protected_count, expired_count) = store
        .run(|connection| {
            Ok((
                commit_alias_count(connection)?,
                commit_alias_count_for(connection, "commit-000")?,
                commit_alias_count_for(connection, "commit-001")?,
            ))
        })
        .await
        .expect("bounded aliases should query");

    assert_eq!(
        count,
        tasks::commit_scope::RETAIN_COMMIT_SCOPE_ALIAS_ROWS + 1
    );
    assert_eq!(protected_count, 1);
    assert_eq!(expired_count, 0);
}

async fn prune(
    store: &SqliteGraphStore,
    active_scope: &str,
) -> crate::domain::CodeScopeRetentionSummary {
    drain_retention(store, active_scope, 0).await
}

async fn drain_retention(
    store: &SqliteGraphStore,
    active_scope: &str,
    retain_recent_successful_scopes: usize,
) -> crate::domain::CodeScopeRetentionSummary {
    let mut aggregate = crate::domain::CodeScopeRetentionSummary::default();
    for pass_index in 0..512 {
        let pass = store
            .run({
                let active_scope = active_scope.to_owned();
                move |connection| {
                    tasks::prune_scopes(
                        connection,
                        CodeScopeRetentionRequest {
                            repository_id: "repo".to_owned(),
                            active_scope,
                            retain_recent_successful_scopes,
                            repository_retention_cutoff_ms: None,
                            repository_retention_cutoff_generation: None,
                            repository_retention_initial_scope: None,
                        },
                    )
                }
            })
            .await
            .expect("prune should run");
        if pass_index == 0 {
            aggregate.repository_id = pass.repository_id.clone();
            aggregate.retained_scope_count = pass.retained_scope_count;
            aggregate.prunable_scope_count = pass.prunable_scope_count;
            aggregate.retained_scopes = pass.retained_scopes.clone();
            aggregate.prunable_scopes = pass.prunable_scopes.clone();
        }
        aggregate.pruned_scopes.extend(pass.pruned_scopes);
        aggregate.pruned_scopes.sort();
        aggregate.pruned_scopes.dedup();
        aggregate.pruned_scope_count = aggregate.pruned_scopes.len();
        aggregate.retiring_job_count = pass.retiring_job_count;
        aggregate.retiring_jobs = pass.retiring_jobs;
        aggregate.maintenance_pending = pass.maintenance_pending;
        if !pass.maintenance_pending {
            return aggregate;
        }
    }
    panic!("bounded retention did not drain within 512 passes")
}

async fn retention_pass(
    store: &SqliteGraphStore,
    active_scope: &str,
    retain_recent_successful_scopes: usize,
) -> crate::domain::CodeScopeRetentionSummary {
    store
        .run({
            let active_scope = active_scope.to_owned();
            move |connection| {
                tasks::prune_scopes(
                    connection,
                    CodeScopeRetentionRequest {
                        repository_id: "repo".to_owned(),
                        active_scope,
                        retain_recent_successful_scopes,
                        repository_retention_cutoff_ms: None,
                        repository_retention_cutoff_generation: None,
                        repository_retention_initial_scope: None,
                    },
                )
            }
        })
        .await
        .expect("retention pass should run")
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
}

fn insert_scope(
    connection: &mut rusqlite::Connection,
    scope: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_scopes (
            source_scope, repository_id, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, indexed_file_count,
            symbol_count, reference_count, chunk_count, stale, degraded_reason
        )
        VALUES (?1, 'repo', ?2, ?3, '[]', '[]', 1, 0, 0, 0, 0, NULL)
        ",
        params![scope, format!("commit-{scope}"), format!("tree-{scope}")],
    )?;
    connection.execute(
        "
        INSERT INTO code_repository_files (
            repository_id, source_scope, file_id, path, language_id, blob_hash,
            byte_len, line_count, parse_status, degraded_reason
        )
        VALUES ('repo', ?1, ?2, 'src/lib.rs', 'rust', 'blob', 1, 1, 'parsed', NULL)
        ",
        params![scope, format!("file-{scope}")],
    )?;
    Ok(())
}

fn update_scope_commit(
    connection: &mut rusqlite::Connection,
    scope: &str,
    commit: &str,
    tree_hash: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        UPDATE code_repository_scopes
        SET resolved_commit_sha = ?2, tree_hash = ?3
        WHERE source_scope = ?1
        ",
        params![scope, commit, tree_hash],
    )?;
    Ok(())
}

fn insert_set_member(
    connection: &mut rusqlite::Connection,
    set_id: &str,
    alias: &str,
    scope: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT OR IGNORE INTO code_repository_sets (
            set_id, alias, description, default_ref_policy_json,
            created_at_ms, updated_at_ms
        )
        VALUES (?1, ?2, NULL, '{}', 1, 1)
        ",
        params![set_id, alias],
    )?;
    connection.execute(
        "
        INSERT INTO code_repository_set_members (
            set_id, repository_id, repository_alias, ref_selector,
            resolved_commit_sha, source_scope, path_filters_json,
            language_filters_json, priority
        )
        VALUES (?1, 'repo', 'repo', ?2, ?2, ?2, '[]', '[]', 0)
        ",
        params![set_id, scope],
    )?;
    Ok(())
}

fn insert_cross_edge(
    connection: &mut rusqlite::Connection,
    edge_id: &str,
    from_scope: &str,
    to_scope: Option<&str>,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_cross_edges (
            edge_id, set_id, from_source_scope, from_repository_id, from_record_kind,
            from_record_id, to_source_scope, to_repository_id, to_record_kind,
            to_record_id, edge_kind, resolution_state, confidence_basis_points,
            confidence_tier, evidence_json, created_at_ms
        )
        VALUES (?1, ?2, ?3, 'repo', 'symbol', ?1, ?4, 'repo', 'symbol', ?1,
                'workspace_reference', 'resolved', 10000, 'exact', '{}', 1)
        ",
        params![
            edge_id,
            workspace::workspace_set_id("repo"),
            from_scope,
            to_scope,
        ],
    )?;
    Ok(())
}

fn task_seed(
    fingerprint: &str,
    source_scope: &str,
    ref_selector: &str,
    mode: CodeIndexMode,
    now_ms: u64,
) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        ref_selector: ref_selector.to_owned(),
        resolved_commit_sha: format!("commit-{source_scope}"),
        tree_hash: "same-tree".to_owned(),
        source_scope: source_scope.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        mode,
        input_fingerprint: fingerprint.to_owned(),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        now_ms,
    }
}

fn insert_checkpoint(
    connection: &mut rusqlite::Connection,
    scope: &str,
    updated_at_ms: u64,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_index_checkpoints (
            source_scope, repository_id, state, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, total_path_count, parsed_file_count,
            committed_file_count, committed_symbol_count, committed_reference_count,
            committed_chunk_count, batch_count, last_path, resource_budget_json,
            updated_at_ms, error_message
        )
        VALUES (?1, 'repo', 'complete', ?2, ?3, '[]', '[]', 1, 1, 1, 0, 0, 0, 1,
                'src/lib.rs', ?4, ?5, NULL)
        ",
        params![
            scope,
            format!("commit-{scope}"),
            format!("tree-{scope}"),
            serde_json::to_string(&CodeIndexResourceBudget::default())
                .map_err(|error| crate::storage::StorageError::InvalidInput(error.to_string()))?,
            updated_at_ms,
        ],
    )?;
    Ok(())
}

fn insert_terminal_task(
    connection: &mut rusqlite::Connection,
    task_suffix: &str,
    scope: &str,
    state: &str,
    updated_at_ms: u64,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_index_tasks (
            task_id, repository_id, alias, ref_selector, resolved_commit_sha, tree_hash,
            source_scope, path_filters_json, language_filters_json, mode_json, state,
            attempt_count, next_retry_at_ms, input_fingerprint, resource_budget_json,
            payload_json, created_at_ms, updated_at_ms
        )
        VALUES (?1, 'repo', 'fixture', ?2, ?2, 'same-tree', ?3, '[]', '[]',
                '\"full\"', ?4, 1, 0, ?1, ?5, '{}', ?6, ?6)
        ",
        params![
            format!("task-{task_suffix}"),
            format!("commit-{task_suffix}"),
            scope,
            state,
            serde_json::to_string(&CodeIndexResourceBudget::default())
                .map_err(|error| crate::storage::StorageError::InvalidInput(error.to_string()))?,
            updated_at_ms,
        ],
    )?;
    Ok(())
}

fn insert_repository_retention_job(
    connection: &mut rusqlite::Connection,
    initial_scope: &str,
    cutoff_ms: u64,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "INSERT INTO code_repository_retention_jobs (
             repository_id, initial_scope, cutoff_ms, phase,
             created_at_ms, updated_at_ms, last_error
         ) VALUES ('repo', ?1, ?2, 'retiring_scopes', ?2, ?2, NULL)",
        params![initial_scope, cutoff_ms],
    )?;
    Ok(())
}

fn scope_count(
    connection: &rusqlite::Connection,
    scope: &str,
) -> Result<usize, crate::storage::StorageError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_scopes WHERE source_scope = ?1",
            params![scope],
            |row| row.get(0),
        )
        .map_err(crate::storage::StorageError::from)
}

fn insert_successful_incremental_task(
    connection: &mut rusqlite::Connection,
    task_suffix: &str,
    scope: &str,
    base_ref: &str,
    head_ref: &str,
    updated_at_ms: u64,
) -> Result<(), crate::storage::StorageError> {
    let mode = CodeIndexMode::incremental(base_ref, head_ref)
        .map_err(|error| crate::storage::StorageError::InvalidInput(error.to_string()))?;
    connection.execute(
        "
        INSERT INTO code_repository_index_tasks (
            task_id, repository_id, alias, ref_selector, resolved_commit_sha, tree_hash,
            source_scope, path_filters_json, language_filters_json, mode_json, state,
            attempt_count, next_retry_at_ms, input_fingerprint, resource_budget_json,
            payload_json, created_at_ms, updated_at_ms
        )
        VALUES (?1, 'repo', 'fixture', ?2, ?2, 'same-tree', ?3, '[]', '[]',
                ?4, 'succeeded', 1, 0, ?1, ?5, '{}', ?6, ?6)
        ",
        params![
            format!("task-{task_suffix}"),
            head_ref,
            scope,
            serde_json::to_string(&mode)
                .map_err(|error| crate::storage::StorageError::InvalidInput(error.to_string()))?,
            serde_json::to_string(&CodeIndexResourceBudget::default())
                .map_err(|error| crate::storage::StorageError::InvalidInput(error.to_string()))?,
            updated_at_ms,
        ],
    )?;
    Ok(())
}

fn insert_commit_alias(
    connection: &mut rusqlite::Connection,
    commit: &str,
    scope: &str,
    published_sequence: u64,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "INSERT INTO code_repository_commit_scopes (
             repository_id, resolved_commit_sha, source_scope, published_sequence
         ) VALUES ('repo', ?1, ?2, ?3)",
        params![commit, scope, published_sequence],
    )?;
    Ok(())
}
fn commit_alias_count(
    connection: &rusqlite::Connection,
) -> Result<usize, crate::storage::StorageError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_commit_scopes WHERE repository_id = 'repo'",
            [],
            |row| row.get(0),
        )
        .map_err(crate::storage::StorageError::from)
}
fn commit_alias_count_for(
    connection: &rusqlite::Connection,
    commit: &str,
) -> Result<usize, crate::storage::StorageError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_commit_scopes
             WHERE repository_id = 'repo' AND resolved_commit_sha = ?1",
            params![commit],
            |row| row.get(0),
        )
        .map_err(crate::storage::StorageError::from)
}

fn task_state_count(
    connection: &rusqlite::Connection,
    state: &str,
) -> Result<usize, crate::storage::StorageError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_index_tasks WHERE state = ?1",
            params![state],
            |row| row.get(0),
        )
        .map_err(crate::storage::StorageError::from)
}
