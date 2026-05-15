use rusqlite::{Connection, params};

use crate::storage::StorageError;

struct DerivedTableSchema {
    table: &'static str,
    required_columns: &'static [&'static str],
}

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
    drop_incompatible_table(connection, "graph_bm25", GRAPH_BM25_COLUMNS)?;
    drop_incompatible_table(
        connection,
        "graph_semantic_documents",
        GRAPH_SEMANTIC_COLUMNS,
    )?;
    drop_incompatible_table(connection, "graph_vector_documents", GRAPH_VECTOR_COLUMNS)?;
    rebuild_incompatible_code_graph_tables(connection)?;

    Ok(())
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
