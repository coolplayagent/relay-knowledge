use rusqlite::Connection;

use super::initialize_schema;

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
