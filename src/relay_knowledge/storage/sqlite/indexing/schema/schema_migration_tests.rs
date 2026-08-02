use super::super::super::*;
use crate::storage::{GraphStore, IndexRefreshClaimRequest, IndexRefreshQueueRequest, IndexStore};

#[tokio::test]
async fn startup_migrates_obsolete_refresh_queue_schema_without_deleting_graph_data() {
    let path = temp_db_path("obsolete-refresh-queue");
    let connection = rusqlite::Connection::open(&path).expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE graph_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                graph_version INTEGER NOT NULL
            );
            INSERT INTO graph_state (id, graph_version) VALUES (1, 1);
            CREATE TABLE evidence (
                id TEXT PRIMARY KEY,
                source_scope TEXT NOT NULL,
                content TEXT NOT NULL,
                created_graph_version INTEGER NOT NULL
            );
            INSERT INTO evidence (id, source_scope, content, created_graph_version)
            VALUES ('ev-legacy', 'docs', 'Legacy queue data should survive startup', 1);
            CREATE TABLE graph_mutations (
                graph_version INTEGER PRIMARY KEY,
                evidence_count INTEGER NOT NULL,
                entity_count INTEGER NOT NULL
            );
            INSERT INTO graph_mutations (graph_version, evidence_count, entity_count)
            VALUES (1, 1, 0);
            CREATE TABLE index_refresh_tasks (
                task_id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                modality TEXT NOT NULL,
                target_graph_version INTEGER NOT NULL,
                state TEXT NOT NULL,
                attempt_count INTEGER NOT NULL,
                next_retry_after_ms INTEGER NOT NULL,
                input_fingerprint TEXT NOT NULL,
                cursor_before INTEGER NOT NULL,
                cursor_after INTEGER,
                last_error_kind TEXT,
                last_error_message TEXT
            );
            INSERT INTO index_refresh_tasks (
                task_id, kind, source_scope, modality, target_graph_version, state,
                attempt_count, next_retry_after_ms, input_fingerprint, cursor_before,
                cursor_after, last_error_kind, last_error_message
            )
            VALUES (
                'bm25:graph:text', 'bm25', 'graph', 'text', 1, 'queued',
                0, 0, 'fingerprint', 0, NULL, NULL, NULL
            );
            ",
        )
        .expect("obsolete schema should be created");
    drop(connection);

    let store = SqliteGraphStore::open(&path).expect("store should migrate obsolete database");
    let graph = store.inspect_graph().await.expect("graph should inspect");
    assert_eq!(graph.graph_version, GraphVersion::new(1));
    assert_eq!(graph.evidence_count, 1);

    let claimed = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-legacy".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 10,
        })
        .await
        .expect("legacy task should be claimable")
        .expect("legacy task should exist");
    assert_eq!(claimed.task_id, "bm25:graph:text");
    assert_eq!(claimed.lease_owner.as_deref(), Some("worker-legacy"));
    store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Semantic],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 4,
            reset_dead_letter_tasks: false,
            now_ms: 20,
        })
        .await
        .expect("new refresh task should insert after legacy table rebuild");

    let guard = store.connection.lock().expect("connection should lock");
    let task_columns = guard
        .prepare("PRAGMA table_info(index_refresh_tasks)")
        .expect("table info should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("columns should read")
        .collect::<Result<Vec<_>, _>>()
        .expect("columns should collect");

    assert!(task_columns.iter().any(|column| column == "lease_owner"));
    assert!(task_columns.iter().any(|column| column == "created_at_ms"));
    assert!(
        !task_columns
            .iter()
            .any(|column| column == "next_retry_after_ms")
    );
    assert_eq!(
        guard
            .query_row("SELECT COUNT(*) FROM index_refresh_tasks", [], |row| {
                row.get::<_, u64>(0)
            })
            .expect("task count should read"),
        2
    );
    let mutation_columns = guard
        .prepare("PRAGMA table_info(graph_mutations)")
        .expect("mutation table info should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("mutation columns should read")
        .collect::<Result<Vec<_>, _>>()
        .expect("mutation columns should collect");
    assert!(
        mutation_columns
            .iter()
            .any(|column| column == "affected_scopes_json")
    );
    drop(guard);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn startup_prefers_legacy_retry_after_when_mixed_retry_columns_exist() {
    let path = mixed_retry_columns_db("mixed-refresh-retry-columns", 0, 60000);

    let store = SqliteGraphStore::open(&path).expect("store should migrate mixed retry columns");
    let early_claim = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-early".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 10,
        })
        .await
        .expect("claim before retry time should load");
    assert_eq!(early_claim, None);

    let (next_retry_at_ms, task_columns) = migrated_retry_state(&store);
    assert_eq!(next_retry_at_ms, 60000);
    assert!(
        !task_columns
            .iter()
            .any(|column| column == "next_retry_after_ms")
    );

    let ready_claim = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-ready".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 60000,
        })
        .await
        .expect("claim at retry time should load")
        .expect("retrying task should become claimable");
    assert_eq!(ready_claim.task_id, "bm25:docs:text");
    assert_eq!(ready_claim.next_retry_at_ms, 60000);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn startup_preserves_updated_retry_at_when_legacy_retry_after_is_stale() {
    let path = mixed_retry_columns_db("mixed-refresh-current-retry-column", 120000, 60000);

    let store = SqliteGraphStore::open(&path).expect("store should migrate mixed retry columns");
    let stale_legacy_claim = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-stale".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 60000,
        })
        .await
        .expect("claim at stale legacy retry time should load");
    assert_eq!(stale_legacy_claim, None);

    let (next_retry_at_ms, task_columns) = migrated_retry_state(&store);
    assert_eq!(next_retry_at_ms, 120000);
    assert!(
        !task_columns
            .iter()
            .any(|column| column == "next_retry_after_ms")
    );

    let ready_claim = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker-current".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 120000,
        })
        .await
        .expect("claim at current retry time should load")
        .expect("retrying task should become claimable");
    assert_eq!(ready_claim.task_id, "bm25:docs:text");
    assert_eq!(ready_claim.next_retry_at_ms, 120000);

    let _ = std::fs::remove_file(path);
}

