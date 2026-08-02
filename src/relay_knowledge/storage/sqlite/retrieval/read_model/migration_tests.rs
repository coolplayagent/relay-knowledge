use rusqlite::Connection;

use super::derived_documents_missing;

#[test]
fn missing_documents_follow_retrievable_source_count() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should initialize");
    super::super::schema::execute_retrieval_schema(&connection)
        .expect("retrieval schema should initialize");

    assert!(!derived_documents_missing(&connection).expect("empty state should inspect"));

    connection
        .execute("INSERT INTO evidence (status) VALUES ('accepted')", [])
        .expect("evidence should insert");
    assert!(derived_documents_missing(&connection).expect("missing documents should inspect"));
}
