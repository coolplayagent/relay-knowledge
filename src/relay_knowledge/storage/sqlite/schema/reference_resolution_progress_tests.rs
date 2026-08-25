//! Reopen and fail-closed coverage for durable reference-resolution progress.

use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    mark_schema_initialization_current, reference_resolution_progress_schema_is_current,
    schema_initialization_is_current,
};

#[test]
fn marker_current_reopen_creates_missing_reference_resolution_progress_schema() {
    let database_path = database_path("reference-resolution-missing");
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("current store should open");
        let connection = store.connection.lock().expect("connection should lock");
        connection
            .execute(
                "DROP TABLE code_repository_reference_resolution_progress",
                [],
            )
            .expect("progress table should drop");
        mark_schema_initialization_current(&connection).expect("marker should be forced current");
        assert!(
            !schema_initialization_is_current(&connection)
                .expect("missing exact owner must invalidate the marker fast path")
        );
    }
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("missing progress table should repair on reopen");
        let connection = store.connection.lock().expect("connection should lock");
        assert!(
            reference_resolution_progress_schema_is_current(&connection)
                .expect("recreated progress owner should be exact")
        );
    }
    remove_database_files(&database_path);
}

#[test]
fn marker_current_reopen_repairs_empty_malformed_reference_resolution_progress_schema() {
    let database_path = database_path("reference-resolution-empty-malformed");
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("current store should open");
        let connection = store.connection.lock().expect("connection should lock");
        install_malformed_progress_table(&connection);
        mark_schema_initialization_current(&connection).expect("marker should be forced current");
    }
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("empty malformed progress should repair on reopen");
        let connection = store.connection.lock().expect("connection should lock");
        assert!(
            reference_resolution_progress_schema_is_current(&connection)
                .expect("repaired progress owner should be exact")
        );
    }
    remove_database_files(&database_path);
}

#[test]
fn marker_current_reopen_rejects_nonempty_malformed_reference_resolution_progress_schema() {
    let database_path = database_path("reference-resolution-nonempty-malformed");
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("current store should open");
        let connection = store.connection.lock().expect("connection should lock");
        install_malformed_progress_table(&connection);
        connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .expect("foreign keys should disable for malformed durable fixture");
        connection
            .execute(
                "INSERT INTO code_repository_reference_resolution_progress
                     (source_scope, protocol_version, stage, completed_page_ordinal,
                      cursor_reference_id, expected_reference_count,
                      resolved_reference_count, page_document_limit, page_byte_limit)
                 VALUES ('scope', 1, 'resolve', 1, 'reference:1', 1, 1, 1, 1024)",
                [],
            )
            .expect("durable malformed row should seed");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("foreign keys should restore");
        mark_schema_initialization_current(&connection).expect("marker should be forced current");
    }
    let error = crate::storage::SqliteGraphStore::open(&database_path)
        .expect_err("nonempty malformed progress must fail closed");
    assert!(
        error
            .to_string()
            .contains("non-empty reference-resolution progress")
    );
    remove_database_files(&database_path);
}

#[test]
fn marker_current_reopen_repairs_empty_reference_resolution_progress_with_extra_check() {
    let database_path = database_path("reference-resolution-empty-extra-check");
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("current store should open");
        let connection = store.connection.lock().expect("connection should lock");
        install_extra_check_progress_table(&connection);
        mark_schema_initialization_current(&connection).expect("marker should be forced current");
        assert!(
            !schema_initialization_is_current(&connection)
                .expect("an unexpected check must invalidate the marker fast path")
        );
    }
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("empty progress with an extra check should repair on reopen");
        let connection = store.connection.lock().expect("connection should lock");
        assert!(
            reference_resolution_progress_schema_is_current(&connection)
                .expect("repaired progress owner should be exact")
        );
    }
    remove_database_files(&database_path);
}

