use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};

use super::{GC_ROW_BATCH_SIZE, PHASES, delete_catalog_route, delete_search_batch, process_one};
use crate::storage::sqlite::schema::marker::SEARCH_ORPHAN_GC_PHASE_MIGRATION;

#[test]
fn code_index_task_retention_gc_replays_one_fixed_file_batch_per_transaction() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_scope_gc_jobs (
                 source_scope TEXT PRIMARY KEY,
                 repository_id TEXT NOT NULL,
                 phase TEXT NOT NULL,
                 search_rowid_cursor INTEGER,
                 deleted_rows INTEGER NOT NULL,
                 created_at_ms INTEGER NOT NULL,
                 updated_at_ms INTEGER NOT NULL,
                 last_error TEXT
             );
             CREATE TABLE code_repository_files (
                 source_scope TEXT NOT NULL,
                 file_id TEXT NOT NULL
             );
             INSERT INTO code_repository_scope_gc_jobs
                 (source_scope, repository_id, phase, deleted_rows,
                  created_at_ms, updated_at_ms, last_error)
             VALUES ('scope-old', 'repo', 'files', 0, 0, 0, NULL);
             WITH RECURSIVE sequence(value) AS (
                 SELECT 0 UNION ALL SELECT value + 1 FROM sequence WHERE value < 1199
             )
             INSERT INTO code_repository_files (source_scope, file_id)
             SELECT 'scope-old', 'file-' || value FROM sequence;",
        )
        .expect("GC fixtures should initialize");

    for pass in 1..=2 {
        let transaction = connection.transaction().expect("transaction should begin");
        process_one(&transaction, "repo", pass).expect("GC batch should replay");
        transaction.commit().expect("GC batch should commit");
        let remaining: usize = connection
            .query_row("SELECT COUNT(*) FROM code_repository_files", [], |row| {
                row.get(0)
            })
            .expect("remaining files should query");
        assert_eq!(remaining, 1_200 - pass as usize * GC_ROW_BATCH_SIZE);
        let progress: (String, usize) = connection
            .query_row(
                "SELECT phase, deleted_rows FROM code_repository_scope_gc_jobs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("durable progress should query");
        assert_eq!(
            progress,
            ("files".to_owned(), pass as usize * GC_ROW_BATCH_SIZE)
        );
    }
}

#[test]
fn code_index_task_retention_gc_bounds_search_physical_rows_per_transaction() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE VIRTUAL TABLE code_repository_search USING fts5(
                 source_scope UNINDEXED,
                 document_kind UNINDEXED,
                 record_id UNINDEXED,
                 path UNINDEXED,
                 language_id UNINDEXED,
                 content
             );
             CREATE TABLE code_repository_search_metadata (
                 source_scope TEXT NOT NULL,
                 document_kind TEXT NOT NULL,
                 record_id TEXT NOT NULL,
                 path TEXT NOT NULL,
                 search_rowid INTEGER NOT NULL
             );
             CREATE INDEX code_repository_search_metadata_scope
                 ON code_repository_search_metadata(source_scope, search_rowid);",
        )
        .expect("search GC schema should initialize");
    for index in 0..300 {
        connection
            .execute(
                "INSERT INTO code_repository_search
                     (source_scope, document_kind, record_id, path, language_id, content)
                 VALUES ('scope-old', 'symbol', ?1, ?2, 'rust', 'content')",
                params![format!("record-{index}"), format!("src/{index}.rs")],
            )
            .expect("search document should insert");
        let search_rowid = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO code_repository_search_metadata
                     (source_scope, document_kind, record_id, path, search_rowid)
                 VALUES ('scope-old', 'symbol', ?1, ?2, ?3)",
                params![
                    format!("record-{index}"),
                    format!("src/{index}.rs"),
                    search_rowid
                ],
            )
            .expect("search owner should insert");
    }

    let transaction = connection.transaction().expect("transaction should begin");
    let (deleted, has_more) =
        delete_search_batch(&transaction, "scope-old").expect("search GC should run");
    transaction.commit().expect("search GC should commit");

    assert_eq!(deleted, GC_ROW_BATCH_SIZE);
    assert!(has_more);
    for table in ["code_repository_search", "code_repository_search_metadata"] {
        let remaining: usize = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("search rows should query");
        assert_eq!(remaining, 44);
    }
}

