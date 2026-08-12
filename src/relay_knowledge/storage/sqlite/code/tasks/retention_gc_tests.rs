use rusqlite::{Connection, params};

use super::{GC_ROW_BATCH_SIZE, delete_catalog_route, delete_search_batch, process_one};

#[test]
fn code_index_task_retention_gc_replays_one_fixed_file_batch_per_transaction() {
    let mut connection = Connection::open_in_memory().expect("database should open");
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
