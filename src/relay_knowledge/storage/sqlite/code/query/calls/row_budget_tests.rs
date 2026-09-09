use super::*;

#[test]
fn exhaustion_is_explicit_and_handler_is_cleared_on_every_result() {
    let connection = Connection::open_in_memory().unwrap();
    let sql = "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<10000) SELECT sum(x) FROM n";
    let error = run(&connection, 0, || {
        connection
            .query_row(sql, [], |r| r.get::<_, i64>(0))
            .map_err(StorageError::from)
    })
    .unwrap_err();
    assert!(error.to_string().contains("call query incomplete"));
    assert_eq!(
        connection
            .query_row(sql, [], |r| r.get::<_, i64>(0))
            .unwrap(),
        50_005_000
    );
    assert!(
        run::<()>(&connection, 0, || Err(StorageError::InvalidInput(
            "test".into()
        )))
        .is_err()
    );
    assert_eq!(
        connection
            .query_row(sql, [], |r| r.get::<_, i64>(0))
            .unwrap(),
        50_005_000
    );
    assert_eq!(
        run(&connection, MAX_PROGRESS_CALLBACKS, || Ok(3)).unwrap(),
        3
    );
    assert_eq!(
        connection
            .query_row(sql, [], |r| r.get::<_, i64>(0))
            .unwrap(),
        50_005_000
    );
}
