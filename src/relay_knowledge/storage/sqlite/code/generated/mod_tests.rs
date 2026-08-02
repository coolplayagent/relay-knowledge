//! Direct contracts for generated-path backfill and scope invalidation.

use rusqlite::{Connection, params};

use super::*;

#[test]
fn generated_backfill_updates_only_matching_paths_in_requested_scope() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                is_generated INTEGER NOT NULL
             );",
        )
        .expect("file table should create");
    for (scope, path) in [
        ("scope-a", "dist/app.js"),
        ("scope-a", "src/lib.rs"),
        ("scope-b", "build/app.js"),
    ] {
        connection
            .execute(
                "INSERT INTO code_repository_files (source_scope, path, is_generated)
                 VALUES (?1, ?2, 0)",
                params![scope, path],
            )
            .expect("file row should insert");
    }

    backfill_scope_path_generated_flags(&connection, "scope-a")
        .expect("generated paths should backfill");

    let mut statement = connection
        .prepare("SELECT path, is_generated FROM code_repository_files ORDER BY path")
        .expect("query should prepare");
    let flags = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .expect("rows should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows should decode");
    assert_eq!(
        flags,
        [
            ("build/app.js".to_owned(), 0),
            ("dist/app.js".to_owned(), 1),
            ("src/lib.rs".to_owned(), 0),
        ]
    );
}

#[test]
fn generated_detection_invalidation_marks_scope_and_active_repository_stale() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_scopes (
                source_scope TEXT PRIMARY KEY,
                stale INTEGER NOT NULL
             );
             CREATE TABLE code_repositories (
                last_indexed_scope_id TEXT,
                stale INTEGER NOT NULL
             );
             INSERT INTO code_repository_scopes VALUES ('scope-a', 0), ('scope-b', 0);
             INSERT INTO code_repositories VALUES ('scope-a', 0), ('scope-b', 0);",
        )
        .expect("scope tables should create");

    mark_scope_generated_detection_stale(&connection, "scope-a").expect("scope should invalidate");

    let scope_stale: i64 = connection
        .query_row(
            "SELECT stale FROM code_repository_scopes WHERE source_scope = 'scope-a'",
            [],
            |row| row.get(0),
        )
        .expect("scope state should load");
    let other_stale: i64 = connection
        .query_row(
            "SELECT stale FROM code_repositories WHERE last_indexed_scope_id = 'scope-b'",
            [],
            |row| row.get(0),
        )
        .expect("other repository state should load");
    assert_eq!(scope_stale, 1);
    assert_eq!(other_stale, 0);
}
