use super::{
    index_has_columns, mark_schema_initialization_current, schema_initialization_is_current,
    table_column_is_not_null, table_has_unique_columns,
};

#[test]
fn schema_marker_reports_current_only_after_successful_mark() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");

    assert!(
        !schema_initialization_is_current(&connection).expect("missing marker should be readable")
    );

    mark_schema_initialization_current(&connection).expect("marker should write");

    assert!(
        schema_initialization_is_current(&connection).expect("current marker should be readable")
    );
}

#[test]
fn schema_marker_requires_label_gram_table() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    mark_schema_initialization_current(&connection).expect("marker should write");

    connection
        .execute("DROP TABLE graph_bm25_label_grams", [])
        .expect("label gram table should drop");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("missing label gram table should be detected")
    );
}

#[test]
fn schema_marker_requires_the_bounded_route_path_index() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    mark_schema_initialization_current(&connection).expect("marker should write");
    connection
        .execute("DROP INDEX graph_bm25_route_documents_scope_path", [])
        .expect("route path index should drop");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("missing route path index should be detected")
    );
}

#[test]
fn bm25_hierarchy_suite_rejects_partial_or_nullable_rowid_invariants() {
    let connection = rusqlite::Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "CREATE TABLE route_documents (
                 document_id TEXT PRIMARY KEY,
                 fts_rowid INTEGER
             );
             CREATE UNIQUE INDEX route_documents_rowid_partial
             ON route_documents(fts_rowid) WHERE fts_rowid IS NOT NULL;
             CREATE INDEX route_documents_path_partial
             ON route_documents(document_id) WHERE document_id <> '';",
        )
        .expect("adversarial rowid schema should create");

    assert!(
        !table_column_is_not_null(&connection, "route_documents", "fts_rowid")
            .expect("rowid nullability should inspect")
    );
    assert!(
        !table_has_unique_columns(&connection, "route_documents", &["fts_rowid"])
            .expect("partial uniqueness should inspect")
    );
    assert!(
        !index_has_columns(
            &connection,
            "route_documents_path_partial",
            &["document_id"]
        )
        .expect("partial lookup index should inspect")
    );
}

#[test]
fn schema_marker_requires_versioned_label_gram_state_index() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    mark_schema_initialization_current(&connection).expect("marker should write");
    connection
        .execute_batch(
            "DROP INDEX graph_bm25_route_documents_label_state;
             CREATE INDEX graph_bm25_route_documents_label_state
             ON graph_bm25_route_documents(
                 label_gram_state, source_scope, document_id
             );",
        )
        .expect("unversioned label state index should replace current index");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("unversioned label state index should be detected")
    );
}

#[test]
fn bm25_hierarchy_suite_marker_requires_version_leading_global_fallback_indexes() {
    let replacements = [
        (
            "graph_semantic_documents_version",
            "CREATE INDEX graph_semantic_documents_version
             ON graph_semantic_documents(document_id, created_graph_version)",
        ),
        (
            "graph_bm25_route_documents_global_label_state",
            "CREATE INDEX graph_bm25_route_documents_global_label_state
             ON graph_bm25_route_documents(
                 label_gram_state, source_scope, created_graph_version, document_id
             )",
        ),
        (
            "graph_bm25_label_grams_global_label_lookup",
            "CREATE INDEX graph_bm25_label_grams_global_label_lookup
             ON graph_bm25_label_grams(
                 label_lower, source_scope, created_graph_version, document_id
             )",
        ),
    ];

    for (index, replacement) in replacements {
        let store = crate::storage::SqliteGraphStore::open_in_memory()
            .expect("store should open with current retrieval indexes");
        let connection = store.connection.lock().expect("connection should lock");
        mark_schema_initialization_current(&connection).expect("marker should write");
        connection
            .execute(&format!("DROP INDEX {index}"), [])
            .expect("current global fallback index should drop");
        connection
            .execute(replacement, [])
            .expect("misordered global fallback index should create");

        assert!(
            !schema_initialization_is_current(&connection)
                .expect("misordered global fallback index should be inspected"),
            "marker accepted misordered index {index}"
        );
    }
}

#[test]
fn schema_marker_requires_bounded_label_gram_lookup_indexes() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    mark_schema_initialization_current(&connection).expect("marker should write");
    connection
        .execute("DROP INDEX graph_bm25_label_grams_global_lookup", [])
        .expect("global label lookup index should drop");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("missing label lookup index should be detected")
    );
}

