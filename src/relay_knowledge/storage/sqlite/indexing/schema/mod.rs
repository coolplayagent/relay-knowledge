use rusqlite::{Connection, params};

use crate::{domain::IndexKind, storage::StorageError};

use super::super::schema::columns;

pub(crate) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS index_status (
            kind TEXT PRIMARY KEY,
            index_version INTEGER NOT NULL,
            indexed_graph_version INTEGER NOT NULL,
            state TEXT NOT NULL,
            last_error TEXT
        );

        CREATE TABLE IF NOT EXISTS index_cursors (
            kind TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            modality TEXT NOT NULL,
            index_version INTEGER NOT NULL,
            indexed_graph_version INTEGER NOT NULL,
            state TEXT NOT NULL,
            last_error TEXT,
            source_hash TEXT,
            backend_cursor TEXT,
            model_name TEXT,
            model_dimension INTEGER,
            PRIMARY KEY (kind, source_scope, modality)
        );

        CREATE TABLE IF NOT EXISTS index_scope_manifest (
            source_scope TEXT PRIMARY KEY
        );

        CREATE TABLE IF NOT EXISTS index_refresh_tasks (
            task_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            modality TEXT NOT NULL,
            target_graph_version INTEGER NOT NULL,
            state TEXT NOT NULL,
            lease_owner TEXT,
            lease_expires_at_ms INTEGER,
            attempt_count INTEGER NOT NULL,
            next_retry_at_ms INTEGER NOT NULL,
            input_fingerprint TEXT NOT NULL,
            cursor_before INTEGER NOT NULL,
            cursor_after INTEGER,
            last_error_kind TEXT,
            last_error_message TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        ",
    )?;
    ensure_index_schema_columns(connection)?;

    for kind in IndexKind::ALL {
        connection.execute(
            "INSERT OR IGNORE INTO index_status
             (kind, index_version, indexed_graph_version, state, last_error)
             VALUES (?1, 0, 0, 'fresh', NULL)",
            params![kind.as_str()],
        )?;
    }
    connection.execute(
        "
        INSERT OR IGNORE INTO index_scope_manifest (source_scope)
        SELECT DISTINCT source_scope FROM evidence
        ",
        [],
    )?;
    connection.execute(
        "
        INSERT OR IGNORE INTO index_scope_manifest (source_scope)
        SELECT DISTINCT source_scope FROM index_cursors
        ",
        [],
    )?;

    Ok(())
}

#[cfg(test)]
#[path = "schema_migration_tests.rs"]
mod schema_migration_tests;

fn ensure_index_schema_columns(connection: &Connection) -> Result<(), StorageError> {
    columns::ensure_column(connection, "index_cursors", "source_hash", "TEXT")?;
    columns::ensure_column(connection, "index_cursors", "backend_cursor", "TEXT")?;
    columns::ensure_column(connection, "index_cursors", "model_name", "TEXT")?;
    columns::ensure_column(connection, "index_cursors", "model_dimension", "INTEGER")?;
    columns::ensure_column(connection, "index_refresh_tasks", "lease_owner", "TEXT")?;
    columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "lease_expires_at_ms",
        "INTEGER",
    )?;
    columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "attempt_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "next_retry_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "input_fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "cursor_before",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    columns::ensure_column(connection, "index_refresh_tasks", "cursor_after", "INTEGER")?;
    columns::ensure_column(connection, "index_refresh_tasks", "last_error_kind", "TEXT")?;
    columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "last_error_message",
        "TEXT",
    )?;
    columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "created_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "updated_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    connection.execute(
        "
        UPDATE index_refresh_tasks
        SET created_at_ms = CAST(strftime('%s', 'now') AS INTEGER) * 1000
        WHERE created_at_ms IS NULL OR created_at_ms = 0
        ",
        [],
    )?;
    connection.execute(
        "
        UPDATE index_refresh_tasks
        SET updated_at_ms = created_at_ms
        WHERE updated_at_ms IS NULL OR updated_at_ms = 0
        ",
        [],
    )?;
    connection.execute(
        "
        UPDATE index_refresh_tasks
        SET input_fingerprint = kind || ':' || source_scope || ':' || modality || ':' || target_graph_version
        WHERE input_fingerprint IS NULL OR input_fingerprint = ''
        ",
        [],
    )?;

    Ok(())
}
