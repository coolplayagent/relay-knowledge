use super::*;

#[test]
fn populated_feature_key_index_is_deferred_until_its_durable_unit() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    ensure_search_query_indexes(&connection).unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys=OFF; DROP INDEX code_repository_feature_flags_source_key; DROP INDEX code_repository_java_namespace_completeness_lookup; DROP INDEX code_repository_java_type_lookup;",
        )
        .unwrap();
    let tx = connection.transaction().unwrap();
    let record = crate::storage::sqlite::code::feature_flags::test_support::record(
        "old",
        "feature_x",
        "config_key",
    );
    crate::storage::sqlite::code::feature_flags::insert_records(&tx, &[record]).unwrap();
    tx.commit().unwrap();
    super::super::initialize_code_schema(&connection).unwrap();
    validate_existing_query_indexes(&connection).unwrap();
    let error = require_feature_flag_query_index(&connection).unwrap_err();
    assert!(error.to_string().contains("run repo index"));
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM code_repository_feature_flags",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        advance_search_query_indexes(&connection, Some(19), false).unwrap(),
        SearchQueryIndexAdvance::Created {
            completed_unit: 20,
            plan_complete: false
        }
    );
    for unit in [21, 22] {
        assert_eq!(
            advance_search_query_indexes(&connection, Some(unit - 1), false).unwrap(),
            SearchQueryIndexAdvance::Created {
                completed_unit: unit,
                plan_complete: unit == 22
            }
        );
    }
    require_feature_flag_query_index(&connection).unwrap();
    assert_eq!(
        advance_search_query_indexes(&connection, Some(22), false).unwrap(),
        SearchQueryIndexAdvance::Complete
    );
    connection.execute_batch("DROP INDEX code_repository_feature_flags_source_key; CREATE INDEX code_repository_feature_flags_source_key ON code_repository_feature_flags(source_scope,source_key)").unwrap();
    assert!(require_feature_flag_query_index(&connection).is_err());
    assert!(validate_existing_query_indexes(&connection).is_err());
}
