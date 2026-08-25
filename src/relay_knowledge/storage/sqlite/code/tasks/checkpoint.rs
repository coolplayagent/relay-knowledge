//! Owns durable code-index checkpoint read projections.

use rusqlite::{Connection, OptionalExtension, params};

use crate::{domain::CodeIndexCheckpoint, storage::StorageError};

use super::record_mapping::checkpoint_from_row;

const CHECKPOINT_COLUMNS: &str = "
    repository_id, source_scope, resolved_commit_sha, tree_hash,
    path_filters_json, language_filters_json, state, total_path_count, parsed_file_count,
    committed_file_count, committed_symbol_count, committed_reference_count,
    committed_chunk_count, committed_fact_row_count, incremental_summary_json, batch_count,
    last_path, resource_budget_json, updated_at_ms
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
        .map_err(|error| checkpoint_projection_error("scope", source_scope, error))
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
        .map_err(|error| checkpoint_projection_error("repository", repository_id, error))
}

fn checkpoint_projection_error(
    identity_kind: &str,
    identity: &str,
    error: rusqlite::Error,
) -> StorageError {
    if matches!(
        &error,
        rusqlite::Error::FromSqlConversionFailure(..)
            | rusqlite::Error::IntegralValueOutOfRange(..)
            | rusqlite::Error::InvalidColumnType(..)
    ) {
        return StorageError::Invariant(format!(
            "code index checkpoint for {identity_kind} '{identity}' cannot be decoded: {error}"
        ));
    }

    StorageError::from(error)
}

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
