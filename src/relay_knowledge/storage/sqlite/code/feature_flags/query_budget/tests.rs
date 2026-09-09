use super::*;
use crate::storage::sqlite::code::feature_flags::{
    insert_records, knowledge,
    test_support::{record, request, status},
};

#[test]
fn feature_flag_query_work_budget() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().unwrap();
    let mut connection = store.connection.lock().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let tx = connection.transaction().unwrap();
    insert_records(
        &tx,
        &[
            record("noise", "unrelated", "config_key"),
            record("target", "selected_switch", "config_key"),
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    connection.execute_batch("WITH RECURSIVE sequence(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM sequence WHERE n < 11000)
      INSERT INTO code_repository_feature_flags SELECT repository_id, source_scope, 'noise-' || n,
      'noise-usage-' || n, file_id, path, language_id, name || n, source_kind, source_key || n, edge_kind,
      confidence_basis_points, confidence_tier, byte_start, byte_end, line_start, line_end, excerpt, metadata_json
      FROM code_repository_feature_flags, sequence WHERE usage_id = 'noise';").unwrap();
    let mut query = request();
    query.limit = 1;
    query.query = Some("selected_switch".to_owned());
    let flags = knowledge::search(&connection, &status(), &query).unwrap();
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].source_key, "selected_switch");
    let narrow = LAST_STEPS.get();
    assert!(
        (1..=2_000_000).contains(&narrow),
        "narrow query steps: {narrow}"
    );
    println!(
        "SELF_ITERATION_METRIC {{\"name\":\"feature_flag_narrow_vm_steps\",\"value\":{narrow},\"budget\":2000000}}"
    );
    query.query = Some(
        (0..30)
            .map(|i| format!("absentterm{i}"))
            .collect::<Vec<_>>()
            .join(" "),
    );
    assert!(matches!(
        knowledge::search(&connection, &status(), &query),
        Err(StorageError::QueryBudgetExceeded(_))
    ));
    let exhausted = LAST_STEPS.get();
    assert_eq!(exhausted, 4_097_000);
    println!(
        "SELF_ITERATION_METRIC {{\"name\":\"feature_flag_exhausted_vm_steps\",\"value\":{exhausted},\"budget\":4097000}}"
    );
    query.query = Some("selected_switch".to_owned());
    assert_eq!(
        knowledge::search(&connection, &status(), &query)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn budget_is_cumulative_and_handler_clears_after_success_and_errors() {
    let connection = Connection::open_in_memory().unwrap();
    let sql = "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<10000) SELECT sum(x) FROM n";
    let mut completed = 0;
    let error = run(&connection, || {
        for _ in 0..100 {
            connection.query_row(sql, [], |r| r.get::<_, i64>(0))?;
            completed += 1;
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(error, StorageError::QueryBudgetExceeded(_)));
    assert!((1..100).contains(&completed));
    assert_eq!(
        connection
            .query_row(sql, [], |r| r.get::<_, i64>(0))
            .unwrap(),
        50_005_000
    );
    assert!(matches!(
        run::<()>(&connection, || Err(StorageError::InvalidInput(
            "invalid".into()
        ))),
        Err(StorageError::InvalidInput(_))
    ));
    assert_eq!(
        connection
            .query_row(sql, [], |r| r.get::<_, i64>(0))
            .unwrap(),
        50_005_000
    );
    let external_interrupt = run::<()>(&connection, || {
        Err(StorageError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_INTERRUPT),
            None,
        )))
    });
    assert!(matches!(external_interrupt, Err(StorageError::Sqlite(_))));
    assert_eq!(run(&connection, || Ok(7)).unwrap(), 7);
    assert_eq!(
        connection
            .query_row(sql, [], |r| r.get::<_, i64>(0))
            .unwrap(),
        50_005_000
    );
}
