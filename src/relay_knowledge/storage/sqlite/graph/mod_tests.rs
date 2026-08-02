use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use super::{current_graph_version, inspect_graph};
use crate::storage::sqlite::{
    connection_runtime::maintenance::SqliteMaintenanceState, schema::initialization,
};

#[test]
fn fresh_schema_reports_zero_graph_state_and_empty_fact_counts() {
    let mut connection = Connection::open_in_memory().expect("connection should open");
    initialization::initialize_schema(&connection).expect("schema should initialize");
    let maintenance = Arc::new(Mutex::new(SqliteMaintenanceState::default()));

    assert_eq!(
        current_graph_version(&mut connection)
            .expect("graph version should load")
            .get(),
        0
    );
    let inspection =
        inspect_graph(&mut connection, None, &maintenance).expect("graph should inspect");

    assert_eq!(inspection.entity_count, 0);
    assert_eq!(inspection.evidence_count, 0);
    assert_eq!(inspection.mutation_count, 0);
    assert_eq!(inspection.code_file_count, 0);
}
