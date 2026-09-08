use super::call_rows_sql;

#[test]
fn call_row_query_keeps_scope_order_and_bound_contracts() {
    let sql = call_rows_sql("AND c.callee_name = ?");

    assert!(sql.contains("WHERE c.source_scope = ?"));
    assert!(sql.contains("AND c.callee_name = ?"));
    assert!(sql.contains("ORDER BY f.is_generated ASC, c.path ASC, c.line_start ASC"));
    assert!(sql.contains("LIMIT ?"));
}

#[test]
fn canonical_resolution_counts_callable_definitions_after_declarations() {
    let connection = canonical_fixture();
    for (id, kind, signature) in [
        ("a", "function_declaration", "void helper();"),
        ("b", "function", "void helper();"),
        ("c", "function", "void helper() { leaf(); }"),
    ] {
        connection
            .execute(
                "INSERT INTO code_repository_symbols VALUES ('scope', 'canonical', ?1, ?2, ?3)",
                rusqlite::params![id, kind, signature],
            )
            .unwrap();
    }
    assert_eq!(
        super::canonical_callable_snapshot(&connection, "scope", "canonical")
            .unwrap()
            .as_deref(),
        Some("c")
    );
    assert!(
        super::canonical_callable_snapshot(&connection, "other", "canonical")
            .unwrap()
            .is_none()
    );
    connection.execute("INSERT INTO code_repository_symbols VALUES ('scope', 'canonical', 'd', 'function', 'void helper(int n) { other(); }')", []).unwrap();
    assert!(matches!(
        super::canonical_callable_snapshot(&connection, "scope", "canonical"),
        Err(crate::storage::StorageError::AmbiguousCodeSymbol(_))
    ));
    connection
        .execute(
            "DELETE FROM code_repository_symbols WHERE symbol_snapshot_id IN ('c', 'd')",
            [],
        )
        .unwrap();
    assert!(
        super::canonical_callable_snapshot(&connection, "scope", "canonical")
            .unwrap_err()
            .to_string()
            .contains("multiple callable declarations")
    );
    connection
        .execute(
            "DELETE FROM code_repository_symbols WHERE symbol_snapshot_id = 'b'",
            [],
        )
        .unwrap();
    assert_eq!(
        super::canonical_callable_snapshot(&connection, "scope", "canonical")
            .unwrap()
            .as_deref(),
        Some("a")
    );
    connection
        .execute("UPDATE code_repository_symbols SET kind = 'variable'", [])
        .unwrap();
    assert!(
        super::canonical_callable_snapshot(&connection, "scope", "canonical")
            .unwrap()
            .is_none()
    );
}

#[test]
fn canonical_resolution_reports_budget_exhaustion_instead_of_false_uniqueness() {
    let connection = canonical_fixture();
    for index in 0..super::MAX_CANONICAL_SYMBOL_CANDIDATES {
        connection.execute("INSERT INTO code_repository_symbols VALUES ('scope', 'canonical', ?1, 'function_declaration', 'void helper();')", [format!("decl-{index:04}")]).unwrap();
    }
    assert!(
        super::canonical_callable_snapshot(&connection, "scope", "canonical")
            .unwrap_err()
            .to_string()
            .contains("multiple callable declarations")
    );
    connection.execute("INSERT INTO code_repository_symbols VALUES ('scope', 'canonical', 'last', 'function', 'void helper() {}')", []).unwrap();
    let error = super::canonical_callable_snapshot(&connection, "scope", "canonical").unwrap_err();
    assert!(matches!(
        error,
        crate::storage::StorageError::AmbiguousCodeSymbol(_)
    ));
    assert!(error.to_string().contains("1024-candidate"));
    assert!(error.to_string().contains("symbol_snapshot_id"));
    connection.execute("UPDATE code_repository_symbols SET kind = 'function', signature = 'void helper() {}' WHERE symbol_snapshot_id = 'decl-0000'", []).unwrap();
    assert!(
        super::canonical_callable_snapshot(&connection, "scope", "canonical")
            .unwrap_err()
            .to_string()
            .contains("1024-candidate")
    );
}

fn canonical_fixture() -> rusqlite::Connection {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE code_repository_symbols (source_scope TEXT, canonical_symbol_id TEXT, symbol_snapshot_id TEXT, kind TEXT, signature TEXT);
        CREATE INDEX canonical_lookup ON code_repository_symbols (source_scope, canonical_symbol_id, symbol_snapshot_id);").unwrap();
    connection
}