#[test]
fn code_index_task_control_gc_preserves_catalog_route_as_shard_capacity_reservation() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE storage_repository_shard_scopes (
                 source_scope TEXT PRIMARY KEY,
                 repository_id TEXT NOT NULL
             );
             INSERT INTO storage_repository_shard_scopes VALUES ('scope-old', 'repo');",
        )
        .expect("catalog route fixture should initialize");

    let transaction = connection.transaction().expect("transaction should begin");
    let progress = delete_catalog_route(&transaction, "scope-old")
        .expect("generic scope GC should preserve the partitioned route");
    transaction.commit().expect("transaction should commit");

    assert_eq!(progress, (0, false));
    let route_count: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM storage_repository_shard_scopes WHERE source_scope = 'scope-old'",
            [],
            |row| row.get(0),
        )
        .expect("catalog route should query");
    assert_eq!(route_count, 1);
}

#[test]
fn code_index_task_retention_gc_keysets_more_than_one_interleaved_search_page() {
    let mut connection = search_orphan_gc_connection();
    seed_interleaved_search_rows(&connection, 1_200);

    for (pass, expected_cursor, expected_deleted) in [(1, Some(512), 256), (2, Some(1_024), 512)] {
        let transaction = connection.transaction().expect("transaction should begin");
        process_one(&transaction, "repo", pass).expect("orphan GC page should advance");
        transaction.commit().expect("orphan GC page should commit");
        assert_eq!(
            orphan_job_progress(&connection),
            Some((
                "search_orphans".to_owned(),
                expected_cursor,
                expected_deleted
            ))
        );
    }

    let transaction = connection.transaction().expect("transaction should begin");
    process_one(&transaction, "repo", 3).expect("terminal orphan page should advance");
    transaction
        .commit()
        .expect("terminal orphan page should commit");
    assert_eq!(
        orphan_job_progress(&connection),
        Some(("reference_search_groups".to_owned(), None, 600))
    );
    advance_empty_reference_search_owner_phases(&mut connection, 4);
    assert_eq!(
        orphan_job_progress(&connection),
        Some(("path_tombstones".to_owned(), None, 600))
    );
    assert_eq!(search_scope_count(&connection, "scope-old"), 0);
    assert_eq!(search_scope_count(&connection, "scope-live"), 600);
}

#[test]
fn code_index_task_retention_gc_advances_cursor_across_empty_target_page() {
    let mut connection = search_orphan_gc_connection();
    seed_search_rows(&connection, "scope-live", 1, GC_ROW_BATCH_SIZE);
    seed_search_rows(&connection, "scope-old", GC_ROW_BATCH_SIZE as i64 + 1, 1);

    let transaction = connection.transaction().expect("transaction should begin");
    process_one(&transaction, "repo", 1).expect("unrelated page should advance");
    transaction
        .commit()
        .expect("unrelated page cursor should commit");
    assert_eq!(
        orphan_job_progress(&connection),
        Some(("search_orphans".to_owned(), Some(512), 0))
    );
    assert_eq!(search_scope_count(&connection, "scope-old"), 1);

    let transaction = connection.transaction().expect("transaction should begin");
    process_one(&transaction, "repo", 2).expect("target page should advance");
    transaction.commit().expect("target page should commit");
    assert_eq!(
        orphan_job_progress(&connection),
        Some(("reference_search_groups".to_owned(), None, 1))
    );
    advance_empty_reference_search_owner_phases(&mut connection, 3);
    assert_eq!(
        orphan_job_progress(&connection),
        Some(("path_tombstones".to_owned(), None, 1))
    );
    assert_eq!(search_scope_count(&connection, "scope-old"), 0);
}

