use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::storage::{FileSearchRequest, GraphStore, IndexStore, StorageError};

use super::SqliteGraphStore;

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
