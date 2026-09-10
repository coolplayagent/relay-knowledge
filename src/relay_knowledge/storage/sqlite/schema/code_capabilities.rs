//! Warm-open code-file and configuration metadata schema capabilities.
use super::introspection::{table_column_is_not_null, table_has_columns};
use crate::storage::StorageError;
use rusqlite::Connection;

pub(super) const CODE_REPOSITORY_FILES_COLUMNS: &[&str] = &[
    "repository_id",
    "source_scope",
    "file_id",
    "path",
    "language_id",
    "blob_hash",
    "byte_len",
    "line_count",
    "parse_status",
    "is_generated",
    "degraded_reason",
];

pub(super) fn configuration_metadata_is_current(
    connection: &Connection,
) -> Result<bool, StorageError> {
    Ok(table_has_columns(
        connection,
        "code_repository_feature_flags",
        &["metadata_json"],
    )? && table_column_is_not_null(
        connection,
        "code_repository_feature_flags",
        "metadata_json",
    )?)
}
