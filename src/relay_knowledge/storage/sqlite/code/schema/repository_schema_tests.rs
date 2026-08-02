use super::*;

#[test]
fn creates_repository_fact_schema_with_edge_defaults() {
    let connection = Connection::open_in_memory().expect("database should open");

    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_repository_schema(&connection).expect("repository schema should be idempotent");

    let table_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_schema
            WHERE type = 'table'
              AND name IN (
                  'code_repository_schema_migrations',
                  'code_repositories',
                  'code_repository_aliases',
                  'code_repository_scopes',
                  'code_repository_files',
                  'code_repository_symbols',
                  'code_repository_references',
                  'code_repository_imports',
                  'code_repository_dependencies',
                  'code_repository_calls',
                  'code_repository_feature_flags',
                  'code_repository_routes',
                  'code_repository_chunks',
                  'code_repository_file_diagnostics',
                  'code_repository_path_tombstones'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("repository tables should be inspectable");
    assert_eq!(table_count, 15);

    let resolution_default: String = connection
        .query_row(
            "
            SELECT dflt_value
            FROM pragma_table_info('code_repository_references')
            WHERE name = 'resolution_state'
            ",
            [],
            |row| row.get(0),
        )
        .expect("reference resolution default should exist");
    assert_eq!(resolution_default, "'unresolved'");
}
