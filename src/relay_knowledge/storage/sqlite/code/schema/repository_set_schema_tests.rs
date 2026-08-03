use super::*;

#[test]
fn creates_repository_set_overlay_and_workspace_schema() {
    let connection = Connection::open_in_memory().expect("database should open");

    initialize_repository_set_schema(&connection).expect("repository-set schema should initialize");
    initialize_repository_set_schema(&connection)
        .expect("repository-set schema should be idempotent");

    let table_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_schema
            WHERE type = 'table'
              AND name IN (
                  'code_repository_sets',
                  'code_repository_set_members',
                  'code_repository_cross_edges',
                  'code_repository_set_overlay_status',
                  'code_repository_set_refresh_tasks',
                  'code_workspace_package_mappings'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("repository-set tables should be inspectable");
    assert_eq!(table_count, 6);

    let index_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_schema
            WHERE type = 'index'
              AND name IN (
                  'code_repository_set_members_scope',
                  'code_repository_cross_edges_set_scope',
                  'code_repository_cross_edges_origin_selector',
                  'code_repository_cross_edges_target_selector',
                  'code_repository_set_refresh_tasks_claimable',
                  'code_workspace_package_mappings_set_package',
                  'code_workspace_package_mappings_scope'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("repository-set indexes should be inspectable");
    assert_eq!(index_count, 7);

    let origin_plan = query_plan(
        &connection,
        "SELECT edge_id FROM code_repository_cross_edges
         WHERE set_id = 'set-a'
           AND from_source_scope = 'scope-a'
           AND from_record_kind = 'module_reference'
           AND from_path = 'src/lib.rs'",
    );
    assert!(
        origin_plan.contains("code_repository_cross_edges_origin_selector"),
        "origin selector should use its composite index: {origin_plan}"
    );
    let target_plan = query_plan(
        &connection,
        "SELECT edge_id FROM code_repository_cross_edges
         WHERE set_id = 'set-a'
           AND to_source_scope = 'scope-b'
           AND to_record_kind = 'code_file'
           AND to_record_id = 'file-b'",
    );
    assert!(
        target_plan.contains("code_repository_cross_edges_target_selector"),
        "target selector should use its composite index: {target_plan}"
    );
}

fn query_plan(connection: &Connection, sql: &str) -> String {
    connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("query plan should prepare")
        .query_map([], |row| row.get::<_, String>(3))
        .expect("query plan should execute")
        .collect::<Result<Vec<_>, _>>()
        .expect("query plan should collect")
        .join("\n")
}
