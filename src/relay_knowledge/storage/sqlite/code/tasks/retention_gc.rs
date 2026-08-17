//! Bounded, durable scope-retirement state machine.

#[cfg(test)]
#[path = "retention_gc_tests.rs"]
mod tests;

use rusqlite::{
    Connection, OptionalExtension, Transaction, params, params_from_iter, types::Value,
};

use crate::{domain::CodeScopeRetirementJobStatus, storage::StorageError};

pub(in crate::storage::sqlite::code) const GC_ROW_BATCH_SIZE: usize = 512;
const SEARCH_OWNER_BATCH_SIZE: usize = GC_ROW_BATCH_SIZE / 2;
const INITIAL_PHASE: &str = "workspace_edges";

const PHASES: &[&str] = &[
    INITIAL_PHASE,
    "workspace_mappings",
    "workspace_members",
    "workspace_overlay",
    "catalog_route",
    "search_documents",
    "path_tombstones",
    "file_diagnostics",
    "chunks",
    "calls",
    "routes",
    "feature_flags",
    "dependencies",
    "imports",
    "references",
    "symbols",
    "files",
    "software_components",
    "software_dependency_usages",
    "software_sdk_usages",
    "software_files",
    "software_topics",
    "software_relationships",
    "software_global_status",
    "software_build_targets",
    "software_iac_resources",
    "software_design_elements",
    "commit_scopes",
    "index_batch_staging",
    "index_task_history",
    "checkpoint",
    "scope_metadata",
];

pub(super) fn jobs(
    connection: &Connection,
    repository_id: &str,
) -> Result<Vec<CodeScopeRetirementJobStatus>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT repository_id, source_scope, phase, deleted_rows, updated_at_ms, last_error
         FROM code_repository_scope_gc_jobs
         WHERE repository_id = ?1
         ORDER BY updated_at_ms ASC, source_scope ASC",
    )?;
    let rows = statement.query_map(params![repository_id], |row| {
        Ok(CodeScopeRetirementJobStatus {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            phase: row.get(2)?,
            deleted_rows: row.get(3)?,
            updated_at_ms: row.get(4)?,
            last_error: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn schedule(
    transaction: &Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
    now_ms: u64,
) -> Result<(), StorageError> {
    transaction.execute(
        "UPDATE code_repository_scopes
         SET retiring = 1
         WHERE repository_id = ?1 AND source_scope = ?2 AND retiring = 0",
        params![repository_id, source_scope],
    )?;
    transaction.execute(
        "INSERT OR IGNORE INTO code_repository_scope_gc_jobs (
             source_scope, repository_id, phase, deleted_rows,
             created_at_ms, updated_at_ms, last_error
         ) VALUES (?1, ?2, ?3, 0, ?4, ?4, NULL)",
        params![source_scope, repository_id, INITIAL_PHASE, now_ms],
    )?;
    transaction.execute(
        "UPDATE code_repositories
         SET last_indexed_scope_id = NULL,
             last_indexed_commit = NULL,
             tree_hash = NULL,
             state = 'registered',
             indexed_file_count = 0,
             symbol_count = 0,
             reference_count = 0,
             chunk_count = 0,
             stale = 1,
             degraded_reason = NULL
         WHERE repository_id = ?1 AND last_indexed_scope_id = ?2",
        params![repository_id, source_scope],
    )?;
    Ok(())
}

pub(super) fn process_one(
    transaction: &Transaction<'_>,
    repository_id: &str,
    now_ms: u64,
) -> Result<Option<String>, StorageError> {
    let Some((source_scope, phase)) = transaction
        .query_row(
            "SELECT source_scope, phase
             FROM code_repository_scope_gc_jobs
             WHERE repository_id = ?1
             ORDER BY updated_at_ms ASC, source_scope ASC
             LIMIT 1",
            params![repository_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };

    let result = delete_phase_batch(transaction, repository_id, &source_scope, &phase);
    let (deleted, has_more) = match result {
        Ok(progress) => progress,
        Err(error) => {
            transaction.execute(
                "UPDATE code_repository_scope_gc_jobs
                 SET updated_at_ms = ?2, last_error = ?3
                 WHERE source_scope = ?1",
                params![source_scope, now_ms, error.to_string()],
            )?;
            return Ok(None);
        }
    };
    let next = (!has_more).then(|| next_phase(&phase)).flatten();
    if has_more {
        update_progress(transaction, &source_scope, &phase, deleted, now_ms)?;
        return Ok(None);
    }
    if let Some(next) = next {
        update_progress(transaction, &source_scope, next, deleted, now_ms)?;
        return Ok(None);
    }

    transaction.execute(
        "DELETE FROM code_repository_scope_gc_jobs WHERE source_scope = ?1",
        params![source_scope],
    )?;
    Ok(Some(source_scope))
}

pub(in crate::storage::sqlite::code) fn reject_retiring_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    let retiring = connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM code_repository_scope_gc_jobs WHERE source_scope = ?1
             UNION ALL
             SELECT 1 FROM code_repository_scopes
             WHERE source_scope = ?1 AND retiring != 0
         )",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    if retiring {
        return Err(StorageError::InvalidInput(format!(
            "code repository source scope '{source_scope}' is retiring; wait for bounded maintenance to complete before rebuilding or querying it"
        )));
    }
    Ok(())
}

fn update_progress(
    transaction: &Transaction<'_>,
    source_scope: &str,
    phase: &str,
    deleted: usize,
    now_ms: u64,
) -> Result<(), StorageError> {
    transaction.execute(
        "UPDATE code_repository_scope_gc_jobs
         SET phase = ?2, deleted_rows = deleted_rows + ?3,
             updated_at_ms = ?4, last_error = NULL
         WHERE source_scope = ?1",
        params![source_scope, phase, deleted, now_ms],
    )?;
    Ok(())
}

fn next_phase(phase: &str) -> Option<&'static str> {
    PHASES
        .iter()
        .position(|candidate| *candidate == phase)
        .and_then(|index| PHASES.get(index + 1).copied())
}

