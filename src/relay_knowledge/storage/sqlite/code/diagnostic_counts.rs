//! Measures typed source coverage without treating an unknown subtree as one file.

use crate::domain::{CodeContentIntegrity, CodeContentIntegrityState};
use rusqlite::Connection;

pub(super) fn measure(
    connection: &Connection,
    scope: &str,
) -> rusqlite::Result<CodeContentIntegrity> {
    let (files, skipped_files, directories) = connection.query_row(
        "SELECT COUNT(DISTINCT CASE WHEN COALESCE(json_extract(io_json, '$.path_kind'), 'file') = 'file' THEN path END),
                COUNT(DISTINCT CASE WHEN json_extract(io_json, '$.path_kind') = 'file' THEN path END),
                COUNT(DISTINCT CASE WHEN json_extract(io_json, '$.path_kind') = 'directory' THEN path END)
         FROM code_repository_file_diagnostics WHERE source_scope = ?1", [scope],
        |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?, row.get::<_, usize>(2)?)))?;
    let mut integrity = CodeContentIntegrity::measured(scope.to_owned(), files);
    integrity.io_skipped_file_count = Some(skipped_files);
    integrity.io_skipped_directory_count = Some(directories);
    if directories > 0 {
        integrity.state = CodeContentIntegrityState::Partial;
    }
    Ok(integrity)
}

#[cfg(test)]
#[path = "diagnostic_counts_tests.rs"]
mod tests;
