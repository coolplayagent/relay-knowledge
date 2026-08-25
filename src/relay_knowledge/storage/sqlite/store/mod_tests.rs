//! Direct contracts for SQLite store execution and connection contention.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;

use crate::storage::{FileSearchRequest, GraphStore, IndexStore, StorageError};

use super::SqliteGraphStore;

#[tokio::test]
async fn code_index_task_fresh_and_marker_current_open_do_not_create_query_indexes() {
    let path = unique_database_path();
    let store = SqliteGraphStore::open(&path).expect("store should open");
    assert!(!query_index_exists(&store, "code_repository_symbols_lookup").await);
    assert!(!query_index_exists(&store, "code_repository_symbols_name_path_lookup").await);
    drop(store);

    let store = SqliteGraphStore::open(&path).expect("marker-current store should reopen");
    assert!(!query_index_exists(&store, "code_repository_symbols_lookup").await);
    assert!(!query_index_exists(&store, "code_repository_symbols_name_path_lookup").await);
    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}

#[tokio::test]
async fn code_index_task_reopen_retains_an_exact_retired_symbol_lookup() {
    let path = unique_database_path();
    let store = SqliteGraphStore::open(&path).expect("store should open");
    store
        .run(|connection| {
            connection.execute(
                "CREATE INDEX code_repository_symbols_lookup
                 ON code_repository_symbols(source_scope, name, qualified_name, path)",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("exact legacy index should install");
    drop(store);

    let store = SqliteGraphStore::open(&path).expect("exact retired shape should reopen");
    assert!(query_index_exists(&store, "code_repository_symbols_lookup").await);
    drop(store);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}

async fn query_index_exists(store: &SqliteGraphStore, name: &str) -> bool {
    let name = name.to_owned();
    store
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'index' AND name = ?1)",
                    [name],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("query-index state should load")
}

#[tokio::test(flavor = "current_thread")]
async fn in_memory_health_snapshot_reports_busy_when_write_connection_is_held() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let lock = hold_write_connection(&store);

    let error = store
        .health_snapshot(0)
        .await
        .expect_err("health should not wait behind an occupied write connection");

    assert!(matches!(error, StorageError::Busy(message) if message.contains("occupied")));
    lock.release();
}

#[tokio::test(flavor = "current_thread")]
async fn file_backed_health_snapshot_uses_read_pool_while_writer_mutex_is_held() {
    let path = unique_database_path();
    let store = SqliteGraphStore::open(&path).expect("store should open");
    let lock = hold_write_connection(&store);

    let snapshot = store
        .health_snapshot(0)
        .await
        .expect("read pool should serve health without the writer mutex");

    assert_eq!(snapshot.graph.graph_version.get(), 0);
    assert_eq!(snapshot.repository_code_totals.repository_count, 0);
    lock.release();
    let _ = std::fs::remove_file(path);
}

#[tokio::test(flavor = "current_thread")]
async fn file_query_timeout_includes_read_pool_wait() {
    let path = unique_database_path();
    let store = SqliteGraphStore::open(&path).expect("store should open");
    let locks = hold_read_connections(&store);
    let started = Instant::now();

    let error = store
        .search_files(FileSearchRequest {
            query: "anything".to_owned(),
            source_scope: None,
            root_id: None,
            limit: 5,
            timeout_ms: 20,
        })
        .await
        .expect_err("query should time out before acquiring a read connection");

    assert!(
        matches!(error, StorageError::InvalidInput(message) if message.contains("file query timed out"))
    );
    assert!(started.elapsed() < Duration::from_millis(500));
    locks.release();
    let _ = std::fs::remove_file(path);
}

#[tokio::test(flavor = "current_thread")]
async fn file_backed_read_uses_idle_pool_lane_when_first_lane_is_busy() {
    let path = unique_database_path();
    let store = SqliteGraphStore::open(&path).expect("store should open");
    let first_connection = store
        .read_pool
        .as_ref()
        .expect("file-backed store should have read pool")
        .connections()
        .into_iter()
        .next()
        .expect("read pool should have a connection");
    let lock = hold_read_connection(first_connection);

    let version = tokio::time::timeout(Duration::from_millis(200), store.current_graph_version())
        .await
        .expect("read should use an idle lane instead of waiting behind the busy first lane")
        .expect("graph version should be readable");

    assert_eq!(version.get(), 0);
    lock.release();
    let _ = std::fs::remove_file(path);
}

struct HeldWriteConnection {
    release: std::sync::mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

impl HeldWriteConnection {
    fn release(self) {
        let _ = self.release.send(());
        self.thread.join().expect("lock thread should finish");
    }
}

fn hold_write_connection(store: &SqliteGraphStore) -> HeldWriteConnection {
    let store = store.clone();
    let (locked_sender, locked_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let _guard = store.connection.lock().expect("write connection lock");
        locked_sender.send(()).expect("lock notice should send");
        release_receiver
            .recv()
            .expect("release notice should arrive");
    });
    locked_receiver.recv().expect("lock notice should arrive");

    HeldWriteConnection {
        release: release_sender,
        thread,
    }
}

struct HeldReadConnections {
    releases: Vec<std::sync::mpsc::Sender<()>>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl HeldReadConnections {
    fn release(self) {
        for release in self.releases {
            let _ = release.send(());
        }
        for thread in self.threads {
            thread.join().expect("lock thread should finish");
        }
    }
}

fn hold_read_connections(store: &SqliteGraphStore) -> HeldReadConnections {
    let connections = store
        .read_pool
        .as_ref()
        .expect("file-backed store should have read pool")
        .connections();
    let mut releases = Vec::new();
    let mut threads = Vec::new();
    for connection in connections {
        let (locked_sender, locked_receiver) = std::sync::mpsc::channel();
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _guard = connection.lock().expect("read connection lock");
            locked_sender.send(()).expect("lock notice should send");
            release_receiver
                .recv()
                .expect("release notice should arrive");
        });
        locked_receiver.recv().expect("lock notice should arrive");
        releases.push(release_sender);
        threads.push(thread);
    }

    HeldReadConnections { releases, threads }
}

fn hold_read_connection(connection: Arc<Mutex<Connection>>) -> HeldReadConnections {
    let (locked_sender, locked_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let _guard = connection.lock().expect("read connection lock");
        locked_sender.send(()).expect("lock notice should send");
        release_receiver
            .recv()
            .expect("release notice should arrive");
    });
    locked_receiver.recv().expect("lock notice should arrive");

    HeldReadConnections {
        releases: vec![release_sender],
        threads: vec![thread],
    }
}

fn unique_database_path() -> std::path::PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be monotonic")
        .as_nanos();
    std::env::temp_dir()
        .join("relay-knowledge-tests")
        .join(format!(
            "health-read-pool-{}-{suffix}.sqlite",
            std::process::id()
        ))
}
