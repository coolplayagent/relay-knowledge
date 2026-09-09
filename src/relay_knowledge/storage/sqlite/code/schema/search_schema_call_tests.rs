use super::*;

#[test]
fn exact_call_selectors_use_scope_and_identity_index_keys() {
    let connection = Connection::open_in_memory().unwrap();
    super::super::repository_schema::initialize_repository_schema(&connection).unwrap();
    initialize_search_schema(&connection).unwrap();
    assert!(require_canonical_call_query_indexes(&connection).is_err());
    ensure_search_query_indexes(&connection).unwrap();
    require_canonical_call_query_indexes(&connection).unwrap();
    for direction in ["caller", "callee"] {
        let sql = format!(
            "EXPLAIN QUERY PLAN SELECT call_id FROM code_repository_calls WHERE source_scope=?1 AND {direction}_symbol_snapshot_id=?2 ORDER BY path,line_start LIMIT 201"
        );
        let plan = connection
            .prepare(&sql)
            .unwrap()
            .query_map(["scope", "symbol:exact"], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            plan.iter().any(|line| line.contains(&format!(
                "code_repository_calls_{direction}_snapshot_lookup"
            )) && line.contains(&format!(
                "source_scope=? AND {direction}_symbol_snapshot_id=?"
            ))),
            "{plan:?}"
        );
    }
    let plan = connection.prepare("EXPLAIN QUERY PLAN SELECT symbol_snapshot_id FROM code_repository_symbols WHERE source_scope=?1 AND canonical_symbol_id=?2 LIMIT 2").unwrap().query_map(["scope", "repo://exact"], |row| row.get::<_, String>(3)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert!(
        plan.iter().any(
            |line| line.contains("code_repository_symbols_canonical_lookup")
                && line.contains("source_scope=? AND canonical_symbol_id=?")
        ),
        "{plan:?}"
    );
}

#[test]
fn legacy_completed_prefix_appends_each_call_index_as_one_durable_unit() {
    let connection = Connection::open_in_memory().unwrap();
    super::super::repository_schema::initialize_repository_schema(&connection).unwrap();
    initialize_search_schema(&connection).unwrap();
    ensure_search_query_indexes(&connection).unwrap();
    for descriptor in &SEARCH_QUERY_INDEXES[17..20] {
        connection
            .execute(&format!("DROP INDEX {}", descriptor.name), [])
            .unwrap();
    }
    // Read-only startup validation accepts absent descriptors but never builds them.
    validate_existing_query_indexes(&connection).unwrap();
    assert!(require_canonical_call_query_indexes(&connection).is_err());
    let mut cursor = Some(16);
    for unit in 17..20 {
        assert_eq!(
            advance_search_query_indexes(&connection, cursor, false).unwrap(),
            SearchQueryIndexAdvance::Created {
                completed_unit: unit,
                plan_complete: unit == 19
            }
        );
        cursor = Some(unit);
    }
    assert_eq!(
        advance_search_query_indexes(&connection, cursor, false).unwrap(),
        SearchQueryIndexAdvance::Complete
    );
    require_canonical_call_query_indexes(&connection).unwrap();
    connection.execute_batch("DROP INDEX code_repository_calls_caller_snapshot_lookup; CREATE INDEX code_repository_calls_caller_snapshot_lookup ON code_repository_calls(source_scope,caller_name)").unwrap();
    assert!(require_canonical_call_query_indexes(&connection).is_err());
}
