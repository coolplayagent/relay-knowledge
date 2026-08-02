//! Direct diagnostics aggregation contract.

use rusqlite::Connection;

use super::diagnostics;
use crate::storage::sqlite::file_index::initialize_schema;

#[test]
fn empty_schema_reports_zeroed_diagnostics() {
    let connection = Connection::open_in_memory().expect("connection should open");
    initialize_schema(&connection).expect("schema should initialize");

    let diagnostics = diagnostics(&connection).expect("diagnostics should load");

    assert_eq!(diagnostics.root_count, 0);
    assert_eq!(diagnostics.indexed_file_count, 0);
    assert!(diagnostics.roots.is_empty());
}
