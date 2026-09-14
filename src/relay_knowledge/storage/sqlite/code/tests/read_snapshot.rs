use std::{
    sync::{Arc, Condvar, Mutex, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    domain::{
        CodeRepositoryRegistration, CodeRepositorySelector, CodebaseViewKind, CodebaseViewRequest,
        FreshnessPolicy,
    },
    storage::{
        CodeIndexPublicationStore as _, CodeQueryReadStore as _, RepositoryCatalogStore as _,
        SqliteGraphStore, StorageError,
    },
};

use super::{code_test_support, retarget_snapshot_to_fact_scope, snapshot_with_chunk};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_codebase_query_never_observes_partially_retired_facts() {
    let database_path = unique_database_path();
    std::fs::create_dir_all(
        database_path
            .parent()
            .expect("test database should have a parent"),
    )
    .expect("test database directory should exist");
    let store = SqliteGraphStore::open(&database_path).expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    let mut indexed = snapshot_with_chunk("repo", "src/lib.rs", "fn entrypoint() {}");
    indexed.imports.push(code_test_support::import(
        "import:dependency",
        "file",
        "src/lib.rs",
    ));
    retarget_snapshot_to_fact_scope(&mut indexed);
    let source_scope = indexed.source_scope.clone();
    store
        .apply_code_index_snapshot(indexed)
        .await
        .expect("snapshot should apply");

    let (phase_started_sender, phase_started_receiver) = mpsc::channel();
    let phase_release = Arc::new((Mutex::new(false), Condvar::new()));
    super::super::read_snapshot_test_hook::install(
        phase_started_sender,
        Arc::clone(&phase_release),
    );

    let request = CodebaseViewRequest::new(
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        CodebaseViewKind::ArchitectureLayers,
        FreshnessPolicy::AllowStale,
        10,
        Vec::new(),
    )
    .expect("view request should validate");
    let query_store = store.clone();
    let query_scope = source_scope.clone();
    let query_request = request.clone();
    let query = tokio::spawn(async move {
        query_store
            .codebase_view_snapshot(query_scope, query_request, 10)
            .await
    });
    tokio::task::spawn_blocking(move || {
        phase_started_receiver.recv_timeout(Duration::from_secs(5))
    })
    .await
    .expect("barrier wait should join")
    .expect("query should reach its first fact read after the retirement check");

    let retired_scope = source_scope.clone();
    let retirement_store = store.clone();
    let retirement = tokio::spawn(async move {
        retirement_store
            .run(move |connection| {
                let transaction = connection.transaction()?;
                let updated = transaction.execute(
                    "UPDATE code_repository_scopes SET retiring = 1 WHERE source_scope = ?1",
                    [&retired_scope],
                )?;
                if updated != 1 {
                    return Err(StorageError::InvalidInput(
                        "test scope was not marked retiring".to_owned(),
                    ));
                }
                transaction.execute(
                    "DELETE FROM code_repository_imports WHERE source_scope = ?1",
                    [&retired_scope],
                )?;
                transaction.commit()?;
                Ok(())
            })
            .await
    });
    release_import_read_barrier(&phase_release);
    retirement
        .await
        .expect("retirement task should join")
        .expect("retirement phase should commit while the reader is paused");

    let snapshot = query
        .await
        .expect("query task should join")
        .expect("in-flight query should finish from its original read snapshot");
    assert_eq!(snapshot.files.len(), 1);
    assert_eq!(snapshot.imports.len(), 1);

    let error = store
        .codebase_view_snapshot(source_scope.clone(), request, 10)
        .await
        .expect_err("a later query should reject the retiring scope");
    assert!(error.to_string().contains("is retiring"));

    drop(store);
    remove_test_database(&database_path);
}

fn release_import_read_barrier(phase_release: &Arc<(Mutex<bool>, Condvar)>) {
    let (released, signal) = &**phase_release;
    if let Ok(mut released) = released.lock() {
        *released = true;
        signal.notify_all();
    }
}

fn unique_database_path() -> std::path::PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should follow the epoch")
        .as_nanos();
    std::env::temp_dir()
        .join("relay-knowledge-tests")
        .join(format!(
            "code-read-snapshot-{}-{suffix}.sqlite",
            std::process::id()
        ))
}

fn remove_test_database(database_path: &std::path::Path) {
    let _ = std::fs::remove_file(database_path);
    let _ = std::fs::remove_file(format!("{}-wal", database_path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", database_path.display()));
}
