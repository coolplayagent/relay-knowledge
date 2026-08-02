//! Direct content read-model schema contract.

use rusqlite::Connection;

use super::*;

#[test]
fn initializes_all_content_read_model_tables() {
    let connection = Connection::open_in_memory().expect("connection should open");
    initialize_schema(&connection).expect("content schema should initialize");

    for table in [
        "file_content_entries",
        "file_content_chunks",
        "file_content_search",
        "file_content_cursors",
    ] {
        let present = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = ?1)",
                [table],
                |row| row.get::<_, bool>(0),
            )
            .expect("schema catalog should be readable");
        assert!(present, "{table} should be initialized");
    }
}
