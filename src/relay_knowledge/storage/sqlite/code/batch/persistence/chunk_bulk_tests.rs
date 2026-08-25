//! Bounded multi-values persistence contracts for chunk facts and ordered search projection.

use std::sync::Mutex;

use rusqlite::{Connection, limits::Limit, params};

use crate::{
    domain::{
        CodeIndexBatch, CodeIndexResourceBudget, CodeIndexSession, CodeParseStatus,
        CodeRepositoryRegistration, RepositoryCodeChunkRecord, RepositoryCodeFileRecord,
        RepositoryCodeRange,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore, StorageError},
};

use super::{
    CHUNK_INSERT_BATCH_SIZE, CHUNK_INSERT_BIND_COUNT, CHUNK_INSERT_COLUMN_COUNT,
    insert_chunk_facts, insert_chunks,
};

static TRACED_CHUNK_INSERTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[test]
fn code_index_persistence_performance_suite_chunk_trace_reduces_1025_facts_to_two_statements() {
    assert_eq!(CHUNK_INSERT_BATCH_SIZE, 1_024);
    assert_eq!(CHUNK_INSERT_COLUMN_COUNT, 12);
    assert_eq!(CHUNK_INSERT_BIND_COUNT, 12_288);
    let mut connection = chunk_database();
    let chunks = (0..=CHUNK_INSERT_BATCH_SIZE).map(chunk).collect::<Vec<_>>();
    let batch = chunk_batch("scope", chunks.clone());
    TRACED_CHUNK_INSERTS
        .lock()
        .expect("chunk trace should lock")
        .clear();
    connection.trace(Some(capture_chunk_insert));
    let transaction = connection.transaction().expect("transaction should start");

    insert_chunks(&transaction, &batch).expect("chunk facts and search rows should persist");

    transaction.commit().expect("transaction should commit");
    connection.trace(None);
    let traced = TRACED_CHUNK_INSERTS
        .lock()
        .expect("chunk trace should lock")
        .clone();
    assert_eq!(
        traced.len(),
        2,
        "1,025 chunk facts must execute one fixed 1,024-row statement and one tail statement"
    );
    assert!(traced[0].contains("chunk-1023"));
    assert!(!traced[0].contains("chunk-1024"));
    assert!(traced[1].contains("chunk-1024"));
    let expected_ids = chunks
        .iter()
        .map(|record| record.chunk_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        ordered_ids(&connection, "code_repository_chunks"),
        expected_ids
    );
    assert_eq!(
        ordered_ids(&connection, "code_repository_search"),
        expected_ids
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("metadata should count"),
        chunks.len()
    );
}

#[test]
fn chunk_fact_groups_follow_lower_and_exact_runtime_variable_limits() {
    let mut grouped = chunk_database();
    grouped.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 24);
    let grouped_transaction = grouped.transaction().expect("transaction should start");
    insert_chunk_facts(&grouped_transaction, &(0..5).map(chunk).collect::<Vec<_>>())
        .expect("two rows should fit the lower runtime limit");
    assert_eq!(
        ordered_ids(&grouped_transaction, "code_repository_chunks"),
        (0..5)
            .map(|index| format!("chunk-{index:04}"))
            .collect::<Vec<_>>()
    );
    grouped_transaction
        .commit()
        .expect("grouped transaction should commit");

    let mut exact = chunk_database();
    exact.set_limit(
        Limit::SQLITE_LIMIT_VARIABLE_NUMBER,
        i32::try_from(CHUNK_INSERT_COLUMN_COUNT).expect("column count should fit"),
    );
    let exact_transaction = exact.transaction().expect("transaction should start");
    insert_chunk_facts(&exact_transaction, &[chunk(0)])
        .expect("the maximum host-parameter index is inclusive");
    exact_transaction
        .commit()
        .expect("exact-limit transaction should commit");

    let mut short = chunk_database();
    short.set_limit(
        Limit::SQLITE_LIMIT_VARIABLE_NUMBER,
        i32::try_from(CHUNK_INSERT_COLUMN_COUNT - 1).expect("column count should fit"),
    );
    let short_transaction = short.transaction().expect("transaction should start");
    let error = insert_chunk_facts(&short_transaction, &[chunk(0)])
        .expect_err("fewer variables than one chunk row requires must fail closed");
    assert!(matches!(
        error,
        StorageError::Invariant(message) if message.contains("12-column chunk row")
    ));
    assert!(
        ordered_ids(&short_transaction, "code_repository_chunks").is_empty(),
        "a rejected limit must not write a partial fact"
    );
    short_transaction
        .rollback()
        .expect("short-limit transaction should roll back");
}

