use super::{
    CODE_WORKSPACE_PACKAGE_MAPPING_UNIQUE, GRAPH_BM25_GLOBAL_LABEL_LOOKUP_INDEX,
    GRAPH_BM25_GLOBAL_LABEL_LOOKUP_INDEX_COLUMNS, GRAPH_BM25_GLOBAL_LABEL_STATE_INDEX,
    GRAPH_BM25_GLOBAL_LABEL_STATE_INDEX_COLUMNS, GRAPH_BM25_ROUTE_DOCUMENT_COLUMNS,
    GRAPH_GLOBAL_VERSION_INDEX_COLUMNS, GRAPH_SEMANTIC_GLOBAL_INDEX,
    bm25_route_table_is_compatible, index_has_columns, prepare_existing_database_once,
    schema_compatibility_error_is_retryable, schema_compatibility_error_message_is_retryable,
    table_has_exact_columns, table_has_unique_columns,
};
use crate::storage::StorageError;
use rusqlite::Connection;

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

#[test]
fn migration_rebuilds_legacy_workspace_package_mapping_uniqueness() {
    let connection = Connection::open_in_memory().expect("connection should open");
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
                INSERT INTO code_workspace_package_mappings (
                    set_id, package_name, ecosystem, repository_id, source_scope,
                    workspace_format, created_at_ms
                )
                VALUES ('set-1', 'core', 'npm', 'repo', 'scope', 'pnpm', 1);
                ",
        )
        .expect("legacy mapping table should create");

    prepare_existing_database_once(&connection).expect("migration should run");

    assert!(
        table_has_unique_columns(
            &connection,
            "code_workspace_package_mappings",
            CODE_WORKSPACE_PACKAGE_MAPPING_UNIQUE
        )
        .expect("unique key should inspect")
    );
    let row_count: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM code_workspace_package_mappings",
            [],
            |row| row.get(0),
        )
        .expect("row count should load");
    assert_eq!(row_count, 1);
}

#[test]
fn schema_version_change_invalidates_compatible_bm25_route_state() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE relay_storage_schema_state (
                key TEXT PRIMARY KEY,
                version INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            INSERT INTO relay_storage_schema_state VALUES ('sqlite_graph_store', 3, 0);
            CREATE TABLE graph_bm25_route_state (
                id INTEGER PRIMARY KEY,
                indexed_graph_version INTEGER NOT NULL,
                document_count INTEGER NOT NULL,
                state TEXT NOT NULL,
                algorithm_version TEXT NOT NULL,
                semantic_generation TEXT NOT NULL,
                vector_generation TEXT NOT NULL,
                rebuild_phase TEXT,
                rebuild_cursor TEXT,
                rebuild_semantic INTEGER,
                rebuild_vector INTEGER,
                rebuild_owner TEXT,
                rebuild_lease_expires_at_ms INTEGER
            );
            INSERT INTO graph_bm25_route_state
            VALUES (
                1, 7, 10, 'fresh', 'compatible', 'generation', 'generation',
                NULL, NULL, NULL, NULL, NULL, NULL
            );
            CREATE TABLE graph_bm25_route_documents (
                document_id TEXT PRIMARY KEY,
                fts_rowid INTEGER NOT NULL UNIQUE,
                document_kind TEXT NOT NULL,
                created_graph_version INTEGER NOT NULL,
                source_scope TEXT NOT NULL,
                source_path TEXT,
                label_gram_state TEXT NOT NULL,
                group_token TEXT NOT NULL,
                term_counts_json TEXT NOT NULL
            );
            CREATE TABLE graph_bm25_route_groups (
                source_scope TEXT NOT NULL,
                group_token TEXT NOT NULL,
                document_count INTEGER NOT NULL,
                PRIMARY KEY (source_scope, group_token)
            );
            CREATE TABLE graph_bm25_route_terms (
                term TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                group_token TEXT NOT NULL,
                collection_frequency INTEGER NOT NULL,
                PRIMARY KEY (term, source_scope, group_token)
            );
            CREATE TABLE graph_bm25_route_term_totals (
                term TEXT PRIMARY KEY,
                document_frequency INTEGER NOT NULL
            );
            ",
        )
        .expect("previous marker and compatible route state should create");

    prepare_existing_database_once(&connection).expect("migration should invalidate routing");

    let state = connection
        .query_row(
            "SELECT state FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("route state should remain readable");
    assert_eq!(state, "stale");
}

