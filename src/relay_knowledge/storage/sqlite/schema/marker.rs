use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::StorageError;

use super::introspection::{
    index_has_columns, table_column_is_not_null, table_columns_have_no_defaults, table_exists,
    table_has_columns, table_has_exact_columns, table_has_exact_plain_columns,
    table_has_exact_primary_key_index_surface, table_has_no_triggers,
    table_has_primary_key_columns, table_has_unique_columns,
};

const SCHEMA_MARKER_KEY: &str = "sqlite_graph_store";
// Version 8 adds the software ontology occurrence, statement, validation, and
// provenance-status surfaces. Existing databases must run the additive schema
// initializer before a v6 software projection can be published.
pub(super) const SCHEMA_MARKER_VERSION: i64 = 8;
pub(in crate::storage::sqlite) const SEARCH_OWNER_V2_MIGRATION: &str =
    "search-owner-v2-writer-and-serving-gate";
pub(in crate::storage::sqlite) const REFERENCE_SEARCH_GROUP_V2_MIGRATION: &str =
    "reference-search-group-owner-v2";
pub(in crate::storage::sqlite) const SEARCH_ORPHAN_GC_PHASE_MIGRATION: &str =
    "scope-gc-search-orphans-phase-v1";
pub(in crate::storage::sqlite) const REFERENCE_SEARCH_GROUP_GC_PHASE_MIGRATION: &str =
    "scope-gc-reference-search-groups-phase-v1";
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
const CODE_REFERENCE_SEARCH_PROGRESS_COLUMNS: &[&str] = &[
    "source_scope",
    "projection_version",
    "stage",
    "completed_page_ordinal",
    "cleanup_cursor_rowid",
    "cleanup_cursor_record_id",
    "discovery_cursor_reference_id",
    "build_cursor_group_id",
    "expected_reference_count",
    "cleanup_total_count",
    "discovered_reference_count",
    "discovered_group_count",
    "build_total_count",
    "cleaned_count",
    "built_count",
    "page_document_limit",
    "page_byte_limit",
];
const CODE_REFERENCE_RESOLUTION_PROGRESS_COLUMNS: &[&str] = &[
    "source_scope",
    "protocol_version",
    "stage",
    "completed_page_ordinal",
    "cursor_reference_id",
    "expected_reference_count",
    "resolved_reference_count",
    "page_document_limit",
    "page_byte_limit",
];
const CODE_REFERENCE_RESOLUTION_PROGRESS_DDL: &str = concat!(
    "createtablecode_repository_reference_resolution_progress(",
    "source_scopetextnotnullprimarykey,protocol_versionintegernotnullcheck(protocol_version=1),",
    "stagetextnotnullcheck(stage='resolve'),completed_page_ordinalintegernotnullcheck(completed_page_ordinal>=0),",
    "cursor_reference_idtext,expected_reference_countintegernotnullcheck(expected_reference_count>=0),",
    "resolved_reference_countintegernotnullcheck(resolved_reference_count>=0),",
    "page_document_limitintegernotnullcheck(page_document_limit>0andpage_document_limit<=32768),",
    "page_byte_limitintegernotnullcheck(page_byte_limit>0andpage_byte_limit<=16777216),",
    "check(resolved_reference_count<=expected_reference_count),",
    "foreignkey(source_scope)referencescode_repository_index_checkpoints(source_scope)ondeletecascade)"
);
const CODE_REFERENCE_SEARCH_PROGRESS_DDL: &str = concat!(
    "createtablecode_repository_reference_search_progress(",
    "source_scopetextnotnullprimarykey,projection_versionintegernotnullcheck(projection_version>0),",
    "stagetextnotnullcheck(stagein('cleanup','discover','build')),",
    "completed_page_ordinalintegernotnullcheck(completed_page_ordinal>=0),",
    "cleanup_cursor_rowidinteger,cleanup_cursor_record_idtext,discovery_cursor_reference_idtext,",
    "build_cursor_group_idtext,expected_reference_countintegernotnullcheck(expected_reference_count>=0),",
    "cleanup_total_countintegernotnullcheck(cleanup_total_count>=0),",
    "discovered_reference_countintegernotnullcheck(discovered_reference_count>=0),",
    "discovered_group_countintegernotnullcheck(discovered_group_count>=0),",
    "build_total_countintegernotnullcheck(build_total_count>=0),cleaned_countintegernotnullcheck(cleaned_count>=0),",
    "built_countintegernotnullcheck(built_count>=0),page_document_limitintegernotnullcheck(page_document_limit>0),",
    "page_byte_limitintegernotnullcheck(page_byte_limit>0),",
    "foreignkey(source_scope)referencescode_repository_index_checkpoints(source_scope)ondeletecascade)"
);
const CODE_REFERENCE_SEARCH_GROUP_COLUMNS: &[&str] = &[
    "source_scope",
    "group_id",
    "name",
    "kind",
    "path",
    "target_hint",
    "language_id",
    "occurrence_count",
];
const CODE_REFERENCE_SEARCH_MANIFEST_COLUMNS: &[&str] = &[
    "source_scope",
    "projection_version",
    "reference_count",
    "group_count",
];
const CODE_SCOPE_GC_JOB_COLUMNS: &[&str] = &[
    "source_scope",
    "repository_id",
    "phase",
    "search_rowid_cursor",
    "deleted_rows",
    "created_at_ms",
    "updated_at_ms",
    "last_error",
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
            "code_repository_scope_gc_jobs",
            CODE_SCOPE_GC_JOB_COLUMNS,
        )?
        || !code_schema_capability_markers_are_current(connection)?
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
        || !reference_search_progress_schema_is_current(connection)?
        || !reference_resolution_progress_schema_is_current(connection)?
        || !super::incremental_clone_marker::schema_is_current(connection)?
        || !reference_search_group_schema_is_current(connection)?
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