#[test]
fn schema_marker_rejects_obsolete_route_term_aggregates() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    mark_schema_initialization_current(&connection).expect("marker should write");
    connection
        .execute(
            "ALTER TABLE graph_bm25_route_terms
             ADD COLUMN document_frequency INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .expect("obsolete aggregate column should add");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("obsolete route term schema should be detected")
    );
}

#[test]
fn schema_marker_rejects_route_tables_without_required_primary_keys() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    mark_schema_initialization_current(&connection).expect("marker should write");
    connection
        .execute_batch(
            "DROP TABLE graph_bm25_route_groups;
             CREATE TABLE graph_bm25_route_groups (
                 source_scope TEXT NOT NULL,
                 group_token TEXT NOT NULL,
                 document_count INTEGER NOT NULL
             );",
        )
        .expect("constraint-free route group table should replace current table");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("missing route primary key should be detected")
    );
}

#[test]
fn schema_marker_rejects_reordered_bm25_weight_columns() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    mark_schema_initialization_current(&connection).expect("marker should write");
    connection
        .execute_batch(
            "DROP TABLE graph_bm25;
             CREATE VIRTUAL TABLE graph_bm25 USING fts5(
                 document_kind UNINDEXED,
                 document_id UNINDEXED,
                 evidence_id UNINDEXED,
                 parent_evidence_id UNINDEXED,
                 modality UNINDEXED,
                 created_graph_version UNINDEXED,
                 routing_key,
                 source_scope,
                 source_path,
                 entity_labels,
                 entity_aliases,
                 content
             );",
        )
        .expect("reordered BM25 table should replace current table");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("reordered BM25 columns should be detected")
    );
}

#[test]
fn schema_marker_rejects_unindexed_bm25_business_columns() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    mark_schema_initialization_current(&connection).expect("marker should write");
    connection
        .execute_batch(
            "DROP TABLE graph_bm25;
             CREATE VIRTUAL TABLE graph_bm25 USING fts5(
                 document_id UNINDEXED,
                 document_kind UNINDEXED,
                 evidence_id UNINDEXED,
                 parent_evidence_id UNINDEXED,
                 modality UNINDEXED,
                 created_graph_version UNINDEXED,
                 routing_key,
                 source_scope,
                 source_path,
                 entity_labels,
                 entity_aliases,
                 content UNINDEXED
             );",
        )
        .expect("unindexed content table should replace current table");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("unindexed BM25 content should be detected")
    );
}

#[test]
fn schema_marker_rejects_contentless_bm25_tables() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    mark_schema_initialization_current(&connection).expect("marker should write");
    connection
        .execute_batch(
            "DROP TABLE graph_bm25;
             CREATE VIRTUAL TABLE graph_bm25 USING fts5(
                 document_id UNINDEXED,
                 document_kind UNINDEXED,
                 evidence_id UNINDEXED,
                 parent_evidence_id UNINDEXED,
                 modality UNINDEXED,
                 created_graph_version UNINDEXED,
                 routing_key,
                 source_scope,
                 source_path,
                 entity_labels,
                 entity_aliases,
                 content,
                 content=''
             );",
        )
        .expect("contentless BM25 table should replace current table");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("contentless BM25 table should be detected")
    );
}

#[test]
fn schema_marker_requires_file_content_schema() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    mark_schema_initialization_current(&connection).expect("marker should write");

    connection
        .execute("DROP TABLE file_content_entries", [])
        .expect("file content table should drop");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("missing file content table should be detected")
    );
}

#[test]
fn schema_marker_requires_workspace_mapping_ecosystem_unique_key() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    mark_schema_initialization_current(&connection).expect("marker should write");
    connection
        .execute("DROP TABLE code_workspace_package_mappings", [])
        .expect("workspace mappings should drop");
    connection
        .execute_batch(
            "
                CREATE TABLE code_workspace_package_mappings (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    set_id TEXT NOT NULL,
                    package_name TEXT NOT NULL,
                    ecosystem TEXT NOT NULL,
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    workspace_format TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    UNIQUE (set_id, package_name)
                );
                ",
        )
        .expect("legacy workspace mappings should create");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("legacy workspace mapping uniqueness should be detected")
    );
}

#[test]
fn schema_marker_rejects_previous_schema_version() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    super::initialize_schema_marker(&connection).expect("marker table should initialize");
    connection
        .execute(
            "
                INSERT INTO relay_storage_schema_state (key, version, updated_at_ms)
                VALUES ('sqlite_graph_store', ?1, 0)
                ",
            [super::SCHEMA_MARKER_VERSION - 1],
        )
        .expect("previous marker should insert");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("previous schema marker should be stale")
    );
}