#[test]
fn bm25_hierarchy_suite_schema_migration_replaces_misordered_global_fallback_indexes() {
    let store = crate::storage::SqliteGraphStore::open_in_memory()
        .expect("complete storage schema should initialize");
    let connection = store.connection.lock().expect("connection should lock");
    super::super::marker::mark_schema_initialization_current(&connection)
        .expect("current schema marker should initialize");
    connection
        .execute_batch(
            "DROP INDEX graph_semantic_documents_version;
             CREATE INDEX graph_semantic_documents_version
             ON graph_semantic_documents(document_id, created_graph_version);
             DROP INDEX graph_bm25_route_documents_global_label_state;
             CREATE INDEX graph_bm25_route_documents_global_label_state
             ON graph_bm25_route_documents(
                 label_gram_state, source_scope, created_graph_version, document_id
             );
             DROP INDEX graph_bm25_label_grams_global_label_lookup;
             CREATE INDEX graph_bm25_label_grams_global_label_lookup
             ON graph_bm25_label_grams(
                 label_lower, source_scope, created_graph_version, document_id
             );",
        )
        .expect("misordered fallback indexes should replace current indexes");

    prepare_existing_database_once(&connection).expect("migration should drop stale indexes");
    crate::storage::sqlite::retrieval::initialize_schema(&connection)
        .expect("retrieval schema should recreate current indexes");

    for (index, columns) in [
        (
            GRAPH_SEMANTIC_GLOBAL_INDEX,
            GRAPH_GLOBAL_VERSION_INDEX_COLUMNS,
        ),
        (
            GRAPH_BM25_GLOBAL_LABEL_STATE_INDEX,
            GRAPH_BM25_GLOBAL_LABEL_STATE_INDEX_COLUMNS,
        ),
        (
            GRAPH_BM25_GLOBAL_LABEL_LOOKUP_INDEX,
            GRAPH_BM25_GLOBAL_LABEL_LOOKUP_INDEX_COLUMNS,
        ),
    ] {
        assert!(
            index_has_columns(&connection, index, columns)
                .expect("recreated global fallback index should inspect"),
            "migration did not recreate exact index {index}"
        );
    }
}

#[test]
fn bm25_hierarchy_suite_schema_migration_preserves_an_active_rebuild_lease() {
    let store = crate::storage::SqliteGraphStore::open_in_memory()
        .expect("complete storage schema should initialize");
    let connection = store.connection.lock().expect("connection should lock");
    create_canonical_bm25_shadow(&connection);
    mark_active_bm25_rebuild(&connection);

    prepare_existing_database_once(&connection).expect("migration should serialize safely");

    let state = connection
        .query_row(
            "SELECT state, rebuild_owner FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("active lease should remain readable");
    assert_eq!(state, ("building".to_owned(), "active-owner".to_owned()));
}

#[test]
fn bm25_hierarchy_suite_schema_migration_stales_unresumable_active_rebuilds() {
    for missing_dependency in ["shadow", "code", "companion"] {
        let store = crate::storage::SqliteGraphStore::open_in_memory()
            .expect("complete storage schema should initialize");
        let connection = store.connection.lock().expect("connection should lock");
        if missing_dependency != "shadow" {
            create_canonical_bm25_shadow(&connection);
        }
        match missing_dependency {
            "shadow" => {}
            "code" => {
                connection
                    .execute("DROP TABLE code_files", [])
                    .expect("code dependency should drop");
            }
            "companion" => {
                connection
                    .execute("DROP TABLE graph_semantic_documents", [])
                    .expect("companion dependency should drop");
            }
            _ => unreachable!("fixture dependency is exhaustive"),
        }
        mark_active_bm25_rebuild(&connection);

        prepare_existing_database_once(&connection)
            .expect("unresumable rebuild should migrate safely");

        let state = connection
            .query_row(
                "SELECT state FROM graph_bm25_route_state WHERE id = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("route state should remain readable");
        assert_eq!(state, "stale", "missing {missing_dependency} was resumable");
    }
}

fn create_canonical_bm25_shadow(connection: &Connection) {
    connection
        .execute_batch(
            "CREATE VIRTUAL TABLE graph_bm25_rebuild USING fts5(
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
                 content
             );",
        )
        .expect("canonical BM25 shadow should initialize");
}

fn mark_active_bm25_rebuild(connection: &Connection) {
    super::super::marker::mark_schema_initialization_current(connection)
        .expect("current schema marker should initialize");
    let marker = connection
        .query_row(
            "SELECT version FROM relay_storage_schema_state
             WHERE key = 'sqlite_graph_store'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("schema marker should load");
    assert_eq!(
        marker,
        super::super::marker::SCHEMA_MARKER_VERSION,
        "rebuild fixture must start at the current marker"
    );
    connection
        .execute(
            "UPDATE graph_bm25_route_state
             SET state = 'building', rebuild_owner = 'active-owner',
                 rebuild_lease_expires_at_ms =
                     CAST(strftime('%s', 'now') AS INTEGER) * 1000 + 60000
             WHERE id = 1",
            [],
        )
        .expect("active rebuild lease should initialize");
}

#[test]
fn obsolete_group_document_frequency_is_not_a_compatible_route_term_schema() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "CREATE TABLE graph_bm25_route_terms (
                 term TEXT NOT NULL,
                 source_scope TEXT NOT NULL,
                 group_token TEXT NOT NULL,
                 document_frequency INTEGER NOT NULL,
                 collection_frequency INTEGER NOT NULL
             );",
        )
        .expect("obsolete route term table should create");

    assert!(
        !table_has_exact_columns(
            &connection,
            "graph_bm25_route_terms",
            &[
                "term",
                "source_scope",
                "group_token",
                "collection_frequency"
            ]
        )
        .expect("route term columns should inspect")
    );
}

