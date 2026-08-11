use std::{thread, time::Duration};

use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::StorageError;

use super::introspection::{
    TableColumn, index_has_columns, table_column_info, table_column_is_not_null, table_exists,
    table_has_columns, table_has_exact_columns, table_has_primary_key_columns,
    table_has_unique_columns,
};

struct DerivedTableSchema {
    table: &'static str,
    required_columns: &'static [&'static str],
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
    "routing_key",
    "source_scope",
    "source_path",
    "entity_labels",
    "entity_aliases",
    "content",
];
const GRAPH_BM25_ROUTE_STATE_COLUMNS: &[&str] = &[
    "id",
    "indexed_graph_version",
    "document_count",
    "state",
    "algorithm_version",
    "semantic_generation",
    "vector_generation",
    "rebuild_phase",
    "rebuild_cursor",
    "rebuild_semantic",
    "rebuild_vector",
    "rebuild_owner",
    "rebuild_lease_expires_at_ms",
];
const GRAPH_BM25_ROUTE_DOCUMENT_COLUMNS: &[&str] = &[
    "document_id",
    "fts_rowid",
    "document_kind",
    "created_graph_version",
    "source_scope",
    "source_path",
    "label_gram_state",
    "group_token",
    "term_counts_json",
];
const GRAPH_BM25_ROUTE_GROUP_COLUMNS: &[&str] = &["source_scope", "group_token", "document_count"];
const GRAPH_BM25_ROUTE_TERM_COLUMNS: &[&str] = &[
    "term",
    "source_scope",
    "group_token",
    "collection_frequency",
];
const GRAPH_BM25_ROUTE_TERM_TOTAL_COLUMNS: &[&str] = &["term", "document_frequency"];
const GRAPH_BM25_ROUTE_PATH_INDEX: &str = "graph_bm25_route_documents_scope_path";
const GRAPH_BM25_ROUTE_PATH_INDEX_COLUMNS: &[&str] =
    &["source_scope", "source_path", "document_id"];
const GRAPH_BM25_LABEL_STATE_INDEX: &str = "graph_bm25_route_documents_label_state";
const GRAPH_BM25_LABEL_STATE_INDEX_COLUMNS: &[&str] = &[
    "label_gram_state",
    "source_scope",
    "created_graph_version",
    "document_id",
];
const GRAPH_BM25_GLOBAL_LABEL_STATE_INDEX: &str = "graph_bm25_route_documents_global_label_state";
const GRAPH_BM25_GLOBAL_LABEL_STATE_INDEX_COLUMNS: &[&str] =
    &["label_gram_state", "created_graph_version", "document_id"];