#[test]
fn code_index_task_retention_gc_reopens_and_replays_cursor_through_eof() {
    let database_path = temporary_retention_database_path();
    let mut connection = Connection::open(&database_path).expect("database should open");
    initialize_search_orphan_gc(&connection);
    seed_search_rows(&connection, "scope-old", 1, GC_ROW_BATCH_SIZE + 1);

    let transaction = connection.transaction().expect("transaction should begin");
    process_one(&transaction, "repo", 1).expect("rolled-back page should execute");
    transaction.rollback().expect("page should roll back");
    assert_eq!(
        orphan_job_progress(&connection),
        Some(("search_orphans".to_owned(), None, 0))
    );
    assert_eq!(
        search_scope_count(&connection, "scope-old"),
        GC_ROW_BATCH_SIZE + 1
    );
    drop(connection);

    let mut connection = Connection::open(&database_path).expect("database should reopen");
    let transaction = connection.transaction().expect("transaction should begin");
    process_one(&transaction, "repo", 2).expect("same page should replay after reopen");
    transaction.commit().expect("replayed page should commit");
    assert_eq!(
        orphan_job_progress(&connection),
        Some((
            "search_orphans".to_owned(),
            Some(GC_ROW_BATCH_SIZE as i64),
            GC_ROW_BATCH_SIZE
        ))
    );
    assert_eq!(search_scope_count(&connection, "scope-old"), 1);
    drop(connection);

    let mut connection = Connection::open(&database_path).expect("database should reopen");
    let transaction = connection.transaction().expect("transaction should begin");
    process_one(&transaction, "repo", 3).expect("EOF page should advance");
    transaction.commit().expect("EOF transition should commit");
    assert_eq!(
        orphan_job_progress(&connection),
        Some((
            "reference_search_groups".to_owned(),
            None,
            GC_ROW_BATCH_SIZE + 1
        ))
    );
    advance_empty_reference_search_owner_phases(&mut connection, 4);
    assert_eq!(
        orphan_job_progress(&connection),
        Some(("path_tombstones".to_owned(), None, GC_ROW_BATCH_SIZE + 1))
    );
    assert_eq!(search_scope_count(&connection, "scope-old"), 0);
    drop(connection);
    std::fs::remove_file(database_path).expect("temporary database should be removed");
}

fn advance_empty_reference_search_owner_phases(connection: &mut Connection, now_ms: u64) {
    for offset in 0..2 {
        let transaction = connection.transaction().expect("transaction should begin");
        process_one(&transaction, "repo", now_ms + offset)
            .expect("empty grouped owner phase should advance");
        transaction.commit().expect("GC phase should commit");
    }
}

#[test]
fn code_index_task_retention_gc_fails_closed_on_resumed_owned_search_row() {
    let mut connection = search_orphan_gc_connection();
    seed_search_rows(&connection, "scope-old", 2, 1);
    connection
        .execute(
            "INSERT INTO code_repository_search_metadata (
                 source_scope, document_kind, record_id, path, search_rowid
             ) VALUES ('scope-old', 'symbol', 'record-2', 'src/2.rs', 2)",
            [],
        )
        .expect("exact metadata owner should insert");
    connection
        .execute(
            "UPDATE code_repository_scope_gc_jobs
             SET search_rowid_cursor = 1 WHERE source_scope = 'scope-old'",
            [],
        )
        .expect("resumed cursor should persist");

    let transaction = connection.transaction().expect("transaction should begin");
    process_one(&transaction, "repo", 1).expect("owned row should fail closed into job status");
    transaction
        .commit()
        .expect("failed-closed status should commit");

    assert_eq!(
        orphan_job_progress(&connection),
        Some(("search_orphans".to_owned(), Some(1), 0))
    );
    assert_eq!(search_scope_count(&connection, "scope-old"), 1);
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata
                 WHERE search_rowid = 2",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("metadata owner should count"),
        1
    );
    let last_error = connection
        .query_row(
            "SELECT last_error FROM code_repository_scope_gc_jobs
             WHERE source_scope = 'scope-old'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("failed-closed error should persist");
    assert!(last_error.contains("exact metadata owner"));
    assert!(last_error.contains("search_documents"));
}

