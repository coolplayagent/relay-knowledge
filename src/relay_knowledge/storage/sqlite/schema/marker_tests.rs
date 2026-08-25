use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use super::{
    SCHEMA_MARKER_VERSION, SEARCH_OWNER_V2_MIGRATION, index_has_columns,
    mark_schema_initialization_current, schema_initialization_is_current, table_column_is_not_null,
    table_has_unique_columns,
};

#[test]
fn marker_current_reopen_creates_missing_reference_search_progress_schema() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    let database_path = std::env::temp_dir().join(format!(
        "relay-knowledge-reference-progress-marker-{}-{nonce}.sqlite",
        std::process::id()
    ));
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("legacy file store should open");
        let connection = store.connection.lock().expect("connection should lock");
        connection
            .execute(
                "INSERT INTO entities (id, label, created_graph_version)
                 VALUES ('preserved', 'preserved', 0)",
                [],
            )
            .expect("preserved row should insert");
        connection
            .execute("DROP TABLE code_repository_reference_search_progress", [])
            .expect("legacy schema should omit progress");
        mark_schema_initialization_current(&connection)
            .expect("legacy marker should remain current");
        assert!(
            !schema_initialization_is_current(&connection)
                .expect("missing progress table should invalidate marker fast path")
        );
    }

    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("marker-current legacy store should migrate on reopen");
        let connection = store.connection.lock().expect("connection should lock");
        assert!(
            schema_initialization_is_current(&connection)
                .expect("reopened progress schema should be current")
        );
        assert!(
            super::reference_search_progress_schema_is_current(&connection)
                .expect("progress shape should validate")
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM entities WHERE id = 'preserved'",
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .expect("preserved row should count"),
            1
        );
    }
    for path in [
        database_path.clone(),
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ] {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn code_index_task_reopen_repairs_empty_malformed_reference_search_owner_schema() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    let database_path = std::env::temp_dir().join(format!(
        "relay-knowledge-reference-owner-empty-repair-{}-{nonce}.sqlite",
        std::process::id()
    ));
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("current file store should open");
        let connection = store.connection.lock().expect("connection should lock");
        connection
            .execute_batch(
                "DROP INDEX code_repository_reference_search_groups_path;
                 DROP TABLE code_repository_reference_search_groups;
                 DROP TABLE code_repository_reference_search_manifests;
                 CREATE TABLE code_repository_reference_search_groups (
                     source_scope TEXT, group_id TEXT, path TEXT
                 );
                 CREATE TABLE code_repository_reference_search_manifests (
                     source_scope TEXT, reference_count INTEGER
                 );
                 CREATE INDEX code_repository_reference_search_groups_path
                     ON code_repository_reference_search_groups(path, source_scope);",
            )
            .expect("empty malformed owner schema should seed");
        mark_schema_initialization_current(&connection).expect("marker should be forced current");
    }
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("empty malformed owner schema should repair on reopen");
        let connection = store.connection.lock().expect("connection should lock");
        assert!(
            super::reference_search_group_schema_is_current(&connection)
                .expect("repaired owner schema should validate")
        );
    }
    remove_database_files(&database_path);
}

#[test]
fn code_index_task_reopen_rejects_nonempty_malformed_reference_search_owner_schema() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    let database_path = std::env::temp_dir().join(format!(
        "relay-knowledge-reference-owner-nonempty-reject-{}-{nonce}.sqlite",
        std::process::id()
    ));
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("current file store should open");
        let connection = store.connection.lock().expect("connection should lock");
        connection
            .execute_batch(
                "DROP INDEX code_repository_reference_search_groups_path;
                 DROP TABLE code_repository_reference_search_groups;
                 DROP TABLE code_repository_reference_search_manifests;
                 CREATE TABLE code_repository_reference_search_groups (
                     source_scope TEXT, group_id TEXT, path TEXT
                 );
                 INSERT INTO code_repository_reference_search_groups
                     VALUES ('scope', 'group', 'src/lib.rs');",
            )
            .expect("nonempty malformed owner schema should seed");
        mark_schema_initialization_current(&connection).expect("marker should be forced current");
    }
    let error = crate::storage::SqliteGraphStore::open(&database_path)
        .expect_err("nonempty malformed owner schema must fail closed");
    assert!(error.to_string().contains("non-empty reference-search"));
    remove_database_files(&database_path);
}

