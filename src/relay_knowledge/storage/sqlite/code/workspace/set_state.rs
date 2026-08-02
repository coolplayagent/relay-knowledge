use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::json;

use crate::{domain::CodeRepositorySet, storage::StorageError};

use super::super::super::evidence_identity::stable_id;

pub(in super::super) fn workspace_set_id(repository_id: &str) -> String {
    stable_id("code-auto-workspace-set", repository_id)
}

/// Creates or retrieves the repository set that owns an auto-detected workspace.
pub(super) fn ensure_workspace_set(
    transaction: &Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
    now: u64,
) -> Result<CodeRepositorySet, StorageError> {
    let set_id = workspace_set_id(repository_id);
    let scope = workspace_scope_metadata(transaction, source_scope)?;
    let set = CodeRepositorySet {
        set_id: set_id.clone(),
        alias: auto_workspace_set_alias(repository_id),
        description: Some(format!(
            "Auto-detected monorepo workspace set for {repository_id}"
        )),
        default_ref_policy_json: json!({"default_ref": "HEAD"}).to_string(),
        created_at_ms: now,
        updated_at_ms: now,
    };

    transaction.execute(
        "INSERT INTO code_repository_sets
         (set_id, alias, description, default_ref_policy_json, created_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(set_id) DO UPDATE SET
            alias = excluded.alias,
            description = excluded.description,
            default_ref_policy_json = excluded.default_ref_policy_json,
            updated_at_ms = excluded.updated_at_ms",
        params![
            &set.set_id,
            &set.alias,
            &set.description,
            &set.default_ref_policy_json,
            set.created_at_ms,
            set.updated_at_ms,
        ],
    )?;

    transaction.execute(
        "INSERT OR REPLACE INTO code_repository_set_members
         (set_id, repository_id, repository_alias, ref_selector,
          resolved_commit_sha, source_scope, path_filters_json,
          language_filters_json, priority)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)",
        params![
            set.set_id,
            repository_id,
            repository_id,
            scope.resolved_commit_sha,
            scope.resolved_commit_sha,
            source_scope,
            scope.path_filters_json,
            scope.language_filters_json
        ],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_set_overlay_status WHERE set_id = ?1",
        params![set.set_id],
    )?;

    Ok(set)
}

pub(in super::super) fn clear_auto_workspace_state(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    clear_workspace_state(&transaction, repository_id, source_scope)?;
    transaction.commit()?;
    Ok(())
}

pub(in super::super) fn clear_workspace_state(
    transaction: &Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    let set_id = workspace_set_id(repository_id);
    transaction.execute(
        "DELETE FROM code_repository_cross_edges
         WHERE set_id = ?1 AND from_repository_id = ?2 AND from_source_scope = ?3",
        params![&set_id, repository_id, source_scope],
    )?;
    transaction.execute(
        "DELETE FROM code_workspace_package_mappings
         WHERE set_id = ?1 AND source_scope = ?2",
        params![&set_id, source_scope],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_set_members
         WHERE set_id = ?1 AND repository_id = ?2 AND source_scope = ?3",
        params![&set_id, repository_id, source_scope],
    )?;

    let remaining_members: usize = transaction.query_row(
        "SELECT COUNT(*) FROM code_repository_set_members WHERE set_id = ?1",
        params![&set_id],
        |row| row.get(0),
    )?;
    if remaining_members == 0 {
        transaction.execute(
            "DELETE FROM code_repository_set_overlay_status WHERE set_id = ?1",
            params![&set_id],
        )?;
        transaction.execute(
            "DELETE FROM code_repository_sets WHERE set_id = ?1",
            params![&set_id],
        )?;
    } else {
        refresh_workspace_overlay_status(transaction, &set_id, current_timestamp_ms())?;
    }

    Ok(())
}

pub(super) fn refresh_workspace_overlay_status(
    transaction: &Transaction<'_>,
    set_id: &str,
    now: u64,
) -> Result<(), StorageError> {
    let edge_count = workspace_cross_edge_count(transaction, set_id)?;
    transaction.execute(
        "
        INSERT INTO code_repository_set_overlay_status (
            set_id, state, refreshed_at_ms, edge_count, member_versions_json, degraded_reason
        )
        VALUES (?1, 'fresh', ?2, ?3, ?4, NULL)
        ON CONFLICT(set_id) DO UPDATE SET
            state = excluded.state,
            refreshed_at_ms = excluded.refreshed_at_ms,
            edge_count = excluded.edge_count,
            member_versions_json = excluded.member_versions_json,
            degraded_reason = NULL
        ",
        params![
            set_id,
            now,
            edge_count,
            workspace_member_versions_json(transaction, set_id)?,
        ],
    )?;
    Ok(())
}

fn workspace_cross_edge_count(
    transaction: &Transaction<'_>,
    set_id: &str,
) -> Result<usize, StorageError> {
    transaction
        .query_row(
            "
            SELECT COUNT(*)
            FROM code_repository_cross_edges edge
            WHERE edge.set_id = ?1
              AND EXISTS (
                  SELECT 1
                  FROM code_repository_set_members member
                  WHERE member.set_id = edge.set_id
                    AND member.source_scope = edge.from_source_scope
              )
            ",
            params![set_id],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

fn workspace_member_versions_json(
    transaction: &Transaction<'_>,
    set_id: &str,
) -> Result<String, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT member.repository_id, member.source_scope, member.resolved_commit_sha,
               scope.tree_hash
        FROM code_repository_set_members member
        JOIN code_repository_scopes scope ON scope.source_scope = member.source_scope
        WHERE member.set_id = ?1
        ORDER BY member.repository_alias ASC, member.source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![set_id], |row| {
        Ok(json!({
            "repository_id": row.get::<_, String>(0)?,
            "source_scope": row.get::<_, String>(1)?,
            "resolved_commit_sha": row.get::<_, String>(2)?,
            "tree_hash": row.get::<_, String>(3)?,
            "stale": false,
        }))
    })?;
    let versions = rows.collect::<Result<Vec<_>, _>>()?;
    serde_json::to_string(&versions).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn auto_workspace_set_alias(repository_id: &str) -> String {
    format!("{repository_id}-auto-workspace")
}

struct WorkspaceScopeMetadata {
    resolved_commit_sha: String,
    path_filters_json: String,
    language_filters_json: String,
}

fn workspace_scope_metadata(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<WorkspaceScopeMetadata, StorageError> {
    transaction
        .query_row(
            "
            SELECT resolved_commit_sha, path_filters_json, language_filters_json
            FROM code_repository_scopes
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| {
                Ok(WorkspaceScopeMetadata {
                    resolved_commit_sha: row.get(0)?,
                    path_filters_json: row.get(1)?,
                    language_filters_json: row.get(2)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "workspace source scope '{source_scope}' is not published"
            ))
        })
}

pub(super) fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
#[path = "set_state_tests.rs"]
mod set_state_tests;
