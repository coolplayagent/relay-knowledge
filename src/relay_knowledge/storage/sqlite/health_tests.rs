use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage::{GraphStore, StorageError};

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
