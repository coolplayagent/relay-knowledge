use rusqlite::Connection;

use super::initialize_schema;

#[test]
fn operational_schema_upgrades_legacy_provenance_and_is_idempotent() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE proposals (
                proposal_id TEXT PRIMARY KEY,
                source_scope TEXT NOT NULL,
                kind TEXT NOT NULL,
                state TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                origin TEXT NOT NULL,
                confidence_basis_points INTEGER NOT NULL,
                decided_by TEXT,
                decision_reason TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            ",
        )
        .expect("legacy proposal table should create");

    initialize_schema(&connection).expect("schema should upgrade");
    initialize_schema(&connection).expect("schema upgrade should be idempotent");

    for table in [
        "worker_tasks",
        "proposals",
        "proposal_conflicts",
        "audit_events",
        "service_operator_state",
    ] {
        let count = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get::<_, usize>(0),
            )
            .expect("table should query");
        assert_eq!(count, 1, "{table} should exist");
    }

    let has_provenance = {
        let mut statement = connection
            .prepare("PRAGMA table_info(proposals)")
            .expect("proposal columns should prepare");
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("proposal columns should query")
            .collect::<Result<Vec<_>, _>>()
            .expect("proposal columns should decode");
        columns.iter().any(|column| column == "provenance_json")
    };
    assert!(has_provenance);
}
