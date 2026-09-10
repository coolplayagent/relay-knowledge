//! Durable page bytes and copies include nullable callable metadata.
use super::*;
use crate::storage::sqlite::code::snapshot::scope_tables::CODE_SCOPE_TABLES;
use rusqlite::Connection;

#[test]
fn durable_page_charges_utf8_callable_key_and_preserves_it_in_the_copy() {
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
    connection
        .execute_batch(
            "INSERT INTO code_repository_symbols (
        repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id, file_id,
        path, language_id, name, qualified_name, kind, signature,
        byte_start, byte_end, line_start, line_end
    ) VALUES ('repo', 'base', 'symbol', 'canonical', 'file', 'a.c', 'c', 'helper', 'helper',
              'function', 'int helper(int original)', 0, 24, 1, 1);",
        )
        .unwrap();
    let table = CODE_SCOPE_TABLES
        .iter()
        .find(|table| table.table == "code_repository_symbols")
        .unwrap();
    let current = progress::CloneProgress {
        source_scope: "target".into(),
        repository_id: "repo".into(),
        base_scope: "base".into(),
        task_id: "task".into(),
        delta_digest: "delta".into(),
        phase: "tables".into(),
        table_ordinal: 0,
        completed_page_ordinal: 0,
        cursor_key: None,
        cursor_tiebreaker: None,
        completed_table_ordinal: None,
        expected_table_rows: None,
        scanned_table_rows: 0,
        copied_table_rows: 0,
        scanned_total_rows: 0,
        copied_total_rows: 0,
        copied_total_bytes: 0,
        cloned_file_count: 0,
        cloned_symbol_count: 0,
        cloned_reference_count: 0,
        cloned_chunk_count: 0,
        cloned_diagnostic_count: 0,
        cloned_reference_group_count: 0,
        cloned_search_document_count: 0,
        base_manifest_reference_count: 0,
        base_manifest_group_count: 0,
        scanned_reference_occurrence_count: 0,
        scanned_reference_row_count: 0,
        scanned_reference_group_count: 0,
        scanned_reference_search_owner_count: 0,
        base_source_fact_row_upper_bound: 1,
        page_row_limit: 1,
        page_byte_limit: 16_384,
    };
    let transaction = connection.transaction().unwrap();
    let before = load_page(&transaction, table, &current, 1, 16_384).unwrap();
    let key = "é".repeat(crate::domain::MAX_CALLABLE_SIGNATURE_KEY_BYTES / 2);
    transaction
        .execute(
            "UPDATE code_repository_symbols SET callable_signature_key = ?1",
            [&key],
        )
        .unwrap();
    let page = load_page(&transaction, table, &current, 1, 16_384).unwrap();
    assert_eq!(page.bytes - before.bytes, key.len());
    assert!(load_page(&transaction, table, &current, 1, before.bytes).is_err());
    assert_eq!(
        copy_prefix(&transaction, table, &current, page.last.as_ref().unwrap()).unwrap(),
        1
    );
    let copied: Option<String> = transaction.query_row(
        "SELECT callable_signature_key FROM code_repository_symbols WHERE source_scope='target'", [], |row| row.get(0),
    ).unwrap();
    assert_eq!(copied, Some(key));
}