#[tokio::test]
async fn failed_chunk_tail_rolls_back_and_the_staged_batch_replays_exactly_once() {
    let source_scope = "git_snapshot:chunk-bulk-tail";
    let store = registered_store(source_scope).await;
    let mut chunks = (0..=CHUNK_INSERT_BATCH_SIZE)
        .map(|index| scoped_chunk(source_scope, index))
        .collect::<Vec<_>>();
    chunks[CHUNK_INSERT_BATCH_SIZE].chunk_id = chunks[0].chunk_id.clone();
    let failed_batch = chunk_batch(source_scope, chunks.clone());

    let error = store
        .apply_code_index_batch(failed_batch)
        .await
        .expect_err("a duplicate in the tail statement must reject the whole fact transaction");

    assert!(error.to_string().contains("UNIQUE constraint failed"));
    assert_eq!(scope_counts(&store, source_scope).await, (0, 0, 0));
    let failed_checkpoint = store
        .code_index_checkpoint(source_scope.to_owned())
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should exist");
    assert_eq!(failed_checkpoint.batch_count, 0);
    assert_eq!(failed_checkpoint.committed_chunk_count, 0);
    assert_eq!(batch_staging_state(&store, source_scope).await, "staged");

    chunks[CHUNK_INSERT_BATCH_SIZE].chunk_id = format!("chunk-{CHUNK_INSERT_BATCH_SIZE:04}");
    let expected_ids = chunks
        .iter()
        .map(|record| record.chunk_id.clone())
        .collect::<Vec<_>>();
    let checkpoint = store
        .apply_code_index_batch(chunk_batch(source_scope, chunks))
        .await
        .expect("the corrected staged batch should replay");

    assert_eq!(checkpoint.batch_count, 1);
    assert_eq!(checkpoint.committed_chunk_count, expected_ids.len());
    assert_eq!(scope_chunk_ids(&store, source_scope).await, expected_ids);
    assert_eq!(
        scope_counts(&store, source_scope).await,
        (1_025, 1_025, 1_025)
    );
    assert_eq!(batch_staging_state(&store, source_scope).await, "published");
}

fn capture_chunk_insert(sql: &str) {
    if sql
        .trim_start()
        .starts_with("INSERT INTO code_repository_chunks")
    {
        TRACED_CHUNK_INSERTS
            .lock()
            .expect("chunk trace should lock")
            .push(sql.to_owned());
    }
}

fn chunk(index: usize) -> RepositoryCodeChunkRecord {
    scoped_chunk("scope", index)
}

fn scoped_chunk(source_scope: &str, index: usize) -> RepositoryCodeChunkRecord {
    let offset = u32::try_from(index).expect("fixture index should fit");
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        chunk_id: format!("chunk-{index:04}"),
        file_id: "file".to_owned(),
        path: "src/lib.rs".to_owned(),
        language_id: "rust".to_owned(),
        content: format!("pub fn chunk_{index:04}() {{}}"),
        byte_range: RepositoryCodeRange {
            start: offset,
            end: offset + 1,
        },
        line_range: RepositoryCodeRange {
            start: offset + 1,
            end: offset + 1,
        },
        symbol_snapshot_id: (index % 2 == 0).then(|| format!("symbol-{index:04}")),
    }
}

