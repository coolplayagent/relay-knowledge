//! Compatibility migration for durable index-refresh tasks.

use rusqlite::Connection;

use crate::storage::StorageError;

use super::super::introspection::{TableColumn, table_column_info, table_exists};

const INDEX_REFRESH_TASK_COLUMNS: &[&str] = &[
    "task_id",
    "kind",
    "source_scope",
    "modality",
    "target_graph_version",
    "state",
    "lease_owner",
    "lease_expires_at_ms",
    "attempt_count",
    "next_retry_at_ms",
    "input_fingerprint",
    "cursor_before",
    "cursor_after",
    "last_error_kind",
    "last_error_message",
    "created_at_ms",
    "updated_at_ms",
];
const LEGACY_NEXT_RETRY_AFTER_COLUMN: &str = "next_retry_after_ms";

pub(super) fn rebuild_incompatible_index_refresh_tasks(
    connection: &Connection,
) -> Result<(), StorageError> {
    if !table_exists(connection, "index_refresh_tasks")? {
        return Ok(());
    }
    let columns = table_column_info(connection, "index_refresh_tasks")?;
    if !index_refresh_tasks_needs_rebuild(&columns) {
        return Ok(());
    }

    let select_expressions = index_refresh_task_select_expressions(&columns);
    let insert_columns = INDEX_REFRESH_TASK_COLUMNS.join(", ");
    let select_columns = select_expressions.join(", ");
    let migration = format!(
        "
        DROP TABLE IF EXISTS index_refresh_tasks_rebuild_legacy;
        ALTER TABLE index_refresh_tasks RENAME TO index_refresh_tasks_rebuild_legacy;
        CREATE TABLE index_refresh_tasks (
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
        INSERT INTO index_refresh_tasks ({insert_columns})
        SELECT {select_columns}
        FROM index_refresh_tasks_rebuild_legacy;
        DROP TABLE index_refresh_tasks_rebuild_legacy;
        ",
    );

    connection
        .execute_batch(&migration)
        .map_err(StorageError::from)
}

fn index_refresh_tasks_needs_rebuild(columns: &[TableColumn]) -> bool {
    columns.iter().any(|column| {
        column.name == LEGACY_NEXT_RETRY_AFTER_COLUMN
            || (!INDEX_REFRESH_TASK_COLUMNS.contains(&column.name.as_str())
                && column.not_null
                && column.default_value.is_none())
    })
}

fn index_refresh_task_select_expressions(columns: &[TableColumn]) -> Vec<String> {
    let now_ms = "CAST(strftime('%s', 'now') AS INTEGER) * 1000";
    let fingerprint =
        "kind || ':' || source_scope || ':' || modality || ':' || target_graph_version";
    let created_at_ms = timestamp_expression(columns, "created_at_ms", now_ms);

    vec![
        column_or(
            columns,
            "task_id",
            "'legacy-index-refresh:' || lower(hex(randomblob(8)))",
        ),
        column_or(columns, "kind", "'bm25'"),
        column_or(columns, "source_scope", "'graph'"),
        column_or(columns, "modality", "'text'"),
        column_or(columns, "target_graph_version", "0"),
        column_or(columns, "state", "'queued'"),
        column_or(columns, "lease_owner", "NULL"),
        column_or(columns, "lease_expires_at_ms", "NULL"),
        column_or(columns, "attempt_count", "0"),
        retry_at_expression(columns),
        if has_column(columns, "input_fingerprint") {
            format!("COALESCE(NULLIF(input_fingerprint, ''), {fingerprint})")
        } else {
            fingerprint.to_owned()
        },
        column_or(columns, "cursor_before", "0"),
        column_or(columns, "cursor_after", "NULL"),
        column_or(columns, "last_error_kind", "NULL"),
        column_or(columns, "last_error_message", "NULL"),
        created_at_ms.clone(),
        if has_column(columns, "updated_at_ms") {
            timestamp_expression(columns, "updated_at_ms", now_ms)
        } else {
            created_at_ms
        },
    ]
}

fn retry_at_expression(columns: &[TableColumn]) -> String {
    if has_column(columns, "next_retry_at_ms")
        && has_column(columns, LEGACY_NEXT_RETRY_AFTER_COLUMN)
    {
        format!(
            "CASE WHEN next_retry_at_ms IS NOT NULL AND next_retry_at_ms != 0 \
             THEN next_retry_at_ms \
             ELSE COALESCE({LEGACY_NEXT_RETRY_AFTER_COLUMN}, 0) END"
        )
    } else if has_column(columns, "next_retry_at_ms") {
        "COALESCE(next_retry_at_ms, 0)".to_owned()
    } else if has_column(columns, LEGACY_NEXT_RETRY_AFTER_COLUMN) {
        format!("COALESCE({LEGACY_NEXT_RETRY_AFTER_COLUMN}, 0)")
    } else {
        "0".to_owned()
    }
}

fn timestamp_expression(columns: &[TableColumn], column: &str, now_ms: &str) -> String {
    if has_column(columns, column) {
        format!("CASE WHEN {column} IS NULL OR {column} = 0 THEN {now_ms} ELSE {column} END")
    } else {
        now_ms.to_owned()
    }
}

fn column_or(columns: &[TableColumn], column: &str, fallback: &str) -> String {
    if has_column(columns, column) {
        column.to_owned()
    } else {
        fallback.to_owned()
    }
}

fn has_column(columns: &[TableColumn], expected: &str) -> bool {
    columns.iter().any(|column| column.name == expected)
}
