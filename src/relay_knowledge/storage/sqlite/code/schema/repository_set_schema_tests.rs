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
                  'code_repository_set_refresh_tasks_claimable',
                  'code_workspace_package_mappings_set_package',
                  'code_workspace_package_mappings_scope'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("repository-set indexes should be inspectable");
    assert_eq!(index_count, 5);
}
