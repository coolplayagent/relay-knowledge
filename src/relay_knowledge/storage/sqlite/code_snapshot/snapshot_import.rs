use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params};

use crate::storage::StorageError;

use super::{CodeScopeTable, IMPORT_SCHEMA};

const OPTIONAL_LEGACY_IMPORT_TABLES: &[&str] = &["code_repository_routes"];
const LEGACY_IMPORT_COLUMN_DEFAULTS: &[(&str, &str, &str)] = &[
    ("code_repository_files", "is_generated", "0"),
    ("code_repository_symbols", "symbol_role_json", "NULL"),
];

pub(super) fn copy_attached_code_table(
    transaction: &rusqlite::Transaction<'_>,
    table: &CodeScopeTable,
    source_scope: &str,
) -> Result<(), StorageError> {
    if !attached_code_table_exists(transaction, table.table)? {
        if OPTIONAL_LEGACY_IMPORT_TABLES.contains(&table.table) {
            tracing::debug!(
                table = table.table,
                "skipping code import table that is absent from legacy database"
            );
            return Ok(());
        }
        return Err(StorageError::InvalidInput(format!(
            "import database is missing required code table '{}'",
            table.table
        )));
    }
    let source_columns = attached_code_table_columns(transaction, table.table)?;
    let selected_columns = selected_attached_code_columns(table, &source_columns)?;
    transaction.execute(
        &format!(
            "INSERT INTO {table} ({columns}) SELECT {selected_columns} FROM {schema}.{table} WHERE source_scope = ?1",
            table = table.table,
            columns = table.columns,
            schema = IMPORT_SCHEMA,
        ),
        params![source_scope],
    )?;

    Ok(())
}

fn attached_code_table_exists(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
) -> Result<bool, StorageError> {
    transaction
        .query_row(
            &format!(
                "SELECT 1 FROM {IMPORT_SCHEMA}.sqlite_master WHERE type = 'table' AND name = ?1"
            ),
            params![table],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(StorageError::from)
}

fn attached_code_table_columns(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
) -> Result<BTreeSet<String>, StorageError> {
    let mut statement =
        transaction.prepare(&format!("PRAGMA {IMPORT_SCHEMA}.table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<Result<BTreeSet<_>, _>>()
        .map_err(StorageError::from)
}

fn selected_attached_code_columns(
    table: &CodeScopeTable,
    source_columns: &BTreeSet<String>,
) -> Result<String, StorageError> {
    let mut selected_columns = Vec::new();
    for column in table.columns.split(',') {
        let column = column.trim();
        if source_columns.contains(column) {
            selected_columns.push(column.to_owned());
        } else if let Some(default) = legacy_import_column_default(table.table, column) {
            selected_columns.push(format!("{default} AS {column}"));
        } else {
            return Err(StorageError::InvalidInput(format!(
                "import database table '{}' is missing required column '{}'",
                table.table, column
            )));
        }
    }
    Ok(selected_columns.join(", "))
}

fn legacy_import_column_default(table: &str, column: &str) -> Option<&'static str> {
    LEGACY_IMPORT_COLUMN_DEFAULTS
        .iter()
        .find(|(legacy_table, legacy_column, _)| *legacy_table == table && *legacy_column == column)
        .map(|(_, _, default)| *default)
}