#[test]
fn marker_current_reopen_rejects_nonempty_reference_resolution_progress_with_extra_check() {
    let database_path = database_path("reference-resolution-nonempty-extra-check");
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("current store should open");
        let connection = store.connection.lock().expect("connection should lock");
        install_extra_check_progress_table(&connection);
        connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .expect("foreign keys should disable for malformed durable fixture");
        connection
            .execute(
                "INSERT INTO code_repository_reference_resolution_progress
                     (source_scope, protocol_version, stage, completed_page_ordinal,
                      cursor_reference_id, expected_reference_count,
                      resolved_reference_count, page_document_limit, page_byte_limit)
                 VALUES ('scope', 1, 'resolve', 0, NULL, 1, 0, 1, 1024)",
                [],
            )
            .expect("durable malformed row should seed");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("foreign keys should restore");
        mark_schema_initialization_current(&connection).expect("marker should be forced current");
    }
    let error = crate::storage::SqliteGraphStore::open(&database_path)
        .expect_err("nonempty progress with an extra check must fail closed");
    assert!(
        error
            .to_string()
            .contains("non-empty reference-resolution progress")
    );
    remove_database_files(&database_path);
}

fn install_malformed_progress_table(connection: &rusqlite::Connection) {
    connection
        .execute_batch(
            "DROP TABLE code_repository_reference_resolution_progress;
             CREATE TABLE code_repository_reference_resolution_progress (
                 source_scope TEXT NOT NULL PRIMARY KEY COLLATE NOCASE,
                 protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
                 stage TEXT NOT NULL CHECK (stage = 'resolve'),
                 completed_page_ordinal INTEGER NOT NULL CHECK (completed_page_ordinal >= 0),
                 cursor_reference_id TEXT NOT NULL,
                 expected_reference_count INTEGER NOT NULL CHECK (expected_reference_count >= 0),
                 resolved_reference_count INTEGER NOT NULL CHECK (resolved_reference_count >= 0),
                 page_document_limit INTEGER NOT NULL
                     CHECK (page_document_limit > 0 AND page_document_limit <= 32768),
                 page_byte_limit INTEGER NOT NULL
                     CHECK (page_byte_limit > 0 AND page_byte_limit <= 16777216),
                 CHECK (resolved_reference_count <= expected_reference_count),
                 FOREIGN KEY (source_scope)
                     REFERENCES code_repository_index_checkpoints(source_scope) ON DELETE CASCADE
             );",
        )
        .expect("malformed progress table should install");
}

fn install_extra_check_progress_table(connection: &rusqlite::Connection) {
    connection
        .execute_batch(
            "DROP TABLE code_repository_reference_resolution_progress;
             CREATE TABLE code_repository_reference_resolution_progress (
                 source_scope TEXT NOT NULL PRIMARY KEY,
                 protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
                 stage TEXT NOT NULL CHECK (stage = 'resolve'),
                 completed_page_ordinal INTEGER NOT NULL CHECK (completed_page_ordinal >= 0),
                 cursor_reference_id TEXT,
                 expected_reference_count INTEGER NOT NULL CHECK (expected_reference_count >= 0),
                 resolved_reference_count INTEGER NOT NULL CHECK (resolved_reference_count >= 0),
                 page_document_limit INTEGER NOT NULL
                     CHECK (page_document_limit > 0 AND page_document_limit <= 32768),
                 page_byte_limit INTEGER NOT NULL
                     CHECK (page_byte_limit > 0 AND page_byte_limit <= 16777216),
                 CHECK (resolved_reference_count <= expected_reference_count),
                 CHECK (completed_page_ordinal = 0),
                 FOREIGN KEY (source_scope)
                     REFERENCES code_repository_index_checkpoints(source_scope) ON DELETE CASCADE
             );",
        )
        .expect("extra-check progress table should install");
}

fn database_path(label: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "relay-knowledge-{label}-{}-{nonce}.sqlite",
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