#[test]
fn legacy_path_tombstone_job_reopens_at_search_orphans_and_finishes() {
    assert_legacy_post_search_job_reopens_and_cleans_orphan("path_tombstones");
}

#[test]
fn legacy_scope_metadata_job_reopens_at_search_orphans_and_finishes() {
    assert_legacy_post_search_job_reopens_and_cleans_orphan("scope_metadata");
}

fn assert_legacy_post_search_job_reopens_and_cleans_orphan(original_phase: &str) {
    let database_path = temporary_retention_database_path();
    {
        let store = crate::storage::SqliteGraphStore::open(&database_path)
            .expect("legacy file store should open");
        let connection = store.connection.lock().expect("connection should lock");
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
                     'repo', 'fixture', '/tmp/repo', '[]', '[]', NULL, NULL, NULL,
                     'fresh', 0, 0, 0, 0, 0, NULL
                 );
                 INSERT INTO code_repository_scopes (
                     source_scope, repository_id, resolved_commit_sha, tree_hash,
                     path_filters_json, language_filters_json, indexed_file_count,
                     symbol_count, reference_count, chunk_count, stale, degraded_reason, retiring
                 ) VALUES (
                     'scope-old', 'repo', 'commit-old', 'tree-old', '[]', '[]',
                     0, 0, 0, 0, 0, NULL, 1
                 );
                 INSERT INTO code_repository_search (
                     source_scope, document_kind, record_id, path, language_id, content
                 ) VALUES (
                     'scope-old', 'symbol', 'orphan-record', 'src/orphan.rs', 'rust',
                     'legacy orphan'
                 );
                 INSERT INTO code_repository_scope_gc_jobs (
                     source_scope, repository_id, phase, search_rowid_cursor, deleted_rows,
                     created_at_ms, updated_at_ms, last_error
                 ) VALUES (
                     'scope-old', 'repo', '{original_phase}', 77, 13, 1, 2, NULL
                 );"
            ))
            .expect("legacy post-search job should initialize");
    }

    let store = crate::storage::SqliteGraphStore::open(&database_path)
        .expect("legacy job should migrate on reopen");
    {
        let mut connection = store.connection.lock().expect("connection should lock");
        assert_eq!(
            orphan_job_progress(&connection),
            Some(("search_orphans".to_owned(), None, 13))
        );
        assert_eq!(search_scope_count(&connection, "scope-old"), 1);
        assert!(
            connection
                .query_row(
                    "SELECT EXISTS (
                         SELECT 1 FROM code_repository_schema_migrations WHERE name = ?1
                     )",
                    [SEARCH_ORPHAN_GC_PHASE_MIGRATION],
                    |row| row.get::<_, bool>(0),
                )
                .expect("retention migration marker should load")
        );

        let transaction = connection.transaction().expect("transaction should begin");
        process_one(&transaction, "repo", 3).expect("orphan page should advance");
        transaction.commit().expect("orphan page should commit");
        assert_eq!(search_scope_count(&connection, "scope-old"), 0);
        assert_eq!(
            orphan_job_progress(&connection),
            Some(("reference_search_groups".to_owned(), None, 14))
        );

        let mut finished = false;
        for pass in 0..=PHASES.len() {
            let transaction = connection.transaction().expect("transaction should begin");
            let retired = process_one(&transaction, "repo", 4 + pass as u64)
                .expect("rewound job should continue idempotently");
            transaction.commit().expect("retention phase should commit");
            if retired.as_deref() == Some("scope-old") {
                finished = true;
                break;
            }
        }
        assert!(finished, "rewound legacy job should reach scope metadata");
        assert_eq!(orphan_job_progress(&connection), None);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM code_repository_scopes
                     WHERE source_scope = 'scope-old'",
                    [],
                    |row| row.get::<_, usize>(0),
                )
                .expect("retired scope should count"),
            0
        );
    }
    drop(store);
    for path in [
        database_path.clone(),
        database_path.with_extension("sqlite-wal"),
        database_path.with_extension("sqlite-shm"),
    ] {
        let _ = std::fs::remove_file(path);
    }
}

