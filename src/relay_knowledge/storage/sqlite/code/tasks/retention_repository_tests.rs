//! Repository-level retention and publication-fence tests.

use super::*;

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
async fn repository_set_cancellation_preserves_latest_incremental_base_alias() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for scope in ["scope-active", "scope-base"] {
                insert_scope(connection, scope)?;
            }
            update_scope_commit(connection, "scope-base", "same-tree-newer", "same-tree")?;
            insert_commit_alias(connection, "base-commit", "scope-base", 1)?;
            for index in 0..(tasks::commit_scope::RETAIN_COMMIT_SCOPE_ALIAS_ROWS + 20) {
                insert_commit_alias(
                    connection,
                    &format!("newer-{index:03}"),
                    "scope-base",
                    index as u64 + 2,
                )?;
            }
            insert_successful_incremental_task(
                connection,
                "latest-incremental",
                "scope-active",
                "base-commit",
                "head-commit",
                500,
            )?;
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-active',
                     last_indexed_commit = 'head-commit', tree_hash = 'same-tree'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            insert_repository_retention_job(connection, "scope-active", 600)?;
            insert_set_member(connection, "user-set", "workspace", "scope-active")?;
            Ok(())
        })
        .await
        .expect("repository retention cancellation fixtures should insert");

    let pass = retention_pass(&store, "scope-active", 2).await;
    let (base_alias_count, parent_job_count) = store
        .run(|connection| {
            Ok((
                commit_alias_count_for(connection, "base-commit")?,
                connection.query_row(
                    "SELECT COUNT(*) FROM code_repository_retention_jobs
                     WHERE repository_id = 'repo'",
                    [],
                    |row| row.get::<_, usize>(0),
                )?,
            ))
        })
        .await
        .expect("cancelled repository retention state should query");

    assert!(pass.repository_retention_job.is_none());
    assert_eq!(parent_job_count, 0);
    assert_eq!(base_alias_count, 1);
}
