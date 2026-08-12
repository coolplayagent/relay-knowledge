//! Owns the repository-local scope-slot budget shared by queueing and rebinds.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{domain::CodeIndexMode, storage::StorageError};

/// Bounds published scopes, partial checkpoints, and unfinished publications.
pub(in crate::storage) const MAX_SCOPE_SLOTS_PER_REPOSITORY: usize = 64;

pub(super) fn enforce_new_target(
    transaction: &Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
    mode: &CodeIndexMode,
) -> Result<(), StorageError> {
    let usage = load_usage(transaction, repository_id, "main", "main")?;
    let already_reserved = if *mode == CodeIndexMode::WorktreeOverlay {
        false
    } else {
        usage.scopes.contains(source_scope)
    };
    if !already_reserved {
        reject_if_full(repository_id, usage.slot_count())?;
    }
    Ok(())
}

/// Bounds direct storage publications that do not carry a durable task fence.
///
/// Fenced publications reserve their target during queue admission (and replace
/// a provisional worktree reservation during rebind). Unfenced storage callers
/// still cannot create an unbounded physical scope/checkpoint backlog.
pub(in crate::storage::sqlite::code) fn enforce_unfenced_target(
    transaction: &Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    let usage = load_usage(transaction, repository_id, "main", "main")?;
    if !usage.scopes.contains(source_scope) {
        reject_if_full(repository_id, usage.slot_count())?;
    }
    Ok(())
}

pub(in crate::storage::sqlite::code) fn enforce_rebound_target(
    transaction: &Transaction<'_>,
    authority_schema: &str,
    repository_id: &str,
    task_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    let usage = load_usage(
        transaction,
        repository_id,
        authority_schema,
        authority_schema,
    )?;
    let current_reservation =
        unfinished_reservation(transaction, authority_schema, repository_id, task_id)?;
    let mut slots_after_rebind = usage.slot_count();
    if current_reservation.is_some_and(|reservation| reservation.counted_separately) {
        slots_after_rebind = slots_after_rebind.saturating_sub(1);
    }
    if !usage.scopes.contains(source_scope) {
        slots_after_rebind = slots_after_rebind.saturating_add(1);
    }
    if slots_after_rebind > MAX_SCOPE_SLOTS_PER_REPOSITORY {
        return Err(capacity_error(repository_id, usage.slot_count()));
    }
    Ok(())
}

struct ScopeUsage {
    scopes: BTreeSet<String>,
    pending_worktree_task_count: usize,
}

impl ScopeUsage {
    fn slot_count(&self) -> usize {
        self.scopes
            .len()
            .saturating_add(self.pending_worktree_task_count)
    }
}

struct UnfinishedReservation {
    counted_separately: bool,
}

fn load_usage(
    connection: &Connection,
    repository_id: &str,
    scope_schema: &str,
    task_schema: &str,
) -> Result<ScopeUsage, StorageError> {
    let mut scopes = BTreeSet::new();
    let published_scope_sql = format!(
        "SELECT source_scope FROM {scope_schema}.code_repository_scopes
         WHERE repository_id = ?1 LIMIT ?2"
    );
    collect_column(connection, &published_scope_sql, repository_id, &mut scopes)?;
    let checkpoint_sql = format!(
        "SELECT source_scope FROM {scope_schema}.code_repository_index_checkpoints
         WHERE repository_id = ?1 LIMIT ?2"
    );
    collect_column(connection, &checkpoint_sql, repository_id, &mut scopes)?;
    if table_exists(connection, scope_schema, "storage_repository_shard_scopes")? {
        let catalog_scope_sql = format!(
            "SELECT source_scope FROM {scope_schema}.storage_repository_shard_scopes
             WHERE repository_id = ?1 LIMIT ?2"
        );
        collect_column(connection, &catalog_scope_sql, repository_id, &mut scopes)?;
    }
    let sql = format!(
        "SELECT task_id, source_scope, mode_json
         FROM {task_schema}.code_repository_index_tasks
         WHERE repository_id = ?1 AND state IN ('queued', 'running', 'retrying')
         ORDER BY task_id LIMIT ?2"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params![repository_id, MAX_SCOPE_SLOTS_PER_REPOSITORY + 1],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    let mut pending_worktree_task_count = 0usize;
    for row in rows {
        let (_, source_scope, mode_json) = row?;
        if task_is_worktree(&mode_json)? {
            pending_worktree_task_count = pending_worktree_task_count.saturating_add(1);
        } else {
            scopes.insert(source_scope);
        }
    }
    Ok(ScopeUsage {
        scopes,
        pending_worktree_task_count,
    })
}

fn unfinished_reservation(
    connection: &Connection,
    authority_schema: &str,
    repository_id: &str,
    task_id: &str,
) -> Result<Option<UnfinishedReservation>, StorageError> {
    let sql = format!(
        "SELECT source_scope, mode_json
         FROM {authority_schema}.code_repository_index_tasks
         WHERE repository_id = ?1 AND task_id = ?2
           AND state IN ('queued', 'running', 'retrying')"
    );
    connection
        .query_row(&sql, params![repository_id, task_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .optional()?
        .map(|(_, mode_json)| {
            Ok(UnfinishedReservation {
                counted_separately: task_is_worktree(&mode_json)?,
            })
        })
        .transpose()
}

fn collect_column(
    connection: &Connection,
    sql: &str,
    repository_id: &str,
    output: &mut BTreeSet<String>,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(
        params![repository_id, MAX_SCOPE_SLOTS_PER_REPOSITORY + 1],
        |row| row.get::<_, String>(0),
    )?;
    for row in rows {
        output.insert(row?);
        if output.len() > MAX_SCOPE_SLOTS_PER_REPOSITORY {
            break;
        }
    }
    Ok(())
}

fn table_exists(connection: &Connection, schema: &str, table: &str) -> Result<bool, StorageError> {
    let sql = format!("SELECT 1 FROM {schema}.sqlite_master WHERE type = 'table' AND name = ?1");
    connection
        .query_row(&sql, [table], |_| Ok(()))
        .optional()
        .map(|row| row.is_some())
        .map_err(StorageError::from)
}

fn task_is_worktree(mode_json: &str) -> Result<bool, StorageError> {
    let mode = serde_json::from_str::<CodeIndexMode>(mode_json)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    Ok(mode == CodeIndexMode::WorktreeOverlay)
}

fn reject_if_full(repository_id: &str, occupied: usize) -> Result<(), StorageError> {
    if occupied >= MAX_SCOPE_SLOTS_PER_REPOSITORY {
        return Err(capacity_error(repository_id, occupied));
    }
    Ok(())
}

fn capacity_error(repository_id: &str, occupied: usize) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "code index scope backlog for repository '{repository_id}' has {occupied} published or pending scopes (capacity {MAX_SCOPE_SLOTS_PER_REPOSITORY}); wait for managed scope maintenance before indexing another commit"
    ))
}
