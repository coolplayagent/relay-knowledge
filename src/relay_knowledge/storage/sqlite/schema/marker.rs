use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::StorageError;

use super::introspection::{
    index_has_columns, table_column_is_not_null, table_exists, table_has_columns,
    table_has_exact_columns, table_has_primary_key_columns, table_has_unique_columns,
};

const SCHEMA_MARKER_KEY: &str = "sqlite_graph_store";
pub(super) const SCHEMA_MARKER_VERSION: i64 = 6;
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
const CODE_REPOSITORY_FILES_COLUMNS: &[&str] = &[
    "repository_id",
    "source_scope",
    "file_id",
    "path",
    "language_id",
    "blob_hash",
    "byte_len",
    "line_count",
    "parse_status",
    "is_generated",
    "degraded_reason",
];
const FILE_INDEX_ROOT_COLUMNS: &[&str] = &[
    "scope_id",
    "root_id",
    "root_path",
    "indexed_file_count",
    "missing_file_count",
    "scan_error_count",
    "truncated",
    "content_truncated",
    "content_read_error_count",
    "indexed_content_count",
    "skipped_content_count",
    "unchanged_content_count",
    "stale_content_cursor_count",
    "last_indexed_at_ms",
    "last_error",
];
const FILE_CONTENT_ENTRY_COLUMNS: &[&str] = &[
    "entry_key",
    "scope_id",
    "root_id",
    "path",
    "relative_path",
    "fingerprint",
    "content_hash",
    "indexed_at_ms",
    "graph_version",
    "status",
    "skipped_reason",
];
const FILE_CONTENT_CHUNK_COLUMNS: &[&str] = &[
    "chunk_id",
    "entry_key",
    "chunk_index",
    "start_byte",
    "end_byte",
    "start_line",
    "end_line",
    "content",
];
const FILE_CONTENT_CURSOR_COLUMNS: &[&str] = &[
    "cursor_key",
    "kind",
    "scope_id",
    "root_id",
    "path",
    "content_hash",
    "indexed_graph_version",
    "state",
    "stale_reason",
    "updated_at_ms",
];