fn code_schema_capability_markers_are_current(
    connection: &Connection,
) -> Result<bool, StorageError> {
    if !table_exists(connection, "code_repository_schema_migrations")? {
        return Ok(false);
    }
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM code_repository_schema_migrations WHERE name = ?1
             ) AND EXISTS (
                 SELECT 1 FROM code_repository_schema_migrations WHERE name = ?2
             ) AND EXISTS (
                 SELECT 1 FROM code_repository_schema_migrations WHERE name = ?3
             ) AND EXISTS (
                 SELECT 1 FROM code_repository_schema_migrations WHERE name = ?4
             )",
            params![
                SEARCH_OWNER_V2_MIGRATION,
                SEARCH_ORPHAN_GC_PHASE_MIGRATION,
                REFERENCE_SEARCH_GROUP_V2_MIGRATION,
                REFERENCE_SEARCH_GROUP_GC_PHASE_MIGRATION,
            ],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite) fn reference_resolution_progress_schema_is_current(
    connection: &Connection,
) -> Result<bool, StorageError> {
    let table = "code_repository_reference_resolution_progress";
    if !table_has_exact_plain_columns(
        connection,
        table,
        CODE_REFERENCE_RESOLUTION_PROGRESS_COLUMNS,
    )? || !table_has_primary_key_columns(connection, table, &["source_scope"])?
    {
        return Ok(false);
    }
    for column in CODE_REFERENCE_RESOLUTION_PROGRESS_COLUMNS
        .iter()
        .copied()
        .filter(|column| *column != "cursor_reference_id")
    {
        if !table_column_is_not_null(connection, table, column)? {
            return Ok(false);
        }
    }
    if table_column_is_not_null(connection, table, "cursor_reference_id")? {
        return Ok(false);
    }
    let definition = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get::<_, String>(0),
        )?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    if !progress_table_has_exact_constraint_surface(
        connection,
        table,
        &definition,
        CODE_REFERENCE_RESOLUTION_PROGRESS_DDL,
    )? {
        return Ok(false);
    }
    let mut foreign_keys = connection.prepare(&format!("PRAGMA foreign_key_list({table})"))?;
    let foreign_keys = foreign_keys
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(foreign_keys.as_slice()
        == [(
            "code_repository_index_checkpoints".to_owned(),
            "source_scope".to_owned(),
            "source_scope".to_owned(),
            "CASCADE".to_owned(),
        )]
        .as_slice())
}

