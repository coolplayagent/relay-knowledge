use super::super::initialize_code_schema;
use super::initialize_retention_schema;
use crate::storage::sqlite::schema::marker::SEARCH_ORPHAN_GC_PHASE_MIGRATION;
use rusqlite::Connection;

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

    let search_cursor_column: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('code_repository_scope_gc_jobs')
             WHERE name = 'search_rowid_cursor' AND type = 'INTEGER' AND \"notnull\" = 0",
            [],
            |row| row.get(0),
        )
        .expect("search cursor column should query");
    assert_eq!(search_cursor_column, 1);

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
fn retention_schema_upgrades_existing_jobs_with_a_nullable_search_cursor() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_scope_gc_jobs (
                 source_scope TEXT PRIMARY KEY,
                 repository_id TEXT NOT NULL,
                 phase TEXT NOT NULL,
                 deleted_rows INTEGER NOT NULL,
                 created_at_ms INTEGER NOT NULL,
                 updated_at_ms INTEGER NOT NULL,
                 last_error TEXT
             );
             INSERT INTO code_repository_scope_gc_jobs (
                 source_scope, repository_id, phase, deleted_rows,
                 created_at_ms, updated_at_ms, last_error
             ) VALUES ('legacy-scope', 'legacy-repo', 'search_documents', 7, 1, 2, NULL);",
        )
        .expect("legacy retention job should initialize");

    initialize_code_schema(&connection).expect("legacy retention schema should upgrade");

    let job: (String, usize, Option<i64>) = connection
        .query_row(
            "SELECT phase, deleted_rows, search_rowid_cursor
             FROM code_repository_scope_gc_jobs WHERE source_scope = 'legacy-scope'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("legacy job should remain readable");
    assert_eq!(job, ("search_documents".to_owned(), 7, None));
}

#[test]
fn retention_search_orphan_rewind_and_marker_roll_back_together() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("schema should initialize");
    connection
        .execute_batch(&format!(
            "DELETE FROM code_repository_schema_migrations
             WHERE name = '{SEARCH_ORPHAN_GC_PHASE_MIGRATION}';
             INSERT INTO code_repositories (
                 repository_id, alias, root_path, path_filters_json, language_filters_json,
                 last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                 indexed_file_count, symbol_count, reference_count, chunk_count,
                 stale, degraded_reason
             ) VALUES (
                 'legacy-repo', 'legacy', '/tmp/legacy', '[]', '[]', NULL, NULL, NULL,
                 'empty', 0, 0, 0, 0, 1, NULL
             );
             INSERT INTO code_repository_scope_gc_jobs (
                 source_scope, repository_id, phase, search_rowid_cursor, deleted_rows,
                 created_at_ms, updated_at_ms, last_error
             ) VALUES ('legacy-scope', 'legacy-repo', 'path_tombstones', 91, 7, 1, 2, NULL);
             CREATE TRIGGER fail_search_orphan_rewind_marker
             BEFORE INSERT ON code_repository_schema_migrations
             WHEN NEW.name = '{SEARCH_ORPHAN_GC_PHASE_MIGRATION}'
             BEGIN
                 SELECT RAISE(ABORT, 'injected search-orphan rewind marker failure');
             END;"
        ))
        .expect("legacy job and failure trigger should initialize");

    initialize_retention_schema(&connection)
        .expect_err("rewind marker failure should roll back the phase update");
    assert_eq!(
        retention_job_state(&connection),
        ("path_tombstones".to_owned(), Some(91), 7)
    );
    assert!(!retention_rewind_marker_applied(&connection));

    connection
        .execute_batch("DROP TRIGGER fail_search_orphan_rewind_marker")
        .expect("failure trigger should drop");
    initialize_retention_schema(&connection).expect("rewind should retry");
    assert_eq!(
        retention_job_state(&connection),
        ("search_orphans".to_owned(), None, 7)
    );
    assert!(retention_rewind_marker_applied(&connection));

    connection
        .execute(
            "UPDATE code_repository_scope_gc_jobs
             SET phase = 'scope_metadata', search_rowid_cursor = NULL
             WHERE source_scope = 'legacy-scope'",
            [],
        )
        .expect("current job should advance past orphan cleanup");
    initialize_retention_schema(&connection).expect("marked migration should remain idempotent");
    assert_eq!(
        retention_job_state(&connection),
        ("scope_metadata".to_owned(), None, 7)
    );
}

fn retention_job_state(connection: &Connection) -> (String, Option<i64>, usize) {
    connection
        .query_row(
            "SELECT phase, search_rowid_cursor, deleted_rows
             FROM code_repository_scope_gc_jobs WHERE source_scope = 'legacy-scope'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("legacy retention job should load")
}

fn retention_rewind_marker_applied(connection: &Connection) -> bool {
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM code_repository_schema_migrations WHERE name = ?1
             )",
            [SEARCH_ORPHAN_GC_PHASE_MIGRATION],
            |row| row.get(0),
        )
        .expect("retention rewind marker should load")
}
