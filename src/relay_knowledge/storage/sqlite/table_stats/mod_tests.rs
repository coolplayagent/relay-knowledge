//! Direct contracts for bounded SQLite table statistics.

use rusqlite::Connection;

use super::*;

#[test]
fn row_count_reports_empty_and_populated_tables() {
    let connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute("CREATE TABLE items (id INTEGER PRIMARY KEY)", [])
        .expect("table should create");

    assert_eq!(
        count_rows(&connection, "items").expect("empty row count should load"),
        0
    );
    connection
        .execute("INSERT INTO items (id) VALUES (1), (2), (3)", [])
        .expect("rows should insert");
    assert_eq!(
        count_rows(&connection, "items").expect("populated row count should load"),
        3
    );
}
