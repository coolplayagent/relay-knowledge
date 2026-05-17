use rusqlite::params;

use crate::storage::StorageError;

pub(super) fn delete_scope_index(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    for table in [
        "code_repository_path_tombstones",
        "code_repository_file_diagnostics",
        "code_repository_chunks",
        "code_repository_calls",
        "code_repository_imports",
        "code_repository_references",
        "code_repository_symbols",
        "code_repository_files",
        "code_repository_search",
    ] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
        )?;
    }

    Ok(())
}

pub(super) fn delete_path_index(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    path: &str,
) -> Result<(), StorageError> {
    for table in [
        "code_repository_file_diagnostics",
        "code_repository_chunks",
        "code_repository_calls",
        "code_repository_imports",
        "code_repository_references",
        "code_repository_symbols",
        "code_repository_files",
        "code_repository_search",
    ] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE source_scope = ?1 AND path = ?2"),
            params![source_scope, path],
        )?;
    }

    Ok(())
}

pub(super) fn count_code_rows(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    source_scope: &str,
) -> Result<usize, StorageError> {
    transaction
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}
