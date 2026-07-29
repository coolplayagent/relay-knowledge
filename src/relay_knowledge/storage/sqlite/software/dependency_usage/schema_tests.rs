use rusqlite::Connection;

use super::*;

#[test]
fn schema_creation_marks_existing_projection_statuses_stale() {
    let connection = Connection::open_in_memory().expect("connection should open");
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
            ) VALUES ('scope', 'repo', 7, 0, 1, 1, NULL);
            ",
        )
        .expect("status should seed");

    initialize_schema(&connection).expect("dependency usage schema should initialize");

    let (stale, last_error) = connection
        .query_row(
            "SELECT stale, last_error FROM software_global_status WHERE source_scope = 'scope'",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .expect("status should load");
    assert_eq!(stale, 1);
    assert_eq!(
        last_error.as_deref(),
        Some("software dependency usage projection requires refresh")
    );
}

#[test]
fn existing_usage_schema_does_not_invalidate_projection_status() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE software_global_status (
                source_scope TEXT PRIMARY KEY,
                stale INTEGER NOT NULL,
                last_error TEXT
            );
            CREATE TABLE software_dependency_usages (
                usage_id TEXT PRIMARY KEY,
                source_scope TEXT NOT NULL,
                language_id TEXT NOT NULL,
                ecosystem TEXT NOT NULL,
                package_name TEXT NOT NULL
            );
            INSERT INTO software_global_status (source_scope, stale, last_error)
            VALUES ('scope', 0, NULL);
            ",
        )
        .expect("existing schema should seed");

    initialize_schema(&connection).expect("existing usage schema should initialize");

    let stale = connection
        .query_row(
            "SELECT stale FROM software_global_status WHERE source_scope = 'scope'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("status should load");
    assert_eq!(stale, 0);
}
