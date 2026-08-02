//! Owns durable code-index checkpoint read projections.

use rusqlite::{Connection, OptionalExtension, params};

use crate::{domain::CodeIndexCheckpoint, storage::StorageError};

use super::record_mapping::checkpoint_from_row;

const CHECKPOINT_COLUMNS: &str = "
    repository_id, source_scope, state, total_path_count, parsed_file_count,
    committed_file_count, committed_symbol_count, committed_reference_count,
    committed_chunk_count, batch_count, last_path, resource_budget_json,
    updated_at_ms
";

pub(in crate::storage::sqlite::code) fn checkpoint(
    connection: &mut Connection,
    source_scope: &str,
) -> Result<Option<CodeIndexCheckpoint>, StorageError> {
    let sql = format!(
        "
        SELECT {CHECKPOINT_COLUMNS}
        FROM code_repository_index_checkpoints
        WHERE source_scope = ?1
        "
    );
    connection
        .query_row(&sql, params![source_scope], checkpoint_from_row)
        .optional()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code) fn latest_checkpoint_for_repository(
    connection: &mut Connection,
    repository_id: &str,
) -> Result<Option<CodeIndexCheckpoint>, StorageError> {
    let sql = format!(
        "
        SELECT {CHECKPOINT_COLUMNS}
        FROM code_repository_index_checkpoints
        WHERE repository_id = ?1
        ORDER BY updated_at_ms DESC, source_scope DESC
        LIMIT 1
        "
    );
    connection
        .query_row(&sql, params![repository_id], checkpoint_from_row)
        .optional()
        .map_err(StorageError::from)
}

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