#[test]
fn marker_current_reopen_repairs_empty_noncanonical_grouped_progress_schema() {
    let database_path = grouped_progress_database_path("empty-repair");
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("current file store should open");
        let connection = store.connection.lock().expect("connection should lock");
        install_noncanonical_grouped_progress(&connection);
        mark_schema_initialization_current(&connection).expect("marker should be forced current");
        assert!(
            !schema_initialization_is_current(&connection)
                .expect("hidden/default/index/constraint drift must invalidate the marker")
        );
    }
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("empty noncanonical grouped progress should repair on reopen");
        let connection = store.connection.lock().expect("connection should lock");
        assert!(
            super::reference_search_progress_schema_is_current(&connection)
                .expect("repaired grouped progress should be exact")
        );
    }
    remove_database_files(&database_path);
}

#[test]
fn marker_current_reopen_rejects_nonempty_noncanonical_grouped_progress_schema() {
    let database_path = grouped_progress_database_path("nonempty-reject");
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("current file store should open");
        let connection = store.connection.lock().expect("connection should lock");
        install_noncanonical_grouped_progress(&connection);
        connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .expect("foreign keys should disable for the malformed durable fixture");
        connection
            .execute(
                "INSERT INTO code_repository_reference_search_progress (
                     source_scope, projection_version, stage, completed_page_ordinal,
                     cleanup_cursor_rowid, cleanup_cursor_record_id,
                     discovery_cursor_reference_id, build_cursor_group_id,
                     expected_reference_count, cleanup_total_count,
                     discovered_reference_count, discovered_group_count,
                     build_total_count, cleaned_count, built_count,
                     page_document_limit, page_byte_limit
                 ) VALUES (
                     'scope', 2, 'cleanup', 0, 1, 'cleanup', 'discovery', 'build',
                     1, 0, 0, 0, 0, 0, 0, 1, 1024
                 )",
                [],
            )
            .expect("noncanonical durable row should seed");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("foreign keys should restore");
        mark_schema_initialization_current(&connection).expect("marker should be forced current");
    }
    let error = crate::storage::SqliteGraphStore::open(&database_path)
        .expect_err("nonempty noncanonical grouped progress must fail closed");
    assert!(
        error
            .to_string()
            .contains("nonempty reference-search progress"),
        "unexpected reopen error: {error}"
    );
    remove_database_files(&database_path);
}

fn install_noncanonical_grouped_progress(connection: &Connection) {
    connection
        .execute_batch(
            "DROP TABLE code_repository_reference_search_progress;
             CREATE TABLE code_repository_reference_search_progress (
                 source_scope TEXT NOT NULL PRIMARY KEY COLLATE NOCASE,
                 projection_version INTEGER NOT NULL DEFAULT(NULL)
                     CHECK (projection_version > 0),
                 stage TEXT NOT NULL CHECK (stage IN ('cleanup', 'discover', 'build')),
                 completed_page_ordinal INTEGER NOT NULL
                     CHECK (completed_page_ordinal >= 0),
                 cleanup_cursor_rowid INTEGER NOT NULL,
                 cleanup_cursor_record_id TEXT NOT NULL,
                 discovery_cursor_reference_id TEXT NOT NULL,
                 build_cursor_group_id TEXT NOT NULL,
                 expected_reference_count INTEGER NOT NULL
                     CHECK (expected_reference_count >= 0),
                 cleanup_total_count INTEGER NOT NULL CHECK (cleanup_total_count >= 0),
                 discovered_reference_count INTEGER NOT NULL
                     CHECK (discovered_reference_count >= 0),
                 discovered_group_count INTEGER NOT NULL
                     CHECK (discovered_group_count >= 0),
                 build_total_count INTEGER NOT NULL CHECK (build_total_count >= 0),
                 cleaned_count INTEGER NOT NULL CHECK (cleaned_count >= 0),
                 built_count INTEGER NOT NULL CHECK (built_count >= 0),
                 page_document_limit INTEGER NOT NULL CHECK (page_document_limit > 0),
                 page_byte_limit INTEGER NOT NULL CHECK (page_byte_limit > 0),
                 page_guard INTEGER GENERATED ALWAYS AS (
                     CASE WHEN completed_page_ordinal = 0 THEN 0 END
                 ) STORED NOT NULL,
                 CHECK (completed_page_ordinal = 0),
                 UNIQUE(discovery_cursor_reference_id),
                 FOREIGN KEY (source_scope) REFERENCES code_repository_index_checkpoints(source_scope)
                     ON DELETE CASCADE
             );
             CREATE INDEX grouped_progress_extra_index
                 ON code_repository_reference_search_progress(stage);",
        )
        .expect("noncanonical grouped progress should install");
}