fn mixed_retry_columns_db(
    test_name: &str,
    next_retry_at_ms: u64,
    next_retry_after_ms: u64,
) -> std::path::PathBuf {
    let path = temp_db_path(test_name);
    let connection = rusqlite::Connection::open(&path).expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE graph_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                graph_version INTEGER NOT NULL
            );
            INSERT INTO graph_state (id, graph_version) VALUES (1, 1);
            CREATE TABLE evidence (
                id TEXT PRIMARY KEY,
                source_scope TEXT NOT NULL,
                content TEXT NOT NULL,
                created_graph_version INTEGER NOT NULL
            );
            INSERT INTO evidence (id, source_scope, content, created_graph_version)
            VALUES ('ev-mixed-retry', 'docs', 'Mixed retry column migration', 1);
            CREATE TABLE graph_mutations (
                graph_version INTEGER PRIMARY KEY,
                evidence_count INTEGER NOT NULL,
                entity_count INTEGER NOT NULL
            );
            INSERT INTO graph_mutations (graph_version, evidence_count, entity_count)
            VALUES (1, 1, 0);
            CREATE TABLE index_refresh_tasks (
                task_id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                modality TEXT NOT NULL,
                target_graph_version INTEGER NOT NULL,
                state TEXT NOT NULL,
                attempt_count INTEGER NOT NULL,
                next_retry_at_ms INTEGER NOT NULL DEFAULT 0,
                next_retry_after_ms INTEGER NOT NULL,
                input_fingerprint TEXT NOT NULL,
                cursor_before INTEGER NOT NULL,
                cursor_after INTEGER,
                last_error_kind TEXT,
                last_error_message TEXT
            );
            ",
        )
        .expect("mixed schema should be created");
    connection
        .execute(
            "
            INSERT INTO index_refresh_tasks (
                task_id, kind, source_scope, modality, target_graph_version, state,
                attempt_count, next_retry_at_ms, next_retry_after_ms,
                input_fingerprint, cursor_before, cursor_after, last_error_kind,
                last_error_message
            )
            VALUES (
                'bm25:docs:text', 'bm25', 'docs', 'text', 1, 'retrying',
                1, ?1, ?2, 'mixed-fingerprint', 0, NULL, 'indexer',
                'retry later'
            )
            ",
            rusqlite::params![next_retry_at_ms, next_retry_after_ms],
        )
        .expect("mixed task should be inserted");
    drop(connection);

    path
}

fn migrated_retry_state(store: &SqliteGraphStore) -> (u64, Vec<String>) {
    let guard = store.connection.lock().expect("connection should lock");
    let next_retry_at_ms = guard
        .query_row(
            "SELECT next_retry_at_ms FROM index_refresh_tasks WHERE task_id = 'bm25:docs:text'",
            [],
            |row| row.get::<_, u64>(0),
        )
        .expect("retry timestamp should read");
    let task_columns = guard
        .prepare("PRAGMA table_info(index_refresh_tasks)")
        .expect("table info should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("columns should read")
        .collect::<Result<Vec<_>, _>>()
        .expect("columns should collect");

    (next_retry_at_ms, task_columns)
}

fn temp_db_path(test_name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be monotonic")
        .as_nanos();
    path.push(format!(
        "relay-knowledge-{test_name}-{}-{unique}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);

    path
}
