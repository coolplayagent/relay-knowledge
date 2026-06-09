use rusqlite::Transaction;

use crate::storage::StorageError;

const IMPORT_SCHEMA: &str = "relay_import";

pub(super) fn attached_code_table_selected_columns(
    transaction: &Transaction<'_>,
    table: &str,
    columns: &str,
) -> Result<String, StorageError> {
    if table == "code_repository_files"
        && !attached_table_has_column(transaction, table, "is_generated")?
    {
        return Ok(
            "repository_id, source_scope, file_id, path, language_id, blob_hash, byte_len, \
             line_count, parse_status, 0, degraded_reason"
                .to_owned(),
        );
    }

    Ok(columns.to_owned())
}

pub(super) fn attached_generated_detection_is_current(
    transaction: &Transaction<'_>,
    migration: &str,
) -> Result<bool, StorageError> {
    Ok(
        attached_table_has_column(transaction, "code_repository_files", "is_generated")?
            && attached_code_schema_migration_applied(transaction, migration)?,
    )
}

fn attached_code_schema_migration_applied(
    transaction: &Transaction<'_>,
    migration: &str,
) -> Result<bool, StorageError> {
    if !attached_table_exists(transaction, "code_repository_schema_migrations")? {
        return Ok(false);
    }
    transaction
        .query_row(
            &format!(
                "
                SELECT EXISTS (
                    SELECT 1
                    FROM {IMPORT_SCHEMA}.code_repository_schema_migrations
                    WHERE name = ?1
                )
                "
            ),
            [migration],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

fn attached_table_exists(transaction: &Transaction<'_>, table: &str) -> Result<bool, StorageError> {
    transaction
        .query_row(
            &format!(
                "
                SELECT EXISTS (
                    SELECT 1
                    FROM {IMPORT_SCHEMA}.sqlite_master
                    WHERE type = 'table' AND name = ?1
                )
                "
            ),
            [table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

fn attached_table_has_column(
    transaction: &Transaction<'_>,
    table: &str,
    column: &str,
) -> Result<bool, StorageError> {
    let mut statement =
        transaction.prepare(&format!("PRAGMA {IMPORT_SCHEMA}.table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }

    Ok(false)
}