const GRAPH_BM25_LABEL_GRAM_SCOPED_INDEX: &str = "graph_bm25_label_grams_lookup";
const GRAPH_BM25_LABEL_GRAM_SCOPED_INDEX_COLUMNS: &[&str] = &[
    "source_scope",
    "gram_size",
    "gram",
    "label_len",
    "created_graph_version",
];
const GRAPH_BM25_LABEL_GRAM_GLOBAL_INDEX: &str = "graph_bm25_label_grams_global_lookup";
const GRAPH_BM25_LABEL_GRAM_GLOBAL_INDEX_COLUMNS: &[&str] = &[
    "gram_size",
    "gram",
    "label_len",
    "created_graph_version",
    "source_scope",
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
const GRAPH_SEMANTIC_SCOPE_INDEX: &str = "graph_semantic_documents_scope_version";
const GRAPH_SEMANTIC_GLOBAL_INDEX: &str = "graph_semantic_documents_version";
const GRAPH_VECTOR_SCOPE_INDEX: &str = "graph_vector_documents_scope_version";
const GRAPH_SCOPE_VERSION_INDEX_COLUMNS: &[&str] = &["source_scope", "created_graph_version"];
const GRAPH_GLOBAL_VERSION_INDEX_COLUMNS: &[&str] = &["created_graph_version", "document_id"];
const GRAPH_BM25_LABEL_GRAM_COLUMNS: &[&str] = &[
    "document_id",
    "document_kind",
    "source_scope",
    "created_graph_version",
    "label",
    "label_lower",
    "label_len",
    "gram_size",
    "gram",
];
const GRAPH_BM25_LABEL_LOOKUP_INDEX: &str = "graph_bm25_label_grams_label_lookup";
const GRAPH_BM25_LABEL_LOOKUP_INDEX_COLUMNS: &[&str] = &[
    "label_lower",
    "source_scope",
    "created_graph_version",
    "document_id",
];
const GRAPH_BM25_GLOBAL_LABEL_LOOKUP_INDEX: &str = "graph_bm25_label_grams_global_label_lookup";
const GRAPH_BM25_GLOBAL_LABEL_LOOKUP_INDEX_COLUMNS: &[&str] = &[
    "label_lower",
    "created_graph_version",
    "source_scope",
    "document_id",
];
const CODE_WORKSPACE_PACKAGE_MAPPING_COLUMNS: &[&str] = &[
    "set_id",
    "package_name",
    "ecosystem",
    "repository_id",
    "source_scope",
    "workspace_format",
    "created_at_ms",
];
const CODE_WORKSPACE_PACKAGE_MAPPING_UNIQUE: &[&str] = &["set_id", "package_name", "ecosystem"];

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

pub(in crate::storage::sqlite) fn prepare_existing_database(
    connection: &Connection,
) -> Result<(), StorageError> {
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
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = prepare_existing_database_in_transaction(connection);
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

fn prepare_existing_database_in_transaction(connection: &Connection) -> Result<(), StorageError> {
    connection.execute("DROP TABLE IF EXISTS graph_bm25_vocabulary", [])?;
    connection.execute("DROP TABLE IF EXISTS graph_bm25_retired", [])?;
    let bm25_routing_was_compatible = bm25_routing_schema_is_compatible(connection)?;
    let graph_bm25_was_compatible =
        table_exists(connection, "graph_bm25")? && graph_bm25_schema_is_compatible(connection)?;
    let rebuild_checkpoint_was_compatible = bm25_routing_was_compatible
        && table_exists(connection, "graph_bm25_rebuild")?
        && named_graph_bm25_schema_is_compatible(connection, "graph_bm25_rebuild")?;
    let companion_tables_were_compatible = [
        (
            "graph_semantic_documents",
            GRAPH_SEMANTIC_COLUMNS,
            &["document_id"][..],
        ),
        (
            "graph_vector_documents",
            GRAPH_VECTOR_COLUMNS,
            &["document_id"][..],
        ),
        (
            "graph_bm25_label_grams",
            GRAPH_BM25_LABEL_GRAM_COLUMNS,
            &["document_id", "label_lower", "gram_size", "gram"][..],
        ),
    ]
    .into_iter()
    .map(|(table, columns, key)| {
        Ok(table_has_exact_columns(connection, table, columns)?
            && table_has_primary_key_columns(connection, table, key)?)
    })
    .collect::<Result<Vec<_>, StorageError>>()?
    .into_iter()
    .all(|compatible| compatible);
    let code_tables_were_compatible = CODE_GRAPH_SCHEMAS
        .iter()
        .map(|schema| {
            Ok(table_exists(connection, schema.table)?
                && table_has_columns(connection, schema.table, schema.required_columns)?)
        })
        .collect::<Result<Vec<_>, StorageError>>()?
        .into_iter()
        .all(|compatible| compatible);
    drop_incompatible_bm25_routing(connection)?;
    let production_dependencies_were_compatible = bm25_routing_was_compatible
        && graph_bm25_was_compatible
        && companion_tables_were_compatible
        && code_tables_were_compatible;
    invalidate_bm25_routing_after_schema_change(
        connection,
        !production_dependencies_were_compatible,
        production_dependencies_were_compatible && rebuild_checkpoint_was_compatible,
    )?;
    connection.execute_batch(
        "DROP INDEX IF EXISTS graph_bm25_route_documents_scope_group;
         DROP INDEX IF EXISTS graph_bm25_route_terms_group;
         DROP INDEX IF EXISTS graph_bm25_label_grams_document;",
    )?;
    if !index_has_columns(
        connection,
        GRAPH_BM25_ROUTE_PATH_INDEX,
        GRAPH_BM25_ROUTE_PATH_INDEX_COLUMNS,
    )? {
        connection.execute(
            "DROP INDEX IF EXISTS graph_bm25_route_documents_scope_path",
            [],
        )?;
    }
    for (index, columns) in [
        (
            GRAPH_BM25_LABEL_STATE_INDEX,
            GRAPH_BM25_LABEL_STATE_INDEX_COLUMNS,
        ),
        (
            GRAPH_BM25_GLOBAL_LABEL_STATE_INDEX,
            GRAPH_BM25_GLOBAL_LABEL_STATE_INDEX_COLUMNS,
        ),
    ] {
        if !index_has_columns(connection, index, columns)? {
            connection.execute(&format!("DROP INDEX IF EXISTS {index}"), [])?;
        }
    }
    for (index, columns) in [
        (
            GRAPH_BM25_LABEL_GRAM_SCOPED_INDEX,
            GRAPH_BM25_LABEL_GRAM_SCOPED_INDEX_COLUMNS,
        ),
        (
            GRAPH_BM25_LABEL_GRAM_GLOBAL_INDEX,
            GRAPH_BM25_LABEL_GRAM_GLOBAL_INDEX_COLUMNS,
        ),
    ] {
        if !index_has_columns(connection, index, columns)? {
            connection.execute(&format!("DROP INDEX IF EXISTS {index}"), [])?;
        }
    }
    drop_incompatible_exact_primary_key_table(
        connection,
        "graph_semantic_documents",
        GRAPH_SEMANTIC_COLUMNS,
        &["document_id"],
    )?;
    drop_incompatible_exact_primary_key_table(
        connection,
        "graph_vector_documents",
        GRAPH_VECTOR_COLUMNS,
        &["document_id"],
    )?;
    drop_incompatible_exact_primary_key_table(
        connection,
        "graph_bm25_label_grams",
        GRAPH_BM25_LABEL_GRAM_COLUMNS,
        &["document_id", "label_lower", "gram_size", "gram"],
    )?;
    for (index, columns) in [
        (
            GRAPH_SEMANTIC_SCOPE_INDEX,
            GRAPH_SCOPE_VERSION_INDEX_COLUMNS,
        ),
        (
            GRAPH_SEMANTIC_GLOBAL_INDEX,
            GRAPH_GLOBAL_VERSION_INDEX_COLUMNS,
        ),
        (GRAPH_VECTOR_SCOPE_INDEX, GRAPH_SCOPE_VERSION_INDEX_COLUMNS),
        (
            GRAPH_BM25_LABEL_LOOKUP_INDEX,
            GRAPH_BM25_LABEL_LOOKUP_INDEX_COLUMNS,
        ),
        (
            GRAPH_BM25_GLOBAL_LABEL_LOOKUP_INDEX,
            GRAPH_BM25_GLOBAL_LABEL_LOOKUP_INDEX_COLUMNS,
        ),
    ] {
        if !index_has_columns(connection, index, columns)? {
            connection.execute(&format!("DROP INDEX IF EXISTS {index}"), [])?;
        }
    }
    rebuild_incompatible_code_graph_tables(connection)?;
    rebuild_incompatible_index_refresh_tasks(connection)?;
    rebuild_incompatible_workspace_package_mappings(connection)?;

    Ok(())
}

fn invalidate_bm25_routing_after_schema_change(
    connection: &Connection,
    routing_schema_changed: bool,
    resumable_building_generation: bool,
) -> Result<(), StorageError> {
    if !table_exists(connection, "relay_storage_schema_state")?
        || !table_exists(connection, "graph_bm25_route_state")?
    {
        return Ok(());
    }
    let version = connection
        .query_row(
            "SELECT version
             FROM relay_storage_schema_state
             WHERE key = 'sqlite_graph_store'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let marker_changed = version != Some(super::marker::SCHEMA_MARKER_VERSION);
    connection.execute(
        "UPDATE graph_bm25_route_state
         SET state = 'stale'
         WHERE id = 1
           AND (
               (state = 'building' AND NOT ?1)
               OR (state <> 'building' AND (?2 OR ?3))
           )",
        params![
            resumable_building_generation,
            routing_schema_changed,
            marker_changed
        ],
    )?;
    Ok(())
}

fn bm25_routing_schema_is_compatible(connection: &Connection) -> Result<bool, StorageError> {
    for (table, columns) in [
        ("graph_bm25_route_state", GRAPH_BM25_ROUTE_STATE_COLUMNS),
        (
            "graph_bm25_route_documents",
            GRAPH_BM25_ROUTE_DOCUMENT_COLUMNS,
        ),
        ("graph_bm25_route_groups", GRAPH_BM25_ROUTE_GROUP_COLUMNS),
        ("graph_bm25_route_terms", GRAPH_BM25_ROUTE_TERM_COLUMNS),
        (
            "graph_bm25_route_term_totals",
            GRAPH_BM25_ROUTE_TERM_TOTAL_COLUMNS,
        ),
    ] {
        if !bm25_route_table_is_compatible(connection, table, columns)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn drop_incompatible_bm25_routing(connection: &Connection) -> Result<(), StorageError> {
    for (table, columns) in [
        ("graph_bm25_route_state", GRAPH_BM25_ROUTE_STATE_COLUMNS),
        (
            "graph_bm25_route_documents",
            GRAPH_BM25_ROUTE_DOCUMENT_COLUMNS,
        ),
        ("graph_bm25_route_groups", GRAPH_BM25_ROUTE_GROUP_COLUMNS),
        ("graph_bm25_route_terms", GRAPH_BM25_ROUTE_TERM_COLUMNS),
        (
            "graph_bm25_route_term_totals",
            GRAPH_BM25_ROUTE_TERM_TOTAL_COLUMNS,
        ),
    ] {
        if table_exists(connection, table)?
            && !bm25_route_table_is_compatible(connection, table, columns)?
        {
            connection.execute(&format!("DROP TABLE {table}"), [])?;
        }
    }
    Ok(())
}

fn bm25_route_table_is_compatible(
    connection: &Connection,
    table: &str,
    columns: &[&str],
) -> Result<bool, StorageError> {
    let columns_are_compatible = table_has_exact_columns(connection, table, columns)?;
    if !columns_are_compatible {
        return Ok(false);
    }
    let primary_key = match table {
        "graph_bm25_route_state" => Some(&["id"][..]),
        "graph_bm25_route_documents" => Some(&["document_id"][..]),
        "graph_bm25_route_groups" => Some(&["source_scope", "group_token"][..]),
        "graph_bm25_route_terms" => Some(&["term", "source_scope", "group_token"][..]),
        "graph_bm25_route_term_totals" => Some(&["term"][..]),
        _ => None,
    };
    match primary_key {
        Some(primary_key) => {
            if !table_has_primary_key_columns(connection, table, primary_key)? {
                return Ok(false);
            }
            if table == "graph_bm25_route_documents" {
                return Ok(table_column_is_not_null(connection, table, "fts_rowid")?
                    && table_has_unique_columns(connection, table, &["fts_rowid"])?);
            }
            Ok(true)
        }
        None => Ok(true),
    }
}

fn drop_incompatible_exact_primary_key_table(
    connection: &Connection,
    table: &str,
    columns: &[&str],
    primary_key: &[&str],
) -> Result<(), StorageError> {
    if table_exists(connection, table)?
        && (!table_has_exact_columns(connection, table, columns)?
            || !table_has_primary_key_columns(connection, table, primary_key)?)
    {
        connection.execute(&format!("DROP TABLE {table}"), [])?;
    }
    Ok(())
}

fn graph_bm25_schema_is_compatible(connection: &Connection) -> Result<bool, StorageError> {
    named_graph_bm25_schema_is_compatible(connection, "graph_bm25")
}

fn named_graph_bm25_schema_is_compatible(
    connection: &Connection,
    table: &str,
) -> Result<bool, StorageError> {
    if !table_has_exact_columns(connection, table, GRAPH_BM25_COLUMNS)? {
        return Ok(false);
    }
    let definition = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |row| row.get::<_, String>(0),
        )?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    Ok(definition.contains("using fts5")
        && [
            "content=",
            "content =",
            "tokenize=",
            "tokenize =",
            "prefix=",
            "prefix =",
            "detail=",
            "detail =",
            "columnsize=",
            "columnsize =",
        ]
        .iter()
        .all(|option| !definition.contains(option))
        && [
            "document_id",
            "document_kind",
            "evidence_id",
            "parent_evidence_id",
            "modality",
            "created_graph_version",
        ]
        .iter()
        .all(|column| definition.contains(&format!("{column} unindexed")))
        && [
            "routing_key",
            "source_scope",
            "source_path",
            "entity_labels",
            "entity_aliases",
            "content",
        ]
        .iter()
        .all(|column| !definition.contains(&format!("{column} unindexed"))))
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

fn rebuild_incompatible_workspace_package_mappings(
    connection: &Connection,
) -> Result<(), StorageError> {
    if !table_exists(connection, "code_workspace_package_mappings")? {
        return Ok(());
    }
    if !table_has_columns(
        connection,
        "code_workspace_package_mappings",
        CODE_WORKSPACE_PACKAGE_MAPPING_COLUMNS,
    )? {
        connection.execute("DROP TABLE code_workspace_package_mappings", [])?;
        return Ok(());
    }
    if table_has_unique_columns(
        connection,
        "code_workspace_package_mappings",
        CODE_WORKSPACE_PACKAGE_MAPPING_UNIQUE,
    )? {
        return Ok(());
    }

    connection.execute_batch(
        "
        DROP TABLE IF EXISTS code_workspace_package_mappings_rebuild_legacy;
        ALTER TABLE code_workspace_package_mappings
            RENAME TO code_workspace_package_mappings_rebuild_legacy;
        CREATE TABLE code_workspace_package_mappings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            set_id TEXT NOT NULL,
            package_name TEXT NOT NULL,
            ecosystem TEXT NOT NULL,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            workspace_format TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            UNIQUE (set_id, package_name, ecosystem)
        );
        INSERT OR IGNORE INTO code_workspace_package_mappings (
            id, set_id, package_name, ecosystem, repository_id, source_scope,
            workspace_format, created_at_ms
        )
        SELECT id, set_id, package_name, ecosystem, repository_id, source_scope,
               workspace_format, created_at_ms
        FROM code_workspace_package_mappings_rebuild_legacy
        ORDER BY created_at_ms DESC, id DESC;
        DROP TABLE code_workspace_package_mappings_rebuild_legacy;
        ",
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod tests;
