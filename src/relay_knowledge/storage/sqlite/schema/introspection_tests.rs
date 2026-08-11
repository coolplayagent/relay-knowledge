use super::{
    index_has_columns, table_column_is_not_null, table_exists, table_has_columns,
    table_has_exact_columns, table_has_primary_key_columns, table_has_unique_columns,
};

#[test]
fn schema_introspection_checks_order_constraints_and_nullability() {
    let connection = rusqlite::Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "CREATE TABLE sample (
                 scope TEXT NOT NULL,
                 identity TEXT NOT NULL,
                 row_key INTEGER NOT NULL,
                 PRIMARY KEY (scope, identity),
                 UNIQUE (row_key)
             );",
        )
        .expect("sample schema should create");

    assert!(table_exists(&connection, "sample").expect("table should inspect"));
    assert!(
        table_has_columns(&connection, "sample", &["identity", "scope"])
            .expect("column membership should inspect")
    );
    assert!(
        table_has_exact_columns(&connection, "sample", &["scope", "identity", "row_key"])
            .expect("column order should inspect")
    );
    assert!(
        table_has_primary_key_columns(&connection, "sample", &["scope", "identity"])
            .expect("primary key should inspect")
    );
    assert!(
        table_column_is_not_null(&connection, "sample", "row_key")
            .expect("nullability should inspect")
    );
    assert!(
        table_has_unique_columns(&connection, "sample", &["row_key"])
            .expect("unique key should inspect")
    );
}

#[test]
fn schema_introspection_rejects_partial_and_expression_indexes() {
    let connection = rusqlite::Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "CREATE TABLE sample (identity TEXT NOT NULL, row_key INTEGER);
             CREATE UNIQUE INDEX partial_unique
             ON sample(row_key) WHERE(row_key IS NOT NULL);
             CREATE INDEX partial_lookup
             ON sample(identity) WHERE(identity <> '');
             CREATE INDEX expression_lookup
             ON sample(lower(identity));
             CREATE INDEX complete_lookup ON sample(identity);",
        )
        .expect("adversarial indexes should create");

    assert!(
        !table_has_unique_columns(&connection, "sample", &["row_key"])
            .expect("partial unique key should inspect")
    );
    assert!(
        !index_has_columns(&connection, "partial_lookup", &["identity"])
            .expect("partial lookup should inspect")
    );
    assert!(
        !index_has_columns(&connection, "expression_lookup", &["identity"])
            .expect("expression lookup should inspect")
    );
    assert!(
        index_has_columns(&connection, "complete_lookup", &["identity"])
            .expect("complete lookup should inspect")
    );
}
