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
                  'code_repository_commit_scopes',
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
    assert_eq!(table_count, 16);

    let alias_index_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_schema
            WHERE type = 'index'
              AND name IN (
                  'code_repository_commit_scopes_scope',
                  'code_repository_commit_scopes_retention'
              )
            ",
            [],
            |row| row.get(0),
        )
        .expect("commit scope alias indexes should be inspectable");
    assert_eq!(alias_index_count, 2);

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

#[test]
fn schema_reopen_does_not_scan_legacy_scopes_into_commit_aliases() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    connection
        .execute(
            "INSERT INTO code_repositories (
                 repository_id, alias, root_path, path_filters_json, language_filters_json,
                 state, indexed_file_count, symbol_count, reference_count, chunk_count, stale
             ) VALUES ('repo', 'fixture', '/tmp/repo', '[]', '[]',
                       'fresh', 0, 0, 0, 0, 0)",
            [],
        )
        .expect("legacy repository should insert");
    connection
        .execute(
            "INSERT INTO code_repository_scopes (
                 source_scope, repository_id, resolved_commit_sha, tree_hash,
                 path_filters_json, language_filters_json, indexed_file_count,
                 symbol_count, reference_count, chunk_count, stale
             ) VALUES ('scope-a', 'repo', 'commit-a', 'tree-a', '[]', '[]',
                       0, 0, 0, 0, 0)",
            [],
        )
        .expect("legacy scope should insert");

    initialize_repository_schema(&connection).expect("schema reopen should remain bounded");

    let alias_count: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_commit_scopes",
            [],
            |row| row.get(0),
        )
        .expect("commit aliases should query");
    assert_eq!(alias_count, 0);
}