fn chunk_batch(source_scope: &str, chunks: Vec<RepositoryCodeChunkRecord>) -> CodeIndexBatch {
    CodeIndexBatch {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        batch_index: 1,
        parsed_byte_count: 4_096,
        files: vec![RepositoryCodeFileRecord {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            file_id: "file".to_owned(),
            path: "src/lib.rs".to_owned(),
            language_id: "rust".to_owned(),
            blob_hash: "blob".to_owned(),
            byte_len: 4_096,
            line_count: 512,
            parse_status: CodeParseStatus::Parsed,
            is_generated: false,
            degraded_reason: None,
        }],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks,
        diagnostics: Vec::new(),
    }
}

fn chunk_database() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_chunks (
                 repository_id TEXT NOT NULL, source_scope TEXT NOT NULL,
                 chunk_id TEXT NOT NULL, file_id TEXT NOT NULL, path TEXT NOT NULL,
                 language_id TEXT NOT NULL, content TEXT NOT NULL,
                 byte_start INTEGER NOT NULL, byte_end INTEGER NOT NULL,
                 line_start INTEGER NOT NULL, line_end INTEGER NOT NULL,
                 symbol_snapshot_id TEXT,
                 PRIMARY KEY (source_scope, chunk_id)
             );
             CREATE VIRTUAL TABLE code_repository_search USING fts5(
                 source_scope UNINDEXED, document_kind UNINDEXED, record_id UNINDEXED,
                 path UNINDEXED, language_id UNINDEXED, content
             );
             CREATE TABLE code_repository_search_metadata (
                 source_scope TEXT NOT NULL, document_kind TEXT NOT NULL,
                 record_id TEXT NOT NULL, path TEXT NOT NULL,
                 search_rowid INTEGER PRIMARY KEY,
                 UNIQUE (source_scope, document_kind, record_id)
             );",
        )
        .expect("chunk schema should initialize");
    connection
}

fn ordered_ids(connection: &Connection, table: &str) -> Vec<String> {
    let id_column = if table == "code_repository_chunks" {
        "chunk_id"
    } else {
        "record_id"
    };
    let sql = format!("SELECT {id_column} FROM {table} ORDER BY rowid");
    let mut statement = connection.prepare(&sql).expect("id query should prepare");
    statement
        .query_map([], |row| row.get(0))
        .expect("ids should query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("ids should collect")
}

async fn registered_store(source_scope: &str) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo",
                "fixture",
                "/tmp/chunk-bulk-fixture",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
        .begin_code_index_session(CodeIndexSession {
            repository_id: "repo".to_owned(),
            source_scope: source_scope.to_owned(),
            base_resolved_commit_sha: None,
            resolved_commit_sha: "commit".to_owned(),
            tree_hash: "tree".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            full_replace: true,
            total_path_count: 1,
            changed_path_count: 1,
            skipped_unchanged_count: 0,
            deleted_paths: Vec::new(),
            changed_paths: Vec::new(),
            tombstones: Vec::new(),
            workspaces: Vec::new(),
            resource_budget: CodeIndexResourceBudget::default(),
        })
        .await
        .expect("session should begin");
    store
}

async fn scope_chunk_ids(store: &SqliteGraphStore, source_scope: &str) -> Vec<String> {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            let mut statement = connection.prepare(
                "SELECT chunk_id FROM code_repository_chunks
                 WHERE source_scope = ?1 ORDER BY rowid",
            )?;
            let rows = statement.query_map([source_scope], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(StorageError::from)
        })
        .await
        .expect("chunk ids should load")
}

async fn scope_counts(store: &SqliteGraphStore, source_scope: &str) -> (usize, usize, usize) {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT
                         (SELECT COUNT(*) FROM code_repository_chunks WHERE source_scope = ?1),
                         (SELECT COUNT(*) FROM code_repository_search
                          WHERE source_scope = ?1 AND document_kind = 'chunk'),
                         (SELECT COUNT(*) FROM code_repository_search_metadata
                          WHERE source_scope = ?1 AND document_kind = 'chunk')",
                    [source_scope],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("scope counts should load")
}

async fn batch_staging_state(store: &SqliteGraphStore, source_scope: &str) -> String {
    let source_scope = source_scope.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT state FROM code_repository_index_batch_staging
                     WHERE source_scope = ?1 AND batch_index = 1",
                    params![source_scope],
                    |row| row.get(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("staging state should load")
}
