//! Warm databases must detect an absent nullable callable proof column.
use super::super::{mark_schema_initialization_current, schema_initialization_is_current};
use crate::storage::SqliteGraphStore;

#[test]
fn warm_marker_cannot_skip_additive_callable_key_migration() {
    let store = SqliteGraphStore::open_in_memory().unwrap();
    let connection = store.connection.lock().unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys=OFF;
        INSERT INTO code_repository_symbols (
            repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id, file_id,
            path, language_id, name, qualified_name, kind, signature,
            byte_start, byte_end, line_start, line_end
        ) VALUES ('r', 'scope', 'symbol', 'canonical', 'file', 'a.c', 'c', 'helper', 'helper',
                  'function', 'int helper(int original)', 0, 24, 1, 1);
        ALTER TABLE code_repository_symbols DROP COLUMN callable_signature_key;
        PRAGMA foreign_keys=ON;",
        )
        .unwrap();
    mark_schema_initialization_current(&connection).unwrap();
    assert!(!schema_initialization_is_current(&connection).unwrap());
    super::super::super::initialization::initialize_schema_for_open(&connection).unwrap();
    assert!(schema_initialization_is_current(&connection).unwrap());
    let preserved: (String, Option<String>) = connection.query_row(
        "SELECT signature, callable_signature_key FROM code_repository_symbols WHERE symbol_snapshot_id = 'symbol'",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(preserved, ("int helper(int original)".into(), None));
    super::super::super::initialization::initialize_schema_for_open(&connection).unwrap();
    let count: usize = connection
        .query_row("SELECT count(*) FROM code_repository_symbols", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}