pub(in crate::storage::sqlite) fn reference_search_progress_schema_is_current(
    connection: &Connection,
) -> Result<bool, StorageError> {
    if !table_has_exact_plain_columns(
        connection,
        "code_repository_reference_search_progress",
        CODE_REFERENCE_SEARCH_PROGRESS_COLUMNS,
    )? || !table_has_primary_key_columns(
        connection,
        "code_repository_reference_search_progress",
        &["source_scope"],
    )? {
        return Ok(false);
    }
    for column in [
        "source_scope",
        "projection_version",
        "stage",
        "completed_page_ordinal",
        "expected_reference_count",
        "cleanup_total_count",
        "discovered_reference_count",
        "discovered_group_count",
        "build_total_count",
        "cleaned_count",
        "built_count",
        "page_document_limit",
        "page_byte_limit",
    ] {
        if !table_column_is_not_null(
            connection,
            "code_repository_reference_search_progress",
            column,
        )? {
            return Ok(false);
        }
    }
    for column in [
        "cleanup_cursor_rowid",
        "cleanup_cursor_record_id",
        "discovery_cursor_reference_id",
        "build_cursor_group_id",
    ] {
        if table_column_is_not_null(
            connection,
            "code_repository_reference_search_progress",
            column,
        )? {
            return Ok(false);
        }
    }
    let definition = connection
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'table' AND name = 'code_repository_reference_search_progress'",
            [],
            |row| row.get::<_, String>(0),
        )?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    if !progress_table_has_exact_constraint_surface(
        connection,
        "code_repository_reference_search_progress",
        &definition,
        CODE_REFERENCE_SEARCH_PROGRESS_DDL,
    )? {
        return Ok(false);
    }
    let mut foreign_keys =
        connection.prepare("PRAGMA foreign_key_list(code_repository_reference_search_progress)")?;
    let foreign_keys = foreign_keys
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(foreign_keys.as_slice()
        == [(
            "code_repository_index_checkpoints".to_owned(),
            "source_scope".to_owned(),
            "source_scope".to_owned(),
            "CASCADE".to_owned(),
        )]
        .as_slice())
}

fn progress_table_has_exact_constraint_surface(
    connection: &Connection,
    table: &str,
    normalized_definition: &str,
    expected_definition: &str,
) -> Result<bool, StorageError> {
    let compact_definition = normalized_definition
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    Ok(table_columns_have_no_defaults(connection, table)?
        && table_has_exact_primary_key_index_surface(connection, table, &["source_scope"])?
        && table_has_no_triggers(connection, table)?
        && compact_definition == expected_definition)
}

pub(in crate::storage::sqlite) fn reference_search_group_schema_is_current(
    connection: &Connection,
) -> Result<bool, StorageError> {
    if !(table_has_exact_columns(
        connection,
        "code_repository_reference_search_groups",
        CODE_REFERENCE_SEARCH_GROUP_COLUMNS,
    )? && table_has_primary_key_columns(
        connection,
        "code_repository_reference_search_groups",
        &["source_scope", "group_id"],
    )? && table_has_unique_columns(
        connection,
        "code_repository_reference_search_groups",
        &["source_scope", "name", "kind", "path", "target_hint"],
    )? && index_has_columns(
        connection,
        "code_repository_reference_search_groups_path",
        &["source_scope", "path", "group_id"],
    )? && table_has_exact_columns(
        connection,
        "code_repository_reference_search_manifests",
        CODE_REFERENCE_SEARCH_MANIFEST_COLUMNS,
    )? && table_has_primary_key_columns(
        connection,
        "code_repository_reference_search_manifests",
        &["source_scope"],
    )?) {
        return Ok(false);
    }
    let compact_sql = |object_type: &str, name: &str| -> Result<String, StorageError> {
        let definition = connection.query_row(
            "SELECT sql FROM sqlite_master WHERE type = ?1 AND name = ?2",
            params![object_type, name],
            |row| row.get::<_, String>(0),
        )?;
        Ok(definition
            .chars()
            .filter(|character| !character.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect())
    };
    let group = compact_sql("table", "code_repository_reference_search_groups")?;
    let manifest = compact_sql("table", "code_repository_reference_search_manifests")?;
    let path_index = compact_sql("index", "code_repository_reference_search_groups_path")?;
    Ok(!group.contains("collate")
        && !manifest.contains("collate")
        && !path_index.contains("collate")
        && !path_index.contains("desc")
        && [
            "source_scopetextnotnull",
            "group_idtextnotnull",
            "nametextnotnull",
            "kindtextnotnull",
            "pathtextnotnull",
            "target_hinttextnotnull",
            "language_idtextnotnull",
            "occurrence_countintegernotnullcheck(occurrence_count>0)",
            "primarykey(source_scope,group_id)",
            "unique(source_scope,name,kind,path,target_hint)",
        ]
        .iter()
        .all(|fragment| group.contains(fragment))
        && [
            "source_scopetextnotnullprimarykey",
            "projection_versionintegernotnullcheck(projection_version>0)",
            "reference_countintegernotnullcheck(reference_count>=0)",
            "group_countintegernotnullcheck(group_count>=0)",
        ]
        .iter()
        .all(|fragment| manifest.contains(fragment))
        && path_index
            .contains("oncode_repository_reference_search_groups(source_scope,path,group_id)"))
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

#[cfg(test)]
#[path = "reference_resolution_progress_tests.rs"]
mod reference_resolution_progress_tests;
