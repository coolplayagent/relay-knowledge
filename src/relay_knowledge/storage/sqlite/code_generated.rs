use rusqlite::{Connection, params};

use crate::storage::StorageError;

pub(in crate::storage::sqlite::code) fn backfill_all_path_generated_flags(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.execute(
        &format!(
            "
            UPDATE code_repository_files
            SET is_generated = 1
            WHERE is_generated = 0
              AND {}
            ",
            generated_path_predicate("path")
        ),
        [],
    )?;

    Ok(())
}

pub(in crate::storage::sqlite::code) fn backfill_scope_path_generated_flags(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    connection.execute(
        &format!(
            "
            UPDATE code_repository_files
            SET is_generated = 1
            WHERE source_scope = ?1
              AND is_generated = 0
              AND {}
            ",
            generated_path_predicate("path")
        ),
        params![source_scope],
    )?;

    Ok(())
}

pub(in crate::storage::sqlite::code) fn mark_all_generated_detection_scopes_stale(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE code_repository_scopes
        SET stale = 1
        WHERE EXISTS (
            SELECT 1
            FROM code_repository_files file
            WHERE file.source_scope = code_repository_scopes.source_scope
        )
        ",
        [],
    )?;
    connection.execute(
        "
        UPDATE code_repositories
        SET stale = 1
        WHERE last_indexed_scope_id IN (
            SELECT source_scope
            FROM code_repository_scopes
            WHERE stale != 0
        )
        ",
        [],
    )?;

    Ok(())
}

pub(in crate::storage::sqlite::code) fn mark_scope_generated_detection_stale(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE code_repository_scopes
        SET stale = 1
        WHERE source_scope = ?1
        ",
        params![source_scope],
    )?;
    connection.execute(
        "
        UPDATE code_repositories
        SET stale = 1
        WHERE last_indexed_scope_id = ?1
        ",
        params![source_scope],
    )?;

    Ok(())
}

fn generated_path_predicate(path_column: &str) -> String {
    format!(
        "
        (
            lower({path_column}) LIKE '%.pb.go'
            OR lower({path_column}) LIKE '%.pulsar.go'
            OR lower({path_column}) LIKE '%.generated.ts'
            OR lower({path_column}) LIKE '%.generated.tsx'
            OR lower({path_column}) LIKE '%.generated.js'
            OR lower({path_column}) LIKE '%.auto.ts'
            OR lower({path_column}) LIKE '%.auto.tsx'
            OR lower({path_column}) LIKE '%.auto.js'
            OR lower({path_column}) LIKE '%.min.js'
            OR lower({path_column}) LIKE '%.min.css'
            OR lower({path_column}) LIKE 'dist/%'
            OR lower({path_column}) LIKE 'build/%'
            OR lower({path_column}) LIKE 'target/generated/%'
            OR lower({path_column}) LIKE '%/target/generated/%'
        )
        "
    )
}
