//! Persists and decodes checkpoint state for checkpointed code-index batches.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

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
            committed_reference_count, committed_chunk_count, committed_fact_row_count,
            batch_count, last_path,
            resource_budget_json, updated_at_ms, error_message
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, 0, 0, 0, 0, 0, NULL, ?9, ?10, ?11)
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

pub(super) fn compare_and_mark_state(
    transaction: &Transaction<'_>,
    source_scope: &str,
    expected_state: &str,
    next_state: &str,
) -> Result<(), StorageError> {
    let changed = transaction.execute(
        "UPDATE code_repository_index_checkpoints
         SET state = ?3, updated_at_ms = ?4
         WHERE source_scope = ?1 AND state = ?2",
        params![source_scope, expected_state, next_state, now_millis()],
    )?;
    if changed == 1 {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "checkpoint for scope '{source_scope}' changed while advancing from '{expected_state}' to '{next_state}'"
    )))
}

pub(super) fn compare_and_mark_dependency_refresh(
    transaction: &Transaction<'_>,
    source_scope: &str,
    expected_state: &str,
    next_state: &str,
    deleted_fact_count: usize,
    inserted_fact_count: usize,
) -> Result<(), StorageError> {
    let current_proof = transaction
        .query_row(
            "SELECT committed_fact_row_count
             FROM code_repository_index_checkpoints
             WHERE source_scope = ?1 AND state = ?2",
            params![source_scope, expected_state],
            |row| row.get::<_, usize>(0),
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Invariant(format!(
                "checkpoint for scope '{source_scope}' changed before dependency refresh"
            ))
        })?;
    let next_proof = if current_proof == 0 {
        0
    } else {
        current_proof
            .checked_sub(deleted_fact_count)
            .and_then(|value| value.checked_add(inserted_fact_count))
            .ok_or_else(|| {
                StorageError::Invariant(format!(
                    "dependency refresh for scope '{source_scope}' exceeds its exact fact-row proof"
                ))
            })?
    };
    let changed = transaction.execute(
        "UPDATE code_repository_index_checkpoints
         SET state = ?4, committed_fact_row_count = ?5, updated_at_ms = ?6
         WHERE source_scope = ?1 AND state = ?2 AND committed_fact_row_count = ?3",
        params![
            source_scope,
            expected_state,
            current_proof,
            next_state,
            next_proof,
            now_millis(),
        ],
    )?;
    if changed == 1 {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "checkpoint for scope '{source_scope}' changed during dependency refresh"
    )))
}

pub(super) fn compare_and_mark_completed(
    transaction: &Transaction<'_>,
    source_scope: &str,
    expected_state: &str,
) -> Result<(), StorageError> {
    let changed = transaction.execute(
        "UPDATE code_repository_index_checkpoints
         SET state = 'completed', updated_at_ms = ?3, error_message = NULL
         WHERE source_scope = ?1 AND state = ?2",
        params![source_scope, expected_state, now_millis()],
    )?;
    if changed == 1 {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "checkpoint for scope '{source_scope}' changed before completed-state publication from '{expected_state}'"
    )))
}

pub(super) fn load(
    connection: &Connection,
    source_scope: &str,
) -> Result<CodeIndexCheckpoint, StorageError> {
    load_optional(connection, source_scope)?.ok_or_else(|| {
        StorageError::Invariant(format!(
            "code index checkpoint for scope '{source_scope}' is unavailable"
        ))
    })
}

pub(super) fn load_optional(
    connection: &Connection,
    source_scope: &str,
) -> Result<Option<CodeIndexCheckpoint>, StorageError> {
    connection
        .query_row(
            "
            SELECT repository_id, source_scope, resolved_commit_sha, tree_hash,
                   path_filters_json, language_filters_json, state, total_path_count,
                   parsed_file_count,
                   committed_file_count, committed_symbol_count, committed_reference_count,
                   committed_chunk_count, committed_fact_row_count, incremental_summary_json,
                   batch_count, last_path,
                   resource_budget_json,
                   updated_at_ms
            FROM code_repository_index_checkpoints
            WHERE source_scope = ?1
            ",
            params![source_scope],
            checkpoint_from_row,
        )
        .optional()
        .map_err(|error| {
            if matches!(
                &error,
                rusqlite::Error::FromSqlConversionFailure(..)
                    | rusqlite::Error::IntegralValueOutOfRange(..)
                    | rusqlite::Error::InvalidColumnType(..)
            ) {
                return StorageError::Invariant(format!(
                    "code index checkpoint for scope '{source_scope}' cannot be decoded: {error}"
                ));
            }
            StorageError::from(error)
        })
}

fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<CodeIndexCheckpoint> {
    let resource_budget =
        serde_json::from_str(row.get::<_, String>(17)?.as_str()).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                17,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    let incremental_summary = super::super::checkpoint_receipt::decode(
        row.get::<_, Option<String>>(14)?,
        14,
        resource_budget,
    )?;
    let path_filters = super::super::status::parse_json_list(row.get(4)?)?;
    let language_filters = super::super::status::parse_json_list(row.get(5)?)?;
    Ok(CodeIndexCheckpoint {
        repository_id: row.get(0)?,
        source_scope: row.get(1)?,
        resolved_commit_sha: row.get(2)?,
        tree_hash: row.get(3)?,
        path_filters,
        language_filters,
        state: row.get(6)?,
        total_path_count: row.get(7)?,
        parsed_file_count: row.get(8)?,
        committed_file_count: row.get(9)?,
        committed_symbol_count: row.get(10)?,
        committed_reference_count: row.get(11)?,
        committed_chunk_count: row.get(12)?,
        committed_fact_row_count: row.get(13)?,
        incremental_summary,
        batch_count: row.get(15)?,
        last_path: row.get(16)?,
        resource_budget,
        updated_at_ms: row.get(18)?,
    })
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
