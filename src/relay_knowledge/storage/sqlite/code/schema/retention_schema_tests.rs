use rusqlite::Connection;

use super::super::initialize_code_schema;

#[test]
fn retention_schema_adds_logical_retirement_and_durable_jobs() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("schema should initialize");

    let retiring_column: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('code_repository_scopes')
             WHERE name = 'retiring' AND dflt_value = '0'",
            [],
            |row| row.get(0),
        )
        .expect("retiring column should query");
    assert_eq!(retiring_column, 1);

    let gc_table: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'code_repository_scope_gc_jobs'",
            [],
            |row| row.get(0),
        )
        .expect("gc table should query");
    assert_eq!(gc_table, 1);

    let cutoff_generation_column: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('code_repository_retention_jobs')
             WHERE name = 'cutoff_publication_generation' AND dflt_value = '0'",
            [],
            |row| row.get(0),
        )
        .expect("retention generation column should query");
    assert_eq!(cutoff_generation_column, 1);

    let member_index_columns = connection
        .prepare("PRAGMA index_info(code_repository_set_members_repository_scope)")
        .expect("set-member retention index should prepare")
        .query_map([], |row| row.get::<_, String>(2))
        .expect("set-member retention index should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("set-member retention columns should collect");
    assert_eq!(
        member_index_columns,
        ["repository_id", "source_scope", "set_id"]
    );
}

#[test]
fn retention_schema_upgrades_parent_jobs_with_a_publication_generation() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_retention_jobs (
                 repository_id TEXT PRIMARY KEY,
                 initial_scope TEXT NOT NULL,
                 cutoff_ms INTEGER NOT NULL,
                 phase TEXT NOT NULL,
                 created_at_ms INTEGER NOT NULL,
                 updated_at_ms INTEGER NOT NULL,
                 last_error TEXT
             );",
        )
        .expect("legacy retention table should create");

    initialize_code_schema(&connection).expect("legacy schema should upgrade");

    let cutoff_generation_column: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('code_repository_retention_jobs')
             WHERE name = 'cutoff_publication_generation'
               AND type = 'INTEGER' AND \"notnull\" = 1 AND dflt_value = '0'",
            [],
            |row| row.get(0),
        )
        .expect("upgraded retention generation column should query");
    assert_eq!(cutoff_generation_column, 1);
}
