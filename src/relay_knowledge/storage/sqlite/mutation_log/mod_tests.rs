//! Direct contracts for persisted mutation-log decoding and bounds.

use rusqlite::{Connection, params};

use super::*;
use crate::storage::sqlite::schema::initialization;

#[test]
fn mutation_log_rejects_zero_limit() {
    let mut connection = initialized_connection();

    assert!(matches!(
        read_mutations_after(&mut connection, GraphVersion::new(0), 0),
        Err(StorageError::InvalidInput(message))
            if message == "mutation log limit must be greater than zero"
    ));
}

#[test]
fn mutation_log_returns_ordered_versions_and_decoded_identity_lists() {
    let mut connection = initialized_connection();
    for (version, scope, evidence) in [(2, "scope-b", "ev-b"), (1, "scope-a", "ev-a")] {
        connection
            .execute(
                "INSERT INTO graph_mutations (
                    graph_version, evidence_count, entity_count, relation_count,
                    claim_count, event_count, affected_scopes_json,
                    affected_entity_ids_json, evidence_ids_json, source_hashes_json
                 ) VALUES (?1, 1, 1, 0, 0, 0, ?2, '[]', ?3, '[]')",
                params![
                    version,
                    format!("[\"{scope}\"]"),
                    format!("[\"{evidence}\"]")
                ],
            )
            .expect("mutation row should insert");
    }

    let entries = read_mutations_after(&mut connection, GraphVersion::new(0), 2)
        .expect("mutation log should decode");

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.graph_version.get())
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(entries[0].affected_scopes, ["scope-a"]);
    assert_eq!(entries[1].evidence_ids, ["ev-b"]);
}

fn initialized_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("connection should open");
    initialization::initialize_schema(&connection).expect("schema should initialize");
    connection
}
