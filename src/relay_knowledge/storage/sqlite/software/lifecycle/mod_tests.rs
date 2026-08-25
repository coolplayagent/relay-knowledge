use rusqlite::Connection;

use super::{BoundedFacts, initialize_schema};
use crate::storage::StorageError;

#[test]
fn initialize_schema_delegates_to_each_lifecycle_projection_owner() {
    let connection = Connection::open_in_memory().expect("sqlite should open");

    initialize_schema(&connection).expect("lifecycle schema should initialize");

    let table_count = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'table'
              AND name IN (
                  'software_build_targets',
                  'software_iac_resources',
                  'software_design_elements'
              )
            ",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("table count should load");
    assert_eq!(table_count, 3);
}

#[test]
fn bounded_lifecycle_facts_dedupe_and_reject_cap_plus_one() {
    let mut facts = BoundedFacts::new(1, "test facts");
    facts.insert("same".to_owned(), 1).expect("first fact fits");
    facts
        .insert("same".to_owned(), 2)
        .expect("duplicate identity should not consume capacity");

    let error = facts
        .insert("second".to_owned(), 3)
        .expect_err("unique cap plus one should fail");

    assert!(matches!(error, StorageError::CapacityExceeded(message)
        if message.contains("test facts")));
    assert_eq!(facts.as_slice(), &[1]);
}
