//! Direct schema contract for file metadata tables and FTS search.

use rusqlite::Connection;

use super::initialize_schema;

#[test]
fn initializes_metadata_root_entry_and_search_tables() {
    let connection = Connection::open_in_memory().expect("connection should open");

    initialize_schema(&connection).expect("schema should initialize");

    for table in [
        "file_index_roots",
        "file_index_entries",
        "file_index_search",
    ] {
        let count = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
                [table],
                |row| row.get::<_, usize>(0),
            )
            .expect("schema catalog should be readable");
        assert_eq!(count, 1, "missing schema object {table}");
    }
}
