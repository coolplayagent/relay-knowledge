use rusqlite::Connection;

use crate::storage::StorageError;

pub(super) fn ensure_index_schema_columns(connection: &Connection) -> Result<(), StorageError> {
    super::super::schema_columns::ensure_column(
        connection,
        "index_cursors",
        "source_hash",
        "TEXT",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_cursors",
        "backend_cursor",
        "TEXT",
    )?;
    super::super::schema_columns::ensure_column(connection, "index_cursors", "model_name", "TEXT")?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_cursors",
        "model_dimension",
        "INTEGER",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "lease_owner",
        "TEXT",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "lease_expires_at_ms",
        "INTEGER",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "attempt_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "next_retry_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "input_fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "cursor_before",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "cursor_after",
        "INTEGER",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "last_error_kind",
        "TEXT",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "last_error_message",
        "TEXT",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "index_refresh_tasks",
        "created_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::schema_columns::ensure_column(
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
