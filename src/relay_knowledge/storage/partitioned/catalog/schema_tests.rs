use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;

use super::*;

#[test]
fn legacy_scope_publication_columns_migrate_idempotently_and_default_active() {
    let path = unique_database_path("legacy-scope-publication");
    let connection = Connection::open(&path).expect("legacy catalog should open");
    connection
        .execute_batch(
            "
            CREATE TABLE storage_repository_shards (
                repository_id TEXT PRIMARY KEY,
                db_path TEXT NOT NULL,
                state TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            CREATE TABLE storage_repository_shard_scopes (
                source_scope TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            INSERT INTO storage_repository_shards VALUES ('repo', 'repo.db', 'active', 1, 1);
            INSERT INTO storage_repository_shard_scopes VALUES ('scope', 'repo', 1);
            ",
        )
        .expect("legacy catalog should seed");
    drop(connection);

    initialize_catalog_schema(&path).expect("legacy schema should migrate");
    initialize_catalog_schema(&path).expect("migration retry should be idempotent");

    let connection = Connection::open(&path).expect("migrated catalog should reopen");
    let columns = connection
        .prepare("PRAGMA table_info(storage_repository_shard_scopes)")
        .expect("scope columns should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("scope columns should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("scope columns should decode");
    let publication = connection
        .query_row(
            "SELECT state, staged_task_id
             FROM storage_repository_shard_scopes
             WHERE source_scope = 'scope'",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .expect("legacy scope publication fields should query");

    assert!(columns.iter().any(|column| column == "state"));
    assert!(columns.iter().any(|column| column == "staged_task_id"));
    assert_eq!(publication, ("active".to_owned(), None));
    drop(connection);
    remove_database(&path);
}

#[test]
fn current_catalog_schema_revalidation_does_not_request_a_write_lock() {
    let path = unique_database_path("current-schema-read-fast-path");
    initialize_catalog_schema(&path).expect("catalog schema should initialize");
    let writer = Connection::open(&path).expect("writer should open");
    writer
        .execute_batch("BEGIN IMMEDIATE TRANSACTION")
        .expect("writer should hold the reserved lock");

    initialize_catalog_schema(&path)
        .expect("current catalog validation should remain read-only under a writer");

    writer
        .execute_batch("ROLLBACK")
        .expect("writer transaction should roll back");
    drop(writer);
    remove_database(&path);
}

fn unique_database_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "relay-knowledge-catalog-schema-{label}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn remove_database(path: &Path) {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        let _ = std::fs::remove_file(candidate);
    }
}
