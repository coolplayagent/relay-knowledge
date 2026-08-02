//! Persists and decodes checkpoint state for checkpointed code-index batches.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, Transaction, params};

use crate::{
    domain::{CodeIndexCheckpoint, CodeIndexSession},
    storage::StorageError,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub(super) fn insert(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    state: &str,
    error_message: Option<&str>,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        INSERT INTO code_repository_index_checkpoints (
            source_scope, repository_id, state, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, total_path_count,
            parsed_file_count, committed_file_count, committed_symbol_count,
            committed_reference_count, committed_chunk_count, batch_count, last_path,
            resource_budget_json, updated_at_ms, error_message
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, 0, 0, 0, 0, NULL, ?9, ?10, ?11)
        ",
        params![
            session.source_scope,
            session.repository_id,
            state,
            session.resolved_commit_sha,
            session.tree_hash,
            serialize_json(&session.path_filters)?,
            serialize_json(&session.language_filters)?,
            session.total_path_count,
            serialize_json(&session.resource_budget)?,
            now_millis(),
            error_message,
        ],
    )?;

    Ok(())
}

pub(super) fn mark_state(
    connection: &mut Connection,
    source_scope: &str,
    state: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE code_repository_index_checkpoints
        SET state = ?2, updated_at_ms = ?3
        WHERE source_scope = ?1
        ",
        params![source_scope, state, now_millis()],
    )?;

    Ok(())
}

pub(super) fn mark_completed(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        UPDATE code_repository_index_checkpoints
        SET state = 'completed', updated_at_ms = ?2, error_message = NULL
        WHERE source_scope = ?1
        ",
        params![source_scope, now_millis()],
    )?;

    Ok(())
}

pub(super) fn load(
    connection: &mut Connection,
    source_scope: &str,
) -> Result<CodeIndexCheckpoint, StorageError> {
    connection
        .query_row(
            "
            SELECT repository_id, source_scope, state, total_path_count, parsed_file_count,
                   committed_file_count, committed_symbol_count, committed_reference_count,
                   committed_chunk_count, batch_count, last_path, resource_budget_json,
                   updated_at_ms
            FROM code_repository_index_checkpoints
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| {
                let resource_budget = serde_json::from_str(row.get::<_, String>(11)?.as_str())
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            11,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                Ok(CodeIndexCheckpoint {
                    repository_id: row.get(0)?,
                    source_scope: row.get(1)?,
                    state: row.get(2)?,
                    total_path_count: row.get(3)?,
                    parsed_file_count: row.get(4)?,
                    committed_file_count: row.get(5)?,
                    committed_symbol_count: row.get(6)?,
                    committed_reference_count: row.get(7)?,
                    committed_chunk_count: row.get(8)?,
                    batch_count: row.get(9)?,
                    last_path: row.get(10)?,
                    resource_budget,
                    updated_at_ms: row.get(12)?,
                })
            },
        )
        .map_err(StorageError::from)
}

pub(super) fn count_scope_rows(
    connection: &Connection,
    source_scope: &str,
) -> Result<usize, StorageError> {
    let mut total = 0usize;
    for table in [
        "code_repository_files",
        "code_repository_symbols",
        "code_repository_references",
        "code_repository_imports",
        "code_repository_dependencies",
        "code_repository_calls",
        "code_repository_feature_flags",
        "code_repository_routes",
        "code_repository_chunks",
        "code_repository_file_diagnostics",
    ] {
        let count = connection.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
            |row| row.get::<_, usize>(0),
        )?;
        total = total.saturating_add(count);
    }

    Ok(total)
}

pub(super) fn count_scope_diagnostics(
    connection: &Connection,
    source_scope: Option<&str>,
) -> Result<usize, StorageError> {
    let Some(source_scope) = source_scope else {
        return Ok(0);
    };
    connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM code_repository_file_diagnostics
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

pub(super) fn serialize_json<T: serde::Serialize>(value: &T) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

pub(super) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
