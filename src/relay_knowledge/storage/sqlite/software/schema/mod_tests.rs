use rusqlite::Connection;

use super::*;

#[test]
fn initialize_schema_marks_legacy_software_projection_status_stale() {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    connection
        .execute_batch(
            "
            CREATE TABLE software_global_status (
                source_scope TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                projected_graph_version INTEGER NOT NULL,
                stale INTEGER NOT NULL,
                component_count INTEGER NOT NULL,
                sdk_usage_count INTEGER NOT NULL,
                last_error TEXT
            );
            INSERT INTO software_global_status (
                source_scope, repository_id, projected_graph_version, stale,
                component_count, sdk_usage_count, last_error
            ) VALUES ('scope-legacy', 'repo', 7, 0, 2, 1, NULL);
            ",
        )
        .expect("legacy status should insert");

    initialize_schema(&connection).expect("software schema should initialize");
    let (stale, schema_version, last_error) = connection
        .query_row(
            "SELECT stale, projection_schema_version, last_error
             FROM software_global_status
             WHERE source_scope = 'scope-legacy'",
            [],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .expect("status should load");

    assert!(stale);
    assert_eq!(schema_version, SOFTWARE_PROJECTION_SCHEMA_VERSION);
    assert_eq!(
        last_error.as_deref(),
        Some("software global projection schema changed; refresh required")
    );
}

#[test]
fn initialize_schema_indexes_software_files_by_source_path() {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    initialize_schema(&connection).expect("software schema should initialize");

    let index_sql = connection
        .query_row(
            "
            SELECT sql
            FROM sqlite_master
            WHERE type = 'index'
              AND name = 'software_files_scope_path'
            ",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("source path index should exist");

    assert!(index_sql.contains("software_files(source_scope, path)"));
}