fn grouped_progress_database_path(label: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "relay-knowledge-grouped-progress-{label}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn remove_database_files(database_path: &std::path::Path) {
    for path in [
        database_path.to_path_buf(),
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ] {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn code_index_task_reopen_preserves_legacy_reference_search_progress_for_leased_restart() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    let database_path = std::env::temp_dir().join(format!(
        "relay-knowledge-reference-progress-v1-{}-{nonce}.sqlite",
        std::process::id()
    ));
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("current file store should open");
        let connection = store.connection.lock().expect("connection should lock");
        connection
            .execute_batch(
                "INSERT INTO code_repositories (
                     repository_id, alias, root_path, path_filters_json, language_filters_json,
                     state, indexed_file_count, symbol_count, reference_count, chunk_count, stale
                 ) VALUES ('repo-v1', 'repo-v1', '/tmp/repo-v1', '[]', '[]',
                           'indexing', 0, 0, 3, 0, 1);
                 INSERT INTO code_repository_index_checkpoints (
                     source_scope, repository_id, state, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, total_path_count,
                     parsed_file_count, committed_file_count, committed_symbol_count,
                     committed_reference_count, committed_chunk_count, batch_count,
                     resource_budget_json, updated_at_ms
                 ) VALUES (
                     'scope-v1', 'repo-v1',
                     'finalizing:rebuild_reference_search:v1:build:7', 'commit', 'tree',
                     '[]', '[]', 0, 0, 0, 0, 3, 0, 0,
                     '{\"max_files_per_batch\":1,\"max_bytes_per_batch\":1024,\"max_rows_per_batch\":16}',
                     1
                 );
                 DROP TABLE code_repository_reference_search_progress;
                 CREATE TABLE code_repository_reference_search_progress (
                     source_scope TEXT PRIMARY KEY,
                     stage TEXT NOT NULL CHECK (stage IN ('cleanup', 'build')),
                     completed_page_ordinal INTEGER NOT NULL CHECK (completed_page_ordinal >= 0),
                     cleanup_cursor_rowid INTEGER,
                     build_cursor_reference_id TEXT,
                     cleanup_total_count INTEGER NOT NULL CHECK (cleanup_total_count >= 0),
                     build_total_count INTEGER NOT NULL CHECK (build_total_count >= 0),
                     cleaned_count INTEGER NOT NULL CHECK (cleaned_count >= 0),
                     built_count INTEGER NOT NULL CHECK (built_count >= 0),
                     page_document_limit INTEGER NOT NULL CHECK (page_document_limit > 0),
                     page_byte_limit INTEGER NOT NULL CHECK (page_byte_limit > 0)
                 );
                 INSERT INTO code_repository_reference_search_progress VALUES (
                     'scope-v1', 'build', 7, NULL, 'reference:3', 0, 3, 0, 3, 4, 1024
                 );",
            )
            .expect("legacy v1 progress should seed");
        mark_schema_initialization_current(&connection).expect("marker should be forced current");
    }
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("legacy progress store should reopen and migrate");
        let connection = store.connection.lock().expect("connection should lock");
        let migrated = connection
            .query_row(
                "SELECT projection_version, stage, completed_page_ordinal,
                        build_cursor_group_id, expected_reference_count, built_count
                 FROM code_repository_reference_search_progress WHERE source_scope = 'scope-v1'",
                [],
                |row| {
                    Ok((
                        row.get::<_, usize>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, usize>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, usize>(4)?,
                        row.get::<_, usize>(5)?,
                    ))
                },
            )
            .expect("migrated progress should load");
        assert_eq!(
            migrated,
            (
                1,
                "build".to_owned(),
                7,
                Some("reference:3".to_owned()),
                3,
                3
            )
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT state FROM code_repository_index_checkpoints
                     WHERE source_scope = 'scope-v1'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .expect("legacy checkpoint should remain exact"),
            "finalizing:rebuild_reference_search:v1:build:7"
        );
    }
    for path in [
        database_path.clone(),
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ] {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn marker_current_reopen_adds_missing_scope_gc_search_cursor() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    let database_path = std::env::temp_dir().join(format!(
        "relay-knowledge-retention-cursor-marker-{}-{nonce}.sqlite",
        std::process::id()
    ));
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("legacy file store should open");
        let connection = store.connection.lock().expect("connection should lock");
        connection
            .execute(
                "ALTER TABLE code_repository_scope_gc_jobs
                 DROP COLUMN search_rowid_cursor",
                [],
            )
            .expect("legacy schema should omit search cursor");
        mark_schema_initialization_current(&connection)
            .expect("legacy marker should remain current");
        assert!(
            !schema_initialization_is_current(&connection)
                .expect("missing search cursor should invalidate marker fast path")
        );
    }

    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("marker-current legacy store should migrate on reopen");
        let connection = store.connection.lock().expect("connection should lock");
        assert!(
            schema_initialization_is_current(&connection)
                .expect("reopened retention schema should be current")
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*)
                     FROM pragma_table_info('code_repository_scope_gc_jobs')
                     WHERE name = 'search_rowid_cursor' AND \"notnull\" = 0",
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .expect("search cursor should inspect"),
            1
        );
    }
    for path in [
        database_path.clone(),
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ] {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn marker_current_reopen_retries_search_owner_after_cursor_upgrade_failure() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    let database_path = std::env::temp_dir().join(format!(
        "relay-knowledge-search-owner-retry-marker-{}-{nonce}.sqlite",
        std::process::id()
    ));
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("legacy file store should open");
        let connection = store.connection.lock().expect("connection should lock");
        connection
            .execute_batch(&format!(
                "DELETE FROM code_repository_schema_migrations
                 WHERE name = '{SEARCH_OWNER_V2_MIGRATION}';
                 INSERT INTO code_repositories (
                     repository_id, alias, root_path, path_filters_json, language_filters_json,
                     last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                     indexed_file_count, symbol_count, reference_count, chunk_count,
                     stale, degraded_reason
                 ) VALUES (
                     'legacy-repo', 'legacy', '/tmp/legacy', '[]', '[]', 'legacy-scope',
                     'legacy-commit', 'legacy-tree', 'fresh', 1, 1, 0, 0, 0, NULL
                 );
                 INSERT INTO code_repository_scopes (
                     source_scope, repository_id, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, indexed_file_count,
                     symbol_count, reference_count, chunk_count, stale, degraded_reason
                 ) VALUES (
                     'legacy-scope', 'legacy-repo', 'legacy-commit', 'legacy-tree',
                     '[]', '[]', 1, 1, 0, 0, 0, NULL
                 );
                 ALTER TABLE code_repository_scope_gc_jobs
                 DROP COLUMN search_rowid_cursor;
                 CREATE TRIGGER fail_search_owner_upgrade
                 BEFORE UPDATE OF stale ON code_repositories
                 WHEN NEW.repository_id = 'legacy-repo'
                 BEGIN
                     SELECT RAISE(ABORT, 'injected search-owner upgrade failure');
                 END;"
            ))
            .expect("marker-current legacy state should initialize");
        mark_schema_initialization_current(&connection)
            .expect("legacy global marker should remain current");
        assert!(
            !schema_initialization_is_current(&connection)
                .expect("missing cursor and capability should invalidate fast path")
        );
    }

    let error = crate::storage::SqliteGraphStore::open(&database_path)
        .expect_err("injected stale migration failure should abort reopen");
    assert!(error.to_string().contains("search-owner upgrade failure"));
    {
        let connection = Connection::open(&database_path).expect("failed database should reopen");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*)
                     FROM pragma_table_info('code_repository_scope_gc_jobs')
                     WHERE name = 'search_rowid_cursor'",
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .expect("persisted cursor column should inspect"),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT version FROM relay_storage_schema_state
                     WHERE key = 'sqlite_graph_store'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("legacy global marker should load"),
            SCHEMA_MARKER_VERSION
        );
        assert!(
            !connection
                .query_row(
                    "SELECT stale FROM code_repository_scopes
                     WHERE source_scope = 'legacy-scope'",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .expect("rolled-back scope state should load")
        );
        assert!(
            !schema_initialization_is_current(&connection)
                .expect("missing capability marker must keep fast path closed")
        );
        connection
            .execute("DROP TRIGGER fail_search_owner_upgrade", [])
            .expect("failure trigger should drop");
    }

    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("second reopen should retry the capability migration");
        let connection = store.connection.lock().expect("connection should lock");
        assert!(
            schema_initialization_is_current(&connection)
                .expect("retried schema should become current")
        );
        assert!(
            connection
                .query_row(
                    "SELECT stale FROM code_repository_scopes
                     WHERE source_scope = 'legacy-scope'",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .expect("migrated scope state should load")
        );
        assert!(
            connection
                .query_row(
                    "SELECT EXISTS (
                         SELECT 1 FROM code_repository_schema_migrations WHERE name = ?1
                     )",
                    [SEARCH_OWNER_V2_MIGRATION],
                    |row| row.get::<_, bool>(0),
                )
                .expect("search-owner marker should load")
        );
    }
    for path in [
        database_path.clone(),
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ] {
        let _ = std::fs::remove_file(path);
    }
}

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