pub(in crate::storage::sqlite) fn schema_initialization_is_current(
    connection: &Connection,
) -> Result<bool, StorageError> {
    if !schema_marker_table_exists(connection)? {
        return Ok(false);
    }
    let version = connection
        .query_row(
            "
            SELECT version
            FROM relay_storage_schema_state
            WHERE key = ?1
            ",
            params![SCHEMA_MARKER_KEY],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;

    if version != Some(SCHEMA_MARKER_VERSION) {
        return Ok(false);
    }
    if !graph_bm25_schema_is_current(connection)?
        || table_exists(connection, "graph_bm25_vocabulary")?
        || table_exists(connection, "graph_bm25_retired")?
        || !table_has_exact_columns(
            connection,
            "graph_bm25_route_state",
            GRAPH_BM25_ROUTE_STATE_COLUMNS,
        )?
        || !table_has_exact_columns(
            connection,
            "graph_bm25_route_documents",
            GRAPH_BM25_ROUTE_DOCUMENT_COLUMNS,
        )?
        || !table_has_exact_columns(
            connection,
            "graph_bm25_route_groups",
            GRAPH_BM25_ROUTE_GROUP_COLUMNS,
        )?
        || !table_has_exact_columns(
            connection,
            "graph_bm25_route_terms",
            GRAPH_BM25_ROUTE_TERM_COLUMNS,
        )?
        || !table_has_exact_columns(
            connection,
            "graph_bm25_route_term_totals",
            GRAPH_BM25_ROUTE_TERM_TOTAL_COLUMNS,
        )?
        || !bm25_route_primary_keys_are_current(connection)?
        || !index_has_columns(
            connection,
            GRAPH_BM25_ROUTE_PATH_INDEX,
            GRAPH_BM25_ROUTE_PATH_INDEX_COLUMNS,
        )?
        || !index_has_columns(
            connection,
            GRAPH_BM25_LABEL_STATE_INDEX,
            GRAPH_BM25_LABEL_STATE_INDEX_COLUMNS,
        )?
        || !index_has_columns(
            connection,
            GRAPH_BM25_GLOBAL_LABEL_STATE_INDEX,
            GRAPH_BM25_GLOBAL_LABEL_STATE_INDEX_COLUMNS,
        )?
        || !table_has_exact_columns(
            connection,
            "graph_semantic_documents",
            GRAPH_SEMANTIC_COLUMNS,
        )?
        || !table_has_primary_key_columns(connection, "graph_semantic_documents", &["document_id"])?
        || !index_has_columns(
            connection,
            GRAPH_SEMANTIC_SCOPE_INDEX,
            GRAPH_SCOPE_VERSION_INDEX_COLUMNS,
        )?
        || !index_has_columns(
            connection,
            GRAPH_SEMANTIC_GLOBAL_INDEX,
            GRAPH_GLOBAL_VERSION_INDEX_COLUMNS,
        )?
        || !table_has_exact_columns(connection, "graph_vector_documents", GRAPH_VECTOR_COLUMNS)?
        || !table_has_primary_key_columns(connection, "graph_vector_documents", &["document_id"])?
        || !index_has_columns(
            connection,
            GRAPH_VECTOR_SCOPE_INDEX,
            GRAPH_SCOPE_VERSION_INDEX_COLUMNS,
        )?
        || !table_has_exact_columns(
            connection,
            "graph_bm25_label_grams",
            GRAPH_BM25_LABEL_GRAM_COLUMNS,
        )?
        || !table_has_primary_key_columns(
            connection,
            "graph_bm25_label_grams",
            &["document_id", "label_lower", "gram_size", "gram"],
        )?
        || !index_has_columns(
            connection,
            GRAPH_BM25_LABEL_GRAM_SCOPED_INDEX,
            GRAPH_BM25_LABEL_GRAM_SCOPED_INDEX_COLUMNS,
        )?
        || !index_has_columns(
            connection,
            GRAPH_BM25_LABEL_GRAM_GLOBAL_INDEX,
            GRAPH_BM25_LABEL_GRAM_GLOBAL_INDEX_COLUMNS,
        )?
        || !index_has_columns(
            connection,
            GRAPH_BM25_LABEL_LOOKUP_INDEX,
            GRAPH_BM25_LABEL_LOOKUP_INDEX_COLUMNS,
        )?
        || !index_has_columns(
            connection,
            GRAPH_BM25_GLOBAL_LABEL_LOOKUP_INDEX,
            GRAPH_BM25_GLOBAL_LABEL_LOOKUP_INDEX_COLUMNS,
        )?
        || !workspace_package_mappings_current(connection)?
        || !table_has_columns(
            connection,
            "relay_sqlite_maintenance_diagnostics",
            &["id", "last_maintenance_at_ms", "last_maintenance_error"],
        )?
        || !table_has_columns(
            connection,
            "code_repository_files",
            CODE_REPOSITORY_FILES_COLUMNS,
        )?
        || !table_has_columns(
            connection,
            "code_repository_retention_activity",
            &["repository_id", "source_scope", "activity_ms"],
        )?
        || !table_has_columns(
            connection,
            "code_repository_retention_activity_dirty",
            &["repository_id"],
        )?
        || !index_has_columns(
            connection,
            "code_repository_retention_activity_order",
            &["activity_ms", "repository_id"],
        )?
        || !table_has_columns(connection, "file_index_roots", FILE_INDEX_ROOT_COLUMNS)?
        || !table_has_columns(
            connection,
            "file_content_entries",
            FILE_CONTENT_ENTRY_COLUMNS,
        )?
        || !table_has_columns(
            connection,
            "file_content_chunks",
            FILE_CONTENT_CHUNK_COLUMNS,
        )?
        || !table_has_columns(
            connection,
            "file_content_cursors",
            FILE_CONTENT_CURSOR_COLUMNS,
        )?
        || !table_has_columns(connection, "file_content_search", &["chunk_id", "content"])?
    {
        return Ok(false);
    }
    if !super::super::retrieval::derived_documents_current(connection)? {
        return Ok(false);
    }
    if !fact_evidence_links_are_current(connection)? {
        return Ok(false);
    }

    Ok(true)
}

fn graph_bm25_schema_is_current(connection: &Connection) -> Result<bool, StorageError> {
    if !table_has_exact_columns(connection, "graph_bm25", GRAPH_BM25_COLUMNS)? {
        return Ok(false);
    }
    let definition = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'graph_bm25'",
            [],
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

fn bm25_route_primary_keys_are_current(connection: &Connection) -> Result<bool, StorageError> {
    for (table, primary_key) in [
        ("graph_bm25_route_state", &["id"][..]),
        ("graph_bm25_route_documents", &["document_id"][..]),
        (
            "graph_bm25_route_groups",
            &["source_scope", "group_token"][..],
        ),
        (
            "graph_bm25_route_terms",
            &["term", "source_scope", "group_token"][..],
        ),
        ("graph_bm25_route_term_totals", &["term"][..]),
    ] {
        if !table_has_primary_key_columns(connection, table, primary_key)? {
            return Ok(false);
        }
    }
    Ok(
        table_column_is_not_null(connection, "graph_bm25_route_documents", "fts_rowid")?
            && table_has_unique_columns(connection, "graph_bm25_route_documents", &["fts_rowid"])?,
    )
}

pub(super) fn initialize_schema_marker(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS relay_storage_schema_state (
            key TEXT PRIMARY KEY,
            version INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        ",
    )?;

    Ok(())
}

pub(in crate::storage::sqlite) fn mark_schema_initialization_current(
    connection: &Connection,
) -> Result<(), StorageError> {
    initialize_schema_marker(connection)?;
    connection.execute(
        "
        INSERT INTO relay_storage_schema_state (key, version, updated_at_ms)
        VALUES (?1, ?2, CAST(strftime('%s', 'now') AS INTEGER) * 1000)
        ON CONFLICT(key) DO UPDATE SET
            version = excluded.version,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![SCHEMA_MARKER_KEY, SCHEMA_MARKER_VERSION],
    )?;

    Ok(())
}

fn schema_marker_table_exists(connection: &Connection) -> Result<bool, StorageError> {
    table_exists(connection, "relay_storage_schema_state")
}

fn workspace_package_mappings_current(connection: &Connection) -> Result<bool, StorageError> {
    if !table_has_columns(
        connection,
        "code_workspace_package_mappings",
        CODE_WORKSPACE_PACKAGE_MAPPING_COLUMNS,
    )? {
        return Ok(false);
    }
    table_has_unique_columns(
        connection,
        "code_workspace_package_mappings",
        CODE_WORKSPACE_PACKAGE_MAPPING_UNIQUE,
    )
}

fn fact_evidence_links_are_current(connection: &Connection) -> Result<bool, StorageError> {
    if !table_exists(connection, "graph_fact_evidence")? {
        return Ok(false);
    }
    for (fact_kind, table) in [
        ("relation", "graph_relations"),
        ("claim", "graph_claims"),
        ("event", "graph_events"),
    ] {
        if !fact_evidence_links_are_current_for_kind(connection, fact_kind, table)? {
            return Ok(false);
        }
    }

    Ok(true)
}

fn fact_evidence_links_are_current_for_kind(
    connection: &Connection,
    fact_kind: &'static str,
    table: &'static str,
) -> Result<bool, StorageError> {
    if !table_exists(connection, table)? {
        return Ok(true);
    }
    let mut statement =
        connection.prepare(&format!("SELECT id, evidence_ids_json FROM {table}"))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let facts = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    for (fact_id, evidence_json) in facts {
        let evidence_ids: Vec<String> = serde_json::from_str(&evidence_json)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
        for evidence_id in evidence_ids {
            if !fact_evidence_link_exists(connection, fact_kind, &fact_id, &evidence_id)? {
                return Ok(false);
            }
        }
    }

    Ok(true)
}

fn fact_evidence_link_exists(
    connection: &Connection,
    fact_kind: &str,
    fact_id: &str,
    evidence_id: &str,
) -> Result<bool, StorageError> {
    connection
        .query_row(
            "
            SELECT EXISTS (
                SELECT 1
                FROM graph_fact_evidence
                WHERE fact_kind = ?1
                  AND fact_id = ?2
                  AND evidence_id = ?3
            )
            ",
            params![fact_kind, fact_id, evidence_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

#[cfg(test)]
#[path = "marker_tests.rs"]
mod tests;
