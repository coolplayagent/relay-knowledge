//! Serializes catalog creation and legacy column upgrades.

use std::{collections::BTreeSet, path::Path};

use rusqlite::{Connection, TransactionBehavior};

use crate::storage::StorageError;

const SHARD_TABLE_COLUMNS: &[&str] = &[
    "repository_id",
    "db_path",
    "state",
    "created_at_ms",
    "updated_at_ms",
];
const SCOPE_TABLE_COLUMNS: &[&str] = &[
    "source_scope",
    "repository_id",
    "state",
    "staged_task_id",
    "updated_at_ms",
];

pub(in crate::storage::partitioned) fn initialize_catalog_schema(
    control_path: &Path,
) -> Result<(), StorageError> {
    if catalog_schema_is_current(control_path)? {
        return Ok(());
    }

    let mut connection = super::open_catalog_connection(control_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS storage_repository_shards (
            repository_id TEXT PRIMARY KEY,
            db_path TEXT NOT NULL,
            state TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS storage_repository_shard_scopes (
            source_scope TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            state TEXT NOT NULL DEFAULT 'active',
            staged_task_id TEXT,
            updated_at_ms INTEGER NOT NULL,
            FOREIGN KEY (repository_id) REFERENCES storage_repository_shards(repository_id)
                ON DELETE CASCADE
        );
        ",
    )?;
    ensure_catalog_scope_state_column(&transaction)?;
    ensure_catalog_scope_staged_task_column(&transaction)?;
    transaction.commit()?;

    Ok(())
}

fn catalog_schema_is_current(control_path: &Path) -> Result<bool, StorageError> {
    if !control_path.exists() {
        return Ok(false);
    }
    let connection = super::open_catalog_readonly_connection(control_path)?;
    Ok(table_has_required_columns(
        &connection,
        "PRAGMA table_info(storage_repository_shards)",
        SHARD_TABLE_COLUMNS,
    )? && table_has_required_columns(
        &connection,
        "PRAGMA table_info(storage_repository_shard_scopes)",
        SCOPE_TABLE_COLUMNS,
    )?)
}

fn table_has_required_columns(
    connection: &Connection,
    table_info_query: &str,
    required_columns: &[&str],
) -> Result<bool, StorageError> {
    let mut missing = required_columns.iter().copied().collect::<BTreeSet<_>>();
    let mut statement = connection.prepare(table_info_query)?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        missing.remove(column?.as_str());
    }
    Ok(missing.is_empty())
}

fn ensure_catalog_scope_staged_task_column(connection: &Connection) -> Result<(), StorageError> {
    let mut statement = connection.prepare("PRAGMA table_info(storage_repository_shard_scopes)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == "staged_task_id" {
            return Ok(());
        }
    }
    connection.execute(
        "ALTER TABLE storage_repository_shard_scopes ADD COLUMN staged_task_id TEXT",
        [],
    )?;
    Ok(())
}

fn ensure_catalog_scope_state_column(connection: &Connection) -> Result<(), StorageError> {
    let has_state = {
        let mut statement =
            connection.prepare("PRAGMA table_info(storage_repository_shard_scopes)")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
        let mut has_state = false;
        for row in rows {
            has_state |= row? == "state";
        }
        has_state
    };
    if has_state {
        return Ok(());
    }
    connection.execute(
        "ALTER TABLE storage_repository_shard_scopes ADD COLUMN state TEXT NOT NULL DEFAULT 'active'",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
