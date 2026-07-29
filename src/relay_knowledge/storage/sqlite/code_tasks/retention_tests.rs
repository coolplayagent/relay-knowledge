use rusqlite::params;

use super::*;
use crate::{
    domain::CodeRepositoryRegistration,
    storage::{CodeRepositoryStore, CodeScopeRetentionRequest, SqliteGraphStore},
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
                &code_workspace::workspace_set_id("repo"),
                "repo-auto-workspace",
                "scope-auto",
            )?;
            Ok(())
        })
        .await
        .expect("fixtures should insert");

    let pruned = store
        .run(|connection| {
            code_tasks::prune_scopes(
                connection,
                CodeScopeRetentionRequest {
                    repository_id: "repo".to_owned(),
                    active_scope: "scope-active".to_owned(),
                    retain_recent_successful_scopes: 0,
                },
            )
        })
        .await
        .expect("prune should run");

    assert_eq!(pruned.pruned_scopes, ["scope-auto"]);
    assert!(pruned.retained_scopes.contains(&"scope-user".to_owned()));
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

    let pruned = store
        .run(|connection| {
            code_tasks::prune_scopes(
                connection,
                CodeScopeRetentionRequest {
                    repository_id: "repo".to_owned(),
                    active_scope: "scope-worktree".to_owned(),
                    retain_recent_successful_scopes: 0,
                },
            )
        })
        .await
        .expect("prune should run");

    assert!(
        pruned
            .retained_scopes
            .contains(&"scope-worktree".to_owned())
    );
    assert!(pruned.retained_scopes.contains(&"scope-base".to_owned()));
    assert!(pruned.pruned_scopes.contains(&"scope-old".to_owned()));
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
        INSERT INTO code_repository_sets (
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
