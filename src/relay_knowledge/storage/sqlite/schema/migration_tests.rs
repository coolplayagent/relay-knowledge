use super::{
    CODE_WORKSPACE_PACKAGE_MAPPING_UNIQUE, prepare_existing_database_once,
    schema_compatibility_error_is_retryable, schema_compatibility_error_message_is_retryable,
    table_has_unique_columns,
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
