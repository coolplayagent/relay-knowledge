//! Trace guards for ordinary-reference pages that contain only call facts.

use super::paged_sql;
use super::paged_tests::{
    REFERENCE_SQL_TRACE, REFERENCE_SQL_TRACE_TEST, advance_and_commit, budget,
    capture_reference_sql, cursor, initialize_and_commit, insert_reference, pending,
    reference_database, stale_reference_count,
};

#[test]
fn code_index_persistence_performance_suite_reference_resolution_skips_call_payload_and_updates() {
    for scan in [paged_sql::SCAN_FIRST, paged_sql::SCAN_AFTER] {
        assert!(
            scan.contains("CASE WHEN kind = 'call' THEN 0"),
            "call rows must skip ordinary-owner sizing: {scan}"
        );
    }
    let _trace_test = REFERENCE_SQL_TRACE_TEST
        .lock()
        .expect("trace test should serialize");
    let mut connection = reference_database();
    connection
        .execute_batch(
            "CREATE TABLE reference_update_audit (reference_id TEXT NOT NULL);
             CREATE TRIGGER audit_reference_update
             AFTER UPDATE ON code_repository_references
             BEGIN
                 INSERT INTO reference_update_audit VALUES (NEW.reference_id);
             END;",
        )
        .expect("audit trigger should initialize");
    {
        let transaction = connection
            .transaction()
            .expect("fixture transaction should open");
        for index in 0..1_025 {
            insert_reference(
                &transaction,
                &format!("reference:{index:04}"),
                "src/calls.rs",
                &format!("callee_{index:04}"),
                "call",
            );
        }
        transaction.commit().expect("fixture should commit");
    }
    let resource_budget = budget(16 * 1024 * 1024, 1_027);
    initialize_and_commit(&mut connection, resource_budget, 1_025);

    REFERENCE_SQL_TRACE
        .lock()
        .expect("trace should lock")
        .clear();
    connection.trace(Some(capture_reference_sql));
    assert_eq!(
        advance_and_commit(&mut connection, cursor(0, 0, None), resource_budget, 1_025,),
        pending(1, 1_025, Some("reference:1024"))
    );
    connection.trace(None);

    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM reference_update_audit", [], |row| {
                row.get::<_, usize>(0)
            })
            .expect("audit rows should count"),
        0
    );
    assert_eq!(stale_reference_count(&connection), 1_025);
    let trace = REFERENCE_SQL_TRACE
        .lock()
        .expect("trace should lock")
        .clone();
    assert!(
        trace.iter().all(|statement| !statement
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("select reference_id, path, name")),
        "ordinary resolution must not fetch call payloads: {trace:?}"
    );
    assert_eq!(
        trace
            .iter()
            .filter(|statement| {
                let statement = statement.to_ascii_lowercase();
                statement.contains("select reference_id") && statement.contains("rowid =")
            })
            .count(),
        1,
        "one call-only page should fetch only its final durable cursor: {trace:?}"
    );
}