#[test]
fn route_aggregate_columns_without_the_primary_key_are_not_compatible() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "CREATE TABLE graph_bm25_route_groups (
                 source_scope TEXT NOT NULL,
                 group_token TEXT NOT NULL,
                 document_count INTEGER NOT NULL
             );",
        )
        .expect("constraint-free route group table should create");

    assert!(
        !bm25_route_table_is_compatible(
            &connection,
            "graph_bm25_route_groups",
            &["source_scope", "group_token", "document_count"]
        )
        .expect("route group constraints should inspect")
    );
}

#[test]
fn route_documents_with_extra_dead_columns_are_not_compatible() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "CREATE TABLE graph_bm25_route_documents (
                 document_id TEXT PRIMARY KEY,
                 fts_rowid INTEGER NOT NULL UNIQUE,
                 document_kind TEXT NOT NULL,
                 created_graph_version INTEGER NOT NULL,
                 source_scope TEXT NOT NULL,
                 source_path TEXT,
                 label_gram_state TEXT NOT NULL,
                 group_token TEXT NOT NULL,
                 term_counts_json TEXT NOT NULL,
                 legacy_generation INTEGER NOT NULL
             );",
        )
        .expect("legacy route documents should create");

    assert!(
        !bm25_route_table_is_compatible(
            &connection,
            "graph_bm25_route_documents",
            &[
                "document_id",
                "fts_rowid",
                "document_kind",
                "created_graph_version",
                "source_scope",
                "source_path",
                "label_gram_state",
                "group_token",
                "term_counts_json"
            ]
        )
        .expect("legacy route columns should inspect")
    );
}

#[test]
fn route_documents_without_created_graph_version_are_not_compatible() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "CREATE TABLE graph_bm25_route_documents (
                 document_id TEXT PRIMARY KEY,
                 fts_rowid INTEGER NOT NULL UNIQUE,
                 document_kind TEXT NOT NULL,
                 source_scope TEXT NOT NULL,
                 source_path TEXT,
                 label_gram_state TEXT NOT NULL,
                 group_token TEXT NOT NULL,
                 term_counts_json TEXT NOT NULL
             );",
        )
        .expect("unversioned route documents should create");

    assert!(
        !bm25_route_table_is_compatible(
            &connection,
            "graph_bm25_route_documents",
            GRAPH_BM25_ROUTE_DOCUMENT_COLUMNS,
        )
        .expect("unversioned route columns should inspect")
    );
}
