use rusqlite::Connection;

use super::initialize_schema;

#[test]
fn initialization_creates_derived_scope_version_indexes() {
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
                'graph_vector_documents_scope_version'
              )
            ",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("derived retrieval indexes should be inspectable");

    assert_eq!(index_count, 2);
}
