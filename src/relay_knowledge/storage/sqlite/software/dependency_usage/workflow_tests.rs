use rusqlite::Connection;

use super::*;

#[test]
fn empty_component_set_short_circuits_before_import_reads() {
    let connection = Connection::open_in_memory().expect("database should open");

    let usages = derive_dependency_usages(&connection, "scope", GraphVersion::new(1), &[])
        .expect("empty matching index should not require import tables");

    assert!(usages.is_empty());
}
