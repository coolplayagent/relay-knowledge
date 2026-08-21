use rusqlite::Connection;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::initialize_code_schema;
use crate::storage::SqliteGraphStore;

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

    let scan_cursor_table: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'code_repository_retention_scans'",
            [],
            |row| row.get(0),
        )
        .expect("repository retention scan table should query");
    assert_eq!(scan_cursor_table, 1);

    let catalog_revision_column: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('code_repository_retention_scans')
             WHERE name = 'catalog_revision' AND type = 'INTEGER' AND \"notnull\" = 1",
            [],
            |row| row.get(0),
        )
        .expect("candidate catalog revision column should query");
    assert_eq!(catalog_revision_column, 1);

    let catalog_table: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'code_repository_retention_catalog'",
            [],
            |row| row.get(0),
        )
        .expect("repository retention catalog should query");
    assert_eq!(catalog_table, 1);

    let activity_table: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'code_repository_retention_activity'",
            [],
            |row| row.get(0),
        )
        .expect("repository activity table should query");
    assert_eq!(activity_table, 1);

    let activity_index_columns = connection
        .prepare("PRAGMA index_info(code_repository_retention_activity_order)")
        .expect("repository activity index should prepare")
        .query_map([], |row| row.get::<_, String>(2))
        .expect("repository activity index should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("repository activity index columns should collect");
    assert_eq!(activity_index_columns, ["activity_ms", "repository_id"]);

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

#[test]
fn retention_schema_upgrades_candidate_scans_with_a_catalog_revision() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_retention_scans (
                 scan_id INTEGER PRIMARY KEY,
                 max_indexed_repositories INTEGER NOT NULL,
                 cursor_activity_ms INTEGER NOT NULL,
                 cursor_repository_id TEXT NOT NULL,
                 eligible_count INTEGER NOT NULL,
                 oldest_repository_id TEXT,
                 oldest_source_scope TEXT,
                 created_at_ms INTEGER NOT NULL,
                 updated_at_ms INTEGER NOT NULL
             );",
        )
        .expect("legacy candidate scan table should create");

    initialize_code_schema(&connection).expect("legacy schema should upgrade");

    let catalog_revision_column: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('code_repository_retention_scans')
             WHERE name = 'catalog_revision' AND type = 'INTEGER'
               AND \"notnull\" = 1 AND dflt_value = '0'",
            [],
            |row| row.get(0),
        )
        .expect("upgraded candidate catalog revision column should query");
    assert_eq!(catalog_revision_column, 1);
}

#[test]
fn retention_activity_dirty_enqueue_survives_outer_upsert_conflict_policy() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("schema should initialize");
    connection
        .execute_batch(
            "INSERT INTO code_repositories (
                 repository_id, alias, root_path, path_filters_json, language_filters_json,
                 state, indexed_file_count, symbol_count, reference_count, chunk_count, stale
             ) VALUES ('repo', 'fixture', '/repo', '[]', '[]', 'registered', 0, 0, 0, 0, 1);

             INSERT INTO code_repository_scopes (
                 source_scope, repository_id, resolved_commit_sha, tree_hash,
                 path_filters_json, language_filters_json, indexed_file_count,
                 symbol_count, reference_count, chunk_count, stale
             ) VALUES ('scope', 'repo', 'commit-a', 'tree', '[]', '[]', 0, 0, 0, 0, 0)
             ON CONFLICT(source_scope) DO UPDATE SET
                 resolved_commit_sha = excluded.resolved_commit_sha;

             INSERT INTO code_repository_scopes (
                 source_scope, repository_id, resolved_commit_sha, tree_hash,
                 path_filters_json, language_filters_json, indexed_file_count,
                 symbol_count, reference_count, chunk_count, stale
             ) VALUES ('scope', 'repo', 'commit-b', 'tree', '[]', '[]', 0, 0, 0, 0, 0)
             ON CONFLICT(source_scope) DO UPDATE SET
                 repository_id = excluded.repository_id,
                 resolved_commit_sha = excluded.resolved_commit_sha;",
        )
        .expect("same-scope upserts should idempotently enqueue retention activity");

    let dirty_count: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_retention_activity_dirty
             WHERE repository_id = 'repo'",
            [],
            |row| row.get(0),
        )
        .expect("dirty activity should query");
    assert_eq!(dirty_count, 1);
}

#[test]
fn retention_schema_replaces_legacy_conflict_policy_trigger() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("schema should initialize");
    connection
        .execute_batch(
            "DROP TRIGGER code_repository_retention_activity_scope_insert;
             CREATE TRIGGER code_repository_retention_activity_scope_insert
             AFTER INSERT ON code_repository_scopes BEGIN
                 INSERT OR IGNORE INTO code_repository_retention_activity_dirty (repository_id)
                 VALUES (NEW.repository_id);
             END;",
        )
        .expect("legacy trigger should install");

    initialize_code_schema(&connection).expect("legacy trigger should upgrade");

    let trigger_sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'trigger'
               AND name = 'code_repository_retention_activity_scope_insert'",
            [],
            |row| row.get(0),
        )
        .expect("upgraded trigger should query");
    assert!(trigger_sql.contains("WHERE NOT EXISTS"));
    assert!(!trigger_sql.contains("INSERT OR IGNORE"));
}

#[test]
fn reopening_previous_schema_version_replaces_legacy_retention_trigger() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be monotonic")
        .as_nanos();
    let path = std::env::temp_dir()
        .join("relay-knowledge-tests")
        .join(format!(
            "legacy-retention-trigger-{}-{suffix}.sqlite",
            std::process::id()
        ));
    {
        let store = SqliteGraphStore::open(&path).expect("store should open");
        let connection = store.connection.lock().expect("connection should lock");
        connection
            .execute_batch(
                "DROP TRIGGER code_repository_retention_activity_scope_insert;
                 CREATE TRIGGER code_repository_retention_activity_scope_insert
                 AFTER INSERT ON code_repository_scopes BEGIN
                     INSERT OR IGNORE INTO code_repository_retention_activity_dirty (repository_id)
                     VALUES (NEW.repository_id);
                 END;",
            )
            .expect("legacy trigger should install");
        connection
            .execute(
                "UPDATE relay_storage_schema_state
                 SET version = version - 1
                 WHERE key = 'sqlite_graph_store'",
                [],
            )
            .expect("previous schema version should install");
    }

    let reopened = SqliteGraphStore::open(&path).expect("previous schema should upgrade on open");
    let connection = reopened.connection.lock().expect("connection should lock");
    let trigger_sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'trigger'
               AND name = 'code_repository_retention_activity_scope_insert'",
            [],
            |row| row.get(0),
        )
        .expect("upgraded trigger should query");
    assert!(trigger_sql.contains("WHERE NOT EXISTS"));
    assert!(!trigger_sql.contains("INSERT OR IGNORE"));
    drop(connection);
    drop(reopened);
    let _ = std::fs::remove_file(path);
}
