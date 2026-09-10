//! Callable proof metadata survives both scope and database copying.
use super::{
    clone_code_table, scope_tables::CODE_SCOPE_TABLES, snapshot_import::copy_attached_code_table,
};
use rusqlite::Connection;

#[test]
fn callable_key_survives_scope_clone_and_current_database_import() {
    let mut connection = Connection::open_in_memory().unwrap();
    crate::storage::sqlite::code::initialize_code_schema(&connection).unwrap();
    crate::storage::sqlite::code::lifecycle::status::upsert_repository(
        &mut connection,
        crate::domain::CodeRepositoryRegistration::new(
            "repo",
            "fixture",
            "/tmp/repo",
            vec![],
            vec![],
        )
        .unwrap(),
    )
    .unwrap();
    connection.execute_batch("INSERT INTO code_repository_symbols (
        repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id, file_id,
        path, language_id, name, qualified_name, kind, signature, callable_signature_key,
        byte_start, byte_end, line_start, line_end
    ) VALUES ('repo', 'base', 'symbol', 'canonical', 'file', 'a.c', 'c', 'helper', 'helper',
              'function', 'int helper(int original)', 'structured-proof', 0, 24, 1, 1);
    ATTACH DATABASE ':memory:' AS relay_import;
    CREATE TABLE relay_import.code_repository_symbols AS SELECT * FROM main.code_repository_symbols;").unwrap();
    let table = CODE_SCOPE_TABLES
        .iter()
        .find(|table| table.table == "code_repository_symbols")
        .unwrap();
    let transaction = connection.transaction().unwrap();
    clone_code_table(&transaction, table, "base", "cloned").unwrap();
    transaction
        .execute(
            "DELETE FROM code_repository_symbols WHERE source_scope = 'base'",
            [],
        )
        .unwrap();
    copy_attached_code_table(&transaction, table, "base").unwrap();
    for scope in ["base", "cloned"] {
        let record: (String, Option<String>) = transaction.query_row(
            "SELECT signature, callable_signature_key FROM code_repository_symbols WHERE source_scope = ?1", [scope],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(
            record,
            (
                "int helper(int original)".into(),
                Some("structured-proof".into())
            )
        );
    }
}
