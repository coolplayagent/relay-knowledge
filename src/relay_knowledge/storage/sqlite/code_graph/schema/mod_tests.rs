//! Direct migration invariants for the code-graph schema owner.

use crate::{domain::GraphVersion, storage::GraphStore};

#[tokio::test]
async fn startup_rebuilds_obsolete_code_tables_without_deleting_graph_data() {
    let path = temp_db_path("obsolete-code-tables");
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
            VALUES ('ev-code-legacy', 'docs', 'Code graph rebuild should not delete graph data', 1);
            CREATE TABLE code_files (
                repository_id TEXT NOT NULL,
                path TEXT NOT NULL,
                blob_hash TEXT NOT NULL,
                PRIMARY KEY (repository_id, path)
            );
            CREATE TABLE code_symbols (
                symbol_snapshot_id TEXT PRIMARY KEY,
                file_id TEXT NOT NULL,
                name TEXT NOT NULL
            );
            INSERT INTO code_files (repository_id, path, blob_hash)
            VALUES ('repo', 'src/lib.rs', 'hash');
            ",
        )
        .expect("obsolete code tables should be created");
    drop(connection);

    let store = crate::storage::SqliteGraphStore::open(&path)
        .expect("store should rebuild obsolete code tables");
    let graph = store.inspect_graph().await.expect("graph should inspect");
    let guard = store.connection.lock().expect("connection should lock");
    let columns = table_columns(&guard, "code_files").expect("columns should read");

    assert_eq!(graph.graph_version, GraphVersion::new(1));
    assert_eq!(graph.evidence_count, 1);
    assert!(columns.iter().any(|column| column == "source_scope"));
    assert!(columns.iter().any(|column| column == "content_hash"));
    assert!(!table_exists(&guard, "code_files_legacy_0").expect("table check should run"));
    assert_eq!(
        guard
            .query_row("SELECT COUNT(*) FROM code_files", [], |row| {
                row.get::<_, u64>(0)
            })
            .expect("code file count should read"),
        0
    );
    drop(guard);
    let _ = std::fs::remove_file(path);
}

fn table_columns(
    connection: &rusqlite::Connection,
    table: &str,
) -> Result<Vec<String>, crate::storage::StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(crate::storage::StorageError::from)
}

fn table_exists(
    connection: &rusqlite::Connection,
    table: &str,
) -> Result<bool, crate::storage::StorageError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(crate::storage::StorageError::from)
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
