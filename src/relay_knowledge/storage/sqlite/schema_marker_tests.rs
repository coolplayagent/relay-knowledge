use super::{mark_schema_initialization_current, schema_initialization_is_current};

#[test]
fn schema_marker_reports_current_only_after_successful_mark() {
    let store = super::super::SqliteGraphStore::open_in_memory().expect("store should open");
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
    let store = super::super::SqliteGraphStore::open_in_memory().expect("store should open");
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
fn schema_marker_requires_file_content_schema() {
    let store = super::super::SqliteGraphStore::open_in_memory().expect("store should open");
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
    let store = super::super::SqliteGraphStore::open_in_memory().expect("store should open");
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
fn schema_marker_rejects_previous_label_gram_migration_version() {
    let store = super::super::SqliteGraphStore::open_in_memory().expect("store should open");
    let connection = store.connection.lock().expect("connection should lock");
    super::initialize_schema_marker(&connection).expect("marker table should initialize");
    connection
        .execute(
            "
                INSERT INTO relay_storage_schema_state (key, version, updated_at_ms)
                VALUES ('sqlite_graph_store', 1, 0)
                ",
            [],
        )
        .expect("previous marker should insert");

    assert!(
        !schema_initialization_is_current(&connection)
            .expect("previous label gram migration marker should be stale")
    );
}
