use std::{thread, time::Duration};

use rusqlite::{Connection, params};

use crate::storage::StorageError;

struct DerivedTableSchema {
    table: &'static str,
    required_columns: &'static [&'static str],
}

#[derive(Debug)]
struct TableColumn {
    name: String,
    not_null: bool,
    default_value: Option<String>,
}

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
const SCHEMA_COMPATIBILITY_RETRY_DELAYS_MS: [u64; 5] = [10, 30, 90, 270, 810];

const GRAPH_BM25_COLUMNS: &[&str] = &[
    "document_id",
    "document_kind",
    "evidence_id",
    "parent_evidence_id",
    "modality",
    "created_graph_version",
    "source_scope",
    "source_path",
    "entity_labels",
    "entity_aliases",
    "content",
];

const GRAPH_SEMANTIC_COLUMNS: &[&str] = &[
    "document_id",
    "document_kind",
    "evidence_id",
    "parent_evidence_id",
    "modality",
    "created_graph_version",
    "source_scope",
    "source_path",
    "entity_labels_json",
    "content",
    "token_signature_json",
    "model",
    "dimension",
    "source_hash",
    "tokenizer_version",
];

const GRAPH_VECTOR_COLUMNS: &[&str] = &[
    "document_id",
    "document_kind",
    "evidence_id",
    "parent_evidence_id",
    "modality",
    "created_graph_version",
    "source_scope",
    "source_path",
    "entity_labels_json",
    "content",
    "vector_json",
    "model",
    "dimension",
    "source_hash",
    "tokenizer_version",
];

const CODE_GRAPH_SCHEMAS: &[DerivedTableSchema] = &[
    DerivedTableSchema {
        table: "code_files",
        required_columns: &[
            "source_scope",
            "path",
            "content_hash",
            "language_id",
            "parse_status",
            "diagnostic",
            "created_graph_version",
        ],
    },
    DerivedTableSchema {
        table: "code_symbols",
        required_columns: &[
            "source_scope",
            "path",
            "symbol_id",
            "name",
            "kind",
            "start_byte",
            "end_byte",
            "start_line",
            "end_line",
            "grammar_version",
            "query_name",
            "query_version",
            "node_kind",
            "capture_kind",
            "created_graph_version",
        ],
    },
    DerivedTableSchema {
        table: "code_references",
        required_columns: &[
            "source_scope",
            "path",
            "reference_id",
            "symbol_text",
            "kind",
            "start_byte",
            "end_byte",
            "start_line",
            "end_line",
            "resolution_state",
            "target_symbol_id",
            "grammar_version",
            "query_name",
            "query_version",
            "node_kind",
            "capture_kind",
            "created_graph_version",
        ],
    },
    DerivedTableSchema {
        table: "code_chunks",
        required_columns: &[
            "source_scope",
            "path",
            "chunk_id",
            "content",
            "start_byte",
            "end_byte",
            "start_line",
            "end_line",
            "grammar_version",
            "query_name",
            "query_version",
            "node_kind",
            "capture_kind",
            "created_graph_version",
        ],
    },
    DerivedTableSchema {
        table: "code_chunk_symbols",
        required_columns: &["source_scope", "path", "chunk_id", "symbol_id"],
    },
];

pub(super) fn prepare_existing_database(connection: &Connection) -> Result<(), StorageError> {
    for delay_ms in SCHEMA_COMPATIBILITY_RETRY_DELAYS_MS {
        match prepare_existing_database_once(connection) {
            Ok(()) => return Ok(()),
            Err(error) if schema_compatibility_error_is_retryable(&error) => {
                thread::sleep(Duration::from_millis(delay_ms));
            }
            Err(error) => return Err(error),
        }
    }

    prepare_existing_database_once(connection)
}

fn prepare_existing_database_once(connection: &Connection) -> Result<(), StorageError> {
    drop_incompatible_table(connection, "graph_bm25", GRAPH_BM25_COLUMNS)?;
    drop_incompatible_table(
        connection,
        "graph_semantic_documents",
        GRAPH_SEMANTIC_COLUMNS,
    )?;
    drop_incompatible_table(connection, "graph_vector_documents", GRAPH_VECTOR_COLUMNS)?;
    rebuild_incompatible_code_graph_tables(connection)?;
    rebuild_incompatible_index_refresh_tasks(connection)?;

    Ok(())
}

fn schema_compatibility_error_is_retryable(error: &StorageError) -> bool {
    match error {
        StorageError::Sqlite(error) => {
            schema_compatibility_error_message_is_retryable(&error.to_string())
        }
        _ => false,
    }
}

fn schema_compatibility_error_message_is_retryable(message: &str) -> bool {
    message.contains("vtable constructor failed: graph_bm25")
        || message.contains("database schema is locked")
        || message.contains("database is locked")
}

fn rebuild_incompatible_index_refresh_tasks(connection: &Connection) -> Result<(), StorageError> {
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

    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = connection
        .execute_batch(&migration)
        .map_err(StorageError::from);
    match result {
        Ok(()) => connection
            .execute_batch("COMMIT")
            .map_err(StorageError::from),
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
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

fn drop_incompatible_table(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> Result<(), StorageError> {
    if table_exists(connection, table)? && !table_has_columns(connection, table, required_columns)?
    {
        connection.execute(&format!("DROP TABLE {table}"), [])?;
    }

    Ok(())
}

fn rebuild_incompatible_code_graph_tables(connection: &Connection) -> Result<(), StorageError> {
    let any_code_graph_table_exists = CODE_GRAPH_SCHEMAS
        .iter()
        .map(|schema| table_exists(connection, schema.table))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .any(|exists| exists);
    if !any_code_graph_table_exists {
        return Ok(());
    }

    let incompatible = CODE_GRAPH_SCHEMAS
        .iter()
        .map(|schema| {
            Ok(table_exists(connection, schema.table)?
                && !table_has_columns(connection, schema.table, schema.required_columns)?)
        })
        .collect::<Result<Vec<_>, StorageError>>()?
        .into_iter()
        .any(|value| value);
    if !incompatible {
        return Ok(());
    }

    for table in [
        "code_chunk_symbols",
        "code_chunks",
        "code_references",
        "code_symbols",
        "code_files",
    ] {
        if table_exists(connection, table)? {
            connection.execute(&format!("DROP TABLE {table}"), [])?;
        }
    }

    Ok(())
}

fn table_has_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> Result<bool, StorageError> {
    let columns = table_columns(connection, table)?;

    Ok(required_columns
        .iter()
        .all(|required| columns.iter().any(|column| column == required)))
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn table_column_info(
    connection: &Connection,
    table: &str,
) -> Result<Vec<TableColumn>, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok(TableColumn {
            name: row.get(1)?,
            not_null: row.get(3)?,
            default_value: row.get(4)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, StorageError> {
    connection
        .query_row(
            "
            SELECT EXISTS (
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = ?1
            )
            ",
            params![table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

#[cfg(test)]
mod tests {
    use super::{
        schema_compatibility_error_is_retryable, schema_compatibility_error_message_is_retryable,
    };
    use crate::storage::StorageError;

    #[test]
    fn schema_compatibility_retry_is_limited_to_transient_open_errors() {
        assert!(schema_compatibility_error_message_is_retryable(
            "vtable constructor failed: graph_bm25"
        ));
        assert!(schema_compatibility_error_message_is_retryable(
            "database schema is locked"
        ));
        assert!(!schema_compatibility_error_message_is_retryable(
            "no such table: graph_bm25"
        ));
        assert!(!schema_compatibility_error_is_retryable(
            &StorageError::InvalidInput("database is locked".to_owned())
        ));
    }
}
