use super::call_rows_sql;

#[test]
fn call_row_query_keeps_scope_order_and_bound_contracts() {
    let sql = call_rows_sql("AND c.callee_name = ?");

    assert!(sql.contains("WHERE c.source_scope = ?"));
    assert!(sql.contains("AND c.callee_name = ?"));
    assert!(sql.contains("ORDER BY f.is_generated ASC, c.path ASC, c.line_start ASC"));
    assert!(sql.contains("LIMIT ?"));
}
