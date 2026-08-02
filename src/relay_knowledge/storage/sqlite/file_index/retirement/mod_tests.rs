//! Direct no-op and active-root preservation contracts for root retirement.

use rusqlite::Connection;

use super::mark_unconfigured_roots;
use crate::{storage::FileIndexRoot, storage::sqlite::file_index::initialize_schema};

#[test]
fn empty_and_active_root_sets_preserve_zeroed_diagnostics() {
    let mut connection = Connection::open_in_memory().expect("connection should open");
    initialize_schema(&connection).expect("schema should initialize");

    let diagnostics = mark_unconfigured_roots(
        &mut connection,
        vec![FileIndexRoot {
            scope_id: "local-files".to_owned(),
            root_id: "root-a".to_owned(),
            root_path: "/workspace".to_owned(),
        }],
        10,
    )
    .expect("retirement scan should succeed");

    assert_eq!(diagnostics.root_count, 0);
    assert_eq!(diagnostics.missing_file_count, 0);
}
