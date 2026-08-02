//! Owns durable code-scope retention planning and pruning.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::CodeScopeRetentionSummary,
    storage::{CodeScopeRetentionRequest, StorageError},
};

use super::super::{lifecycle::cleanup::delete_scope_index, workspace};
use super::worktree::active_worktree_base_scopes;

pub(in crate::storage::sqlite::code) fn retention_status(
    connection: &mut Connection,
    repository_id: &str,
) -> Result<CodeScopeRetentionSummary, StorageError> {
    let active_scope = connection
        .query_row(
            "SELECT last_indexed_scope_id FROM code_repositories WHERE repository_id = ?1",
            params![repository_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .unwrap_or_default();
    retention_summary(
        connection,
        repository_id,
        &active_scope,
        2,
        false,
        Vec::new(),
    )
}

pub(in crate::storage::sqlite::code) fn prune_scopes(
    connection: &mut Connection,
    request: CodeScopeRetentionRequest,
) -> Result<CodeScopeRetentionSummary, StorageError> {
    prune_scopes_with_retained(connection, request, Vec::new())
}

pub(in crate::storage::sqlite::code) fn prune_scopes_with_retained(
    connection: &mut Connection,
    request: CodeScopeRetentionRequest,
    extra_retained_scopes: Vec<String>,
) -> Result<CodeScopeRetentionSummary, StorageError> {
    retention_summary(
        connection,
        &request.repository_id,
        &request.active_scope,
        request.retain_recent_successful_scopes,
        true,
        extra_retained_scopes,
    )
}

fn retention_summary(
    connection: &mut Connection,
    repository_id: &str,
    active_scope: &str,
    retain_recent_successful_scopes: usize,
    prune: bool,
    extra_retained_scopes: Vec<String>,
) -> Result<CodeScopeRetentionSummary, StorageError> {
    let all_scopes = repository_scopes(connection, repository_id)?;
    let mut retained = BTreeSet::new();
    if !active_scope.is_empty() {
        retained.insert(active_scope.to_owned());
    }
    for scope in active_worktree_base_scopes(connection, repository_id, active_scope)? {
        retained.insert(scope);
    }
    for scope in
        recent_successful_scopes(connection, repository_id, retain_recent_successful_scopes)?
    {
        retained.insert(scope);
    }
    for scope in unfinished_task_scopes(connection, repository_id)? {
        retained.insert(scope);
    }
    for scope in user_repository_set_member_scopes(connection, repository_id)? {
        retained.insert(scope);
    }
    for scope in extra_retained_scopes {
        retained.insert(scope);
    }
    let prunable = all_scopes
        .iter()
        .filter(|scope| !retained.contains(*scope))
        .cloned()
        .collect::<Vec<_>>();
    let mut pruned = Vec::new();
    if prune && !prunable.is_empty() {
        let transaction = connection.transaction()?;
        for scope in &prunable {
            workspace::clear_workspace_state(&transaction, repository_id, scope)?;
            delete_scope_index(&transaction, scope)?;
            transaction.execute(
                "DELETE FROM code_repository_scopes WHERE source_scope = ?1",
                params![scope],
            )?;
            transaction.execute(
                "DELETE FROM code_repository_index_checkpoints WHERE source_scope = ?1",
                params![scope],
            )?;
            pruned.push(scope.clone());
        }
        transaction.commit()?;
    }

    Ok(CodeScopeRetentionSummary {
        repository_id: repository_id.to_owned(),
        retained_scope_count: retained.len(),
        prunable_scope_count: prunable.len(),
        pruned_scope_count: pruned.len(),
        retained_scopes: retained.into_iter().collect(),
        prunable_scopes: prunable,
        pruned_scopes: pruned,
    })
}

fn repository_scopes(
    connection: &Connection,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT source_scope
        FROM code_repository_scopes
        WHERE repository_id = ?1
        ORDER BY source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn recent_successful_scopes(
    connection: &Connection,
    repository_id: &str,
    limit: usize,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT scope.source_scope
        FROM code_repository_scopes scope
        LEFT JOIN code_repository_index_checkpoints checkpoint
          ON checkpoint.source_scope = scope.source_scope
        WHERE scope.repository_id = ?1
        ORDER BY coalesce(checkpoint.updated_at_ms, 0) DESC, scope.source_scope DESC
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(params![repository_id, limit], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn unfinished_task_scopes(
    connection: &Connection,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT source_scope
        FROM code_repository_index_tasks
        WHERE repository_id = ?1 AND state IN ('queued', 'running', 'retrying')
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn user_repository_set_member_scopes(
    connection: &Connection,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let auto_set_id = workspace::workspace_set_id(repository_id);
    let mut statement = connection.prepare(
        "
        SELECT DISTINCT member.source_scope
        FROM code_repository_set_members member
        INNER JOIN code_repository_sets set_record
           ON set_record.set_id = member.set_id
        WHERE member.repository_id = ?1
          AND member.set_id <> ?2
        ORDER BY source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id, auto_set_id], |row| {
        row.get::<_, String>(0)
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;