fn delete_phase_batch(
    transaction: &Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
    phase: &str,
) -> Result<(usize, bool), StorageError> {
    match phase {
        "workspace_edges" => delete_cross_edges(transaction, source_scope),
        "workspace_mappings" => delete_table_batch(
            transaction,
            "code_workspace_package_mappings",
            "source_scope",
            source_scope,
        ),
        "workspace_members" => delete_table_batch(
            transaction,
            "code_repository_set_members",
            "source_scope",
            source_scope,
        ),
        "workspace_overlay" => delete_workspace_overlay(transaction, repository_id),
        "catalog_route" => delete_catalog_route(transaction, source_scope),
        "search_documents" => delete_search_batch(transaction, source_scope),
        "path_tombstones" => {
            delete_scope_table(transaction, "code_repository_path_tombstones", source_scope)
        }
        "file_diagnostics" => delete_scope_table(
            transaction,
            "code_repository_file_diagnostics",
            source_scope,
        ),
        "chunks" => delete_scope_table(transaction, "code_repository_chunks", source_scope),
        "calls" => delete_scope_table(transaction, "code_repository_calls", source_scope),
        "routes" => delete_scope_table(transaction, "code_repository_routes", source_scope),
        "feature_flags" => {
            delete_scope_table(transaction, "code_repository_feature_flags", source_scope)
        }
        "dependencies" => {
            delete_scope_table(transaction, "code_repository_dependencies", source_scope)
        }
        "imports" => delete_scope_table(transaction, "code_repository_imports", source_scope),
        "references" => delete_scope_table(transaction, "code_repository_references", source_scope),
        "symbols" => delete_scope_table(transaction, "code_repository_symbols", source_scope),
        "files" => delete_scope_table(transaction, "code_repository_files", source_scope),
        "software_components" => {
            delete_scope_table(transaction, "software_components", source_scope)
        }
        "software_dependency_usages" => {
            delete_scope_table(transaction, "software_dependency_usages", source_scope)
        }
        "software_sdk_usages" => {
            delete_scope_table(transaction, "software_sdk_usages", source_scope)
        }
        "software_files" => delete_scope_table(transaction, "software_files", source_scope),
        "software_topics" => delete_scope_table(transaction, "software_topics", source_scope),
        "software_relationships" => {
            delete_scope_table(transaction, "software_relationships", source_scope)
        }
        "software_global_status" => {
            delete_scope_table(transaction, "software_global_status", source_scope)
        }
        "software_build_targets" => {
            delete_scope_table(transaction, "software_build_targets", source_scope)
        }
        "software_iac_resources" => {
            delete_scope_table(transaction, "software_iac_resources", source_scope)
        }
        "software_design_elements" => {
            delete_scope_table(transaction, "software_design_elements", source_scope)
        }
        "commit_scopes" => {
            delete_scope_table(transaction, "code_repository_commit_scopes", source_scope)
        }
        "index_batch_staging" => delete_scope_table(
            transaction,
            "code_repository_index_batch_staging",
            source_scope,
        ),
        "index_task_history" => delete_terminal_tasks(transaction, source_scope),
        "checkpoint" => delete_scope_table(
            transaction,
            "code_repository_index_checkpoints",
            source_scope,
        ),
        "scope_metadata" => delete_scope_table(transaction, "code_repository_scopes", source_scope),
        unknown => Err(StorageError::InvalidInput(format!(
            "scope GC job has unknown phase '{unknown}'"
        ))),
    }
}

fn delete_scope_table(
    transaction: &Transaction<'_>,
    table: &'static str,
    source_scope: &str,
) -> Result<(usize, bool), StorageError> {
    delete_table_batch(transaction, table, "source_scope", source_scope)
}

