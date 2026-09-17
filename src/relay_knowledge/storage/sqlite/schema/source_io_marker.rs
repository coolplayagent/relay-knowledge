use rusqlite::Connection;

use crate::storage::StorageError;

use super::introspection::table_has_columns;

pub(super) fn schema_is_current(connection: &Connection) -> Result<bool, StorageError> {
    for (table, column) in [
        ("code_repository_file_diagnostics", "io_json"),
        ("code_repository_index_checkpoints", "processed_path_count"),
        ("code_repository_scope_gc_jobs", "source_replan_task_id"),
    ] {
        if !table_has_columns(connection, table, &[column])? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
#[path = "source_io_marker_tests.rs"]
mod tests;
