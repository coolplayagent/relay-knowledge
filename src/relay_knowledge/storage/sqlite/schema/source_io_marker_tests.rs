use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::storage::SqliteGraphStore;
use crate::storage::sqlite::schema::marker::{
    mark_schema_initialization_current, schema_initialization_is_current,
};

#[test]
fn source_io_current_marker_reopen_migrates_each_missing_column_without_losing_rows() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    for (table, column) in [
        ("code_repository_file_diagnostics", "io_json"),
        ("code_repository_index_checkpoints", "processed_path_count"),
        ("code_repository_scope_gc_jobs", "source_replan_task_id"),
    ] {
        let database_path = std::env::temp_dir().join(format!(
            "relay-knowledge-source-io-marker-{}-{nonce}-{column}.sqlite",
            std::process::id()
        ));
        {
            let store = SqliteGraphStore::open(&database_path)
                .expect("current file store should initialize");
            let connection = store.connection.lock().expect("connection should lock");
            seed_legacy_rows(&connection);
            assert!(schema_initialization_is_current(&connection).expect("current schema"));
            connection
                .execute(&format!("ALTER TABLE {table} DROP COLUMN {column}"), [])
                .expect("fixture should remove only the selected new column");
            mark_schema_initialization_current(&connection)
                .expect("legacy database should keep its current marker");
            assert!(
                !schema_initialization_is_current(&connection)
                    .expect("missing capability should invalidate the warm-open gate"),
                "missing {table}.{column} must trigger initialization"
            );
        }
        {
            let store = SqliteGraphStore::open(&database_path)
                .expect("closed legacy database should migrate on reopen");
            let connection = store.connection.lock().expect("connection should lock");
            assert!(schema_initialization_is_current(&connection).expect("migrated schema"));
            assert_legacy_rows_preserved(&connection);
            assert_eq!(read_new_fields(&connection), (None, 0, None));
            connection
                .execute_batch(
                    "UPDATE code_repository_file_diagnostics SET io_json = '{}';
                     UPDATE code_repository_index_checkpoints SET processed_path_count = 3;
                     UPDATE code_repository_scope_gc_jobs
                         SET source_replan_task_id = 'replanned-task';",
                )
                .expect("all migrated columns should accept new writes");
        }
        {
            let store = SqliteGraphStore::open(&database_path)
                .expect("migrated database should reopen through the warm gate");
            let connection = store.connection.lock().expect("connection should lock");
            assert!(schema_initialization_is_current(&connection).expect("warm schema"));
            assert_legacy_rows_preserved(&connection);
            assert_eq!(
                read_new_fields(&connection),
                (Some("{}".to_owned()), 3, Some("replanned-task".to_owned()))
            );
        }
        for path in [
            database_path.clone(),
            database_path.with_extension("sqlite-wal"),
            database_path.with_extension("sqlite-shm"),
        ] {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn seed_legacy_rows(connection: &Connection) {
    connection
        .execute_batch(
            "INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                state, indexed_file_count, symbol_count, reference_count, chunk_count, stale
             ) VALUES ('legacy-repo', 'legacy', 'legacy-root', '[]', '[]',
                       'indexed', 2, 3, 4, 5, 0);
             INSERT INTO code_repository_file_diagnostics (
                repository_id, source_scope, path, parse_status, message
             ) VALUES ('legacy-repo', 'legacy-scope', 'src/main.rs', 'failed', 'legacy failure');
             INSERT INTO code_repository_index_checkpoints (
                source_scope, repository_id, state, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, total_path_count, parsed_file_count,
                committed_file_count, committed_symbol_count, committed_reference_count,
                committed_chunk_count, batch_count, last_path, resource_budget_json, updated_at_ms
             ) VALUES ('legacy-scope', 'legacy-repo', 'indexing', 'legacy-commit', 'legacy-tree',
                       '[]', '[]', 3, 2, 2, 3, 4, 5, 1, 'src/main.rs', '{}', 7);
             INSERT INTO code_repository_scope_gc_jobs (
                source_scope, repository_id, phase, deleted_rows, created_at_ms, updated_at_ms
             ) VALUES ('legacy-scope', 'legacy-repo', 'search', 6, 8, 9);",
        )
        .expect("legacy diagnostic, checkpoint and GC rows should seed");
}

fn assert_legacy_rows_preserved(connection: &Connection) {
    let diagnostic = connection
        .query_row(
            "SELECT path, parse_status, message FROM code_repository_file_diagnostics
             WHERE source_scope = 'legacy-scope'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .expect("legacy diagnostic should remain readable");
    assert_eq!(
        diagnostic,
        (
            "src/main.rs".to_owned(),
            "failed".to_owned(),
            "legacy failure".to_owned()
        )
    );
    let checkpoint = connection
        .query_row(
            "SELECT state, total_path_count, parsed_file_count, committed_file_count, last_path
             FROM code_repository_index_checkpoints WHERE source_scope = 'legacy-scope'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, usize>(1)?,
                    row.get::<_, usize>(2)?,
                    row.get::<_, usize>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .expect("legacy checkpoint should remain readable");
    assert_eq!(
        checkpoint,
        ("indexing".to_owned(), 3, 2, 2, "src/main.rs".to_owned())
    );
    let gc_job = connection
        .query_row(
            "SELECT deleted_rows, created_at_ms, updated_at_ms
             FROM code_repository_scope_gc_jobs WHERE source_scope = 'legacy-scope'",
            [],
            |row| {
                Ok((
                    row.get::<_, usize>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            },
        )
        .expect("legacy GC job should remain readable");
    assert_eq!(gc_job, (6, 8, 9));
}

fn read_new_fields(connection: &Connection) -> (Option<String>, usize, Option<String>) {
    connection
        .query_row(
            "SELECT diagnostic.io_json, checkpoint.processed_path_count, gc.source_replan_task_id
             FROM code_repository_file_diagnostics diagnostic
             JOIN code_repository_index_checkpoints checkpoint USING (source_scope)
             JOIN code_repository_scope_gc_jobs gc USING (source_scope)
             WHERE diagnostic.source_scope = 'legacy-scope'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("all source I/O columns should be present after reopening")
}