fn search_orphan_gc_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_search_orphan_gc(&connection);
    connection
}

fn initialize_search_orphan_gc(connection: &Connection) {
    connection
        .execute_batch(
            "CREATE VIRTUAL TABLE code_repository_search USING fts5(
                 source_scope UNINDEXED,
                 document_kind UNINDEXED,
                 record_id UNINDEXED,
                 path UNINDEXED,
                 language_id UNINDEXED,
                 content
             );
             CREATE TABLE code_repository_scope_gc_jobs (
                 source_scope TEXT PRIMARY KEY,
                 repository_id TEXT NOT NULL,
                 phase TEXT NOT NULL,
                 search_rowid_cursor INTEGER,
                 deleted_rows INTEGER NOT NULL,
                 created_at_ms INTEGER NOT NULL,
                 updated_at_ms INTEGER NOT NULL,
                 last_error TEXT
             );
             CREATE TABLE code_repository_search_metadata (
                 source_scope TEXT NOT NULL,
                 document_kind TEXT NOT NULL,
                 record_id TEXT NOT NULL,
                 path TEXT NOT NULL,
                 search_rowid INTEGER PRIMARY KEY,
                 UNIQUE (source_scope, document_kind, record_id)
             );
             CREATE TABLE code_repository_reference_search_groups (
                 source_scope TEXT NOT NULL,
                 group_id TEXT NOT NULL,
                 path TEXT NOT NULL,
                 language_id TEXT NOT NULL,
                 name TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 target_hint TEXT NOT NULL,
                 occurrence_count INTEGER NOT NULL,
                 PRIMARY KEY (source_scope, group_id)
             );
             CREATE TABLE code_repository_reference_search_manifests (
                 source_scope TEXT NOT NULL PRIMARY KEY,
                 projection_version INTEGER NOT NULL,
                 reference_count INTEGER NOT NULL,
                 group_count INTEGER NOT NULL
             );
             INSERT INTO code_repository_scope_gc_jobs (
                 source_scope, repository_id, phase, search_rowid_cursor,
                 deleted_rows, created_at_ms, updated_at_ms, last_error
             ) VALUES (
                 'scope-old', 'repo', 'search_orphans', NULL, 0, 0, 0, NULL
             );",
        )
        .expect("orphan GC schema should initialize");
}

fn seed_interleaved_search_rows(connection: &Connection, count: usize) {
    for rowid in 1..=count {
        let source_scope = if rowid % 2 == 0 {
            "scope-old"
        } else {
            "scope-live"
        };
        insert_search_row(connection, rowid as i64, source_scope);
    }
}

fn seed_search_rows(connection: &Connection, source_scope: &str, first_rowid: i64, count: usize) {
    for offset in 0..count {
        insert_search_row(connection, first_rowid + offset as i64, source_scope);
    }
}

fn insert_search_row(connection: &Connection, rowid: i64, source_scope: &str) {
    connection
        .execute(
            "INSERT INTO code_repository_search (
                 rowid, source_scope, document_kind, record_id,
                 path, language_id, content
             ) VALUES (?1, ?2, 'symbol', ?3, ?4, 'rust', 'content')",
            params![
                rowid,
                source_scope,
                format!("record-{rowid}"),
                format!("src/{rowid}.rs")
            ],
        )
        .expect("search row should insert");
}

fn orphan_job_progress(connection: &Connection) -> Option<(String, Option<i64>, usize)> {
    connection
        .query_row(
            "SELECT phase, search_rowid_cursor, deleted_rows
             FROM code_repository_scope_gc_jobs WHERE source_scope = 'scope-old'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .expect("orphan GC progress should query")
}

fn search_scope_count(connection: &Connection, source_scope: &str) -> usize {
    connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_search WHERE source_scope = ?1",
            params![source_scope],
            |row| row.get(0),
        )
        .expect("search scope rows should count")
}

fn temporary_retention_database_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "relay-knowledge-retention-orphan-{}-{nonce}.sqlite",
        std::process::id()
    ))
}
