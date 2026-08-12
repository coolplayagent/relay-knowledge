use rusqlite::Connection;

use super::*;

#[test]
fn legacy_overlay_delete_guard_reads_only_cap_plus_one_rows() {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_cross_edges (set_id TEXT NOT NULL);
             WITH RECURSIVE rows(value) AS (
                 SELECT 0 UNION ALL SELECT value + 1 FROM rows WHERE value < 8192
             )
             INSERT INTO code_repository_cross_edges (set_id)
             SELECT 'set-over-cap' FROM rows;
             INSERT INTO code_repository_cross_edges (set_id)
             SELECT 'set-at-cap' FROM code_repository_cross_edges LIMIT 8192;",
        )
        .expect("legacy overlays should insert");

    ensure_overlay_delete_is_bounded(&connection, "set-at-cap")
        .expect("overlay at cap should remain maintainable");
    let error = ensure_overlay_delete_is_bounded(&connection, "set-over-cap")
        .expect_err("cap plus one must require explicit maintenance");
    assert!(matches!(error, StorageError::CapacityExceeded(_)));
}
