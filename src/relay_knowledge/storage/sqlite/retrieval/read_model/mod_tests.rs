use rusqlite::Connection;

use super::initialize_schema;

#[test]
fn initialization_creates_scoped_and_global_derived_version_indexes() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist for retrieval migration checks");

    initialize_schema(&connection).expect("schema should initialize");

    let index_count = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'index'
              AND name IN (
                'graph_semantic_documents_scope_version',
                'graph_semantic_documents_version',
                'graph_vector_documents_scope_version'
              )
            ",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("derived retrieval indexes should be inspectable");

    assert_eq!(index_count, 3);
}

#[test]
fn reopening_a_current_generation_removes_a_retired_bm25_table() {
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-retired-bm25-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should follow the Unix epoch")
            .as_nanos()
    ));
    {
        let connection = Connection::open(&path).expect("database should open");
        connection
            .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
            .expect("evidence table should exist for retrieval migration checks");
        initialize_schema(&connection).expect("current generation should initialize");
        connection
            .execute_batch("CREATE VIRTUAL TABLE graph_bm25_retired USING fts5(content);")
            .expect("crash-window retired table should remain on disk");
    }

    let connection = Connection::open(&path).expect("database should reopen");
    initialize_schema(&connection).expect("current generation should clean up on reopen");
    let retired_exists = connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'graph_bm25_retired'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("retired table state should be inspectable");
    let active_exists = connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'graph_bm25'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("active table state should be inspectable");

    assert!(!retired_exists);
    assert!(active_exists);
    drop(connection);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}