fn delete_table_batch(
    transaction: &Transaction<'_>,
    table: &'static str,
    column: &'static str,
    value: &str,
) -> Result<(usize, bool), StorageError> {
    let deleted = transaction.execute(
        &format!(
            "DELETE FROM {table} WHERE rowid IN (
                 SELECT rowid FROM {table} WHERE {column} = ?1
                 ORDER BY rowid LIMIT ?2
             )"
        ),
        params![value, GC_ROW_BATCH_SIZE],
    )?;
    let has_more = transaction.query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE {column} = ?1 LIMIT 1)"),
        params![value],
        |row| row.get::<_, bool>(0),
    )?;
    Ok((deleted, has_more))
}

fn delete_cross_edges(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(usize, bool), StorageError> {
    let deleted = transaction.execute(
        "DELETE FROM code_repository_cross_edges WHERE rowid IN (
             SELECT rowid FROM code_repository_cross_edges
             WHERE from_source_scope = ?1 OR to_source_scope = ?1
             ORDER BY rowid LIMIT ?2
         )",
        params![source_scope, GC_ROW_BATCH_SIZE],
    )?;
    let has_more = transaction.query_row(
        "SELECT EXISTS (SELECT 1 FROM code_repository_cross_edges
         WHERE from_source_scope = ?1 OR to_source_scope = ?1 LIMIT 1)",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    Ok((deleted, has_more))
}

fn delete_workspace_overlay(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<(usize, bool), StorageError> {
    let set_id = super::super::workspace::workspace_set_id(repository_id);
    let mut deleted = transaction.execute(
        "DELETE FROM code_repository_set_overlay_status WHERE set_id = ?1",
        params![set_id],
    )?;
    deleted += transaction.execute(
        "DELETE FROM code_repository_sets
         WHERE set_id = ?1 AND NOT EXISTS (
             SELECT 1 FROM code_repository_set_members WHERE set_id = ?1
         )",
        params![set_id],
    )?;
    Ok((deleted, false))
}

fn delete_catalog_route(
    _transaction: &Transaction<'_>,
    _source_scope: &str,
) -> Result<(usize, bool), StorageError> {
    // The control-plane catalog route is also the durable capacity reservation
    // for a scope whose physical rows still exist in a repository shard. The
    // partitioned retention coordinator removes that route immediately before
    // the shard's final scope_metadata phase. Removing it from this generic
    // control-DB state machine would let admission reuse the slot while a large
    // shard scope was still being deleted in bounded batches.
    Ok((0, false))
}

fn delete_search_batch(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(usize, bool), StorageError> {
    let mut statement = transaction.prepare(
        "SELECT rowid, search_rowid FROM code_repository_search_metadata
         WHERE source_scope = ?1 ORDER BY rowid LIMIT ?2",
    )?;
    let pairs = statement
        .query_map(params![source_scope, SEARCH_OWNER_BATCH_SIZE], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut deleted = 0;
    if !pairs.is_empty() {
        deleted += delete_rowids(
            transaction,
            "code_repository_search",
            pairs.iter().map(|(_, search_rowid)| *search_rowid),
        )?;
        deleted += delete_rowids(
            transaction,
            "code_repository_search_metadata",
            pairs.iter().map(|(rowid, _)| *rowid),
        )?;
    }
    // Search persistence and schema-open backfill maintain one indexed
    // metadata owner for every FTS row. Never scan the FTS5 UNINDEXED
    // source_scope column as a cleanup fallback.
    let has_more = transaction.query_row(
        "SELECT EXISTS (SELECT 1 FROM code_repository_search_metadata
         WHERE source_scope = ?1 LIMIT 1)",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    Ok((deleted, has_more))
}

fn delete_rowids(
    transaction: &Transaction<'_>,
    table: &'static str,
    rowids: impl IntoIterator<Item = i64>,
) -> Result<usize, StorageError> {
    let rowids = rowids.into_iter().collect::<Vec<_>>();
    if rowids.is_empty() {
        return Ok(0);
    }
    let placeholders = std::iter::repeat_n("?", rowids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let values = rowids.into_iter().map(Value::Integer).collect::<Vec<_>>();
    transaction
        .execute(
            &format!("DELETE FROM {table} WHERE rowid IN ({placeholders})"),
            params_from_iter(values),
        )
        .map_err(StorageError::from)
}

fn delete_terminal_tasks(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(usize, bool), StorageError> {
    let deleted = transaction.execute(
        "DELETE FROM code_repository_index_tasks WHERE task_id IN (
             SELECT task_id FROM code_repository_index_tasks
             WHERE source_scope = ?1
               AND state IN ('succeeded', 'failed', 'dead_letter', 'cancelled')
             ORDER BY updated_at_ms, task_id LIMIT ?2
         )",
        params![source_scope, GC_ROW_BATCH_SIZE],
    )?;
    let has_more = transaction.query_row(
        "SELECT EXISTS (SELECT 1 FROM code_repository_index_tasks
         WHERE source_scope = ?1
           AND state IN ('succeeded', 'failed', 'dead_letter', 'cancelled') LIMIT 1)",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    Ok((deleted, has_more))
}
