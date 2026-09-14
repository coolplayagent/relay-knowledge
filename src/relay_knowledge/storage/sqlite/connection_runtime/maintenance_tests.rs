use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexSession,
        CodeIndexSnapshot, CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexPublicationStore as _, CodeIndexTaskClaimRequest, CodeIndexTaskCompletion,
        CodeIndexTaskSeed, CodeIndexTaskStore as _, GraphStore, RepositoryCatalogStore as _,
        SoftwareProjectionStore as _, SqliteGraphStore,
    },
};
use rusqlite::OpenFlags;

#[test]
fn maintenance_failure_is_recorded_without_returning_error() {
    let state = Arc::new(Mutex::new(SqliteMaintenanceState::default()));

    record_post_index_maintenance_result(&state, 123, Some("maintenance failed".to_owned()));

    let state = state.lock().expect("maintenance state should lock");
    assert_eq!(state.last_maintenance_at_ms, Some(123));
    assert!(
        state
            .last_maintenance_error
            .as_deref()
            .is_some_and(|message| message.contains("maintenance failed"))
    );
}

#[test]
fn missing_wal_file_reports_zero_bytes_for_file_database() {
    let path = std::env::temp_dir().join("relay-knowledge-missing-wal-test.sqlite");
    let _ = std::fs::remove_file(wal_path(&path));

    assert_eq!(wal_size_bytes(&path), Some(0));
}

#[test]
fn shared_connection_config_does_not_force_wal_on_read_only_connections() {
    let path = unique_database_path("readonly-config");
    {
        let connection = Connection::open(&path).expect("writer connection should open");
        configure_writer_connection(&connection).expect("writer pragmas should apply");
        connection
            .execute("CREATE TABLE catalog_probe (id INTEGER PRIMARY KEY)", [])
            .expect("probe table should create");
    }
    let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("read-only connection should open");

    configure_connection(&connection).expect("read-only pragmas should not require writes");
    cleanup_database_path(&path);
}

#[test]
fn read_only_database_diagnostics_does_not_require_graph_schema() {
    let path = unique_database_path("readonly-diagnostics");
    {
        let connection = Connection::open(&path).expect("writer connection should open");
        configure_writer_connection(&connection).expect("writer pragmas should apply");
        initialize_schema(&connection).expect("maintenance schema should initialize");
        persist_maintenance_result(&connection, 456, Some("checkpoint busy"))
            .expect("maintenance diagnostics should persist");
    }

    let diagnostics =
        read_only_database_diagnostics(&path).expect("read-only diagnostics should load");

    assert_eq!(diagnostics.journal_mode, "wal");
    assert_eq!(diagnostics.last_maintenance_at_ms, Some(456));
    assert_eq!(
        diagnostics.last_maintenance_error.as_deref(),
        Some("checkpoint busy")
    );
    cleanup_database_path(&path);
}

#[test]
fn wal_checkpoint_incomplete_result_is_recorded_as_maintenance_error() {
    let path = unique_database_path("checkpoint-busy");
    let writer = Connection::open(&path).expect("writer connection should open");
    configure_writer_connection(&writer).expect("writer pragmas should apply");
    writer
        .execute_batch(
            "
                CREATE TABLE checkpoint_probe (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO checkpoint_probe (value) VALUES ('before-reader');
                ",
        )
        .expect("probe rows should create");
    let reader = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("reader connection should open");
    configure_read_connection(&reader).expect("reader pragmas should apply");
    reader
        .execute_batch("BEGIN;")
        .expect("reader transaction should begin");
    let _: i64 = reader
        .query_row("SELECT COUNT(*) FROM checkpoint_probe", [], |row| {
            row.get(0)
        })
        .expect("reader snapshot should be established");
    writer
        .execute(
            "INSERT INTO checkpoint_probe (value) VALUES ('after-reader')",
            [],
        )
        .expect("writer should append WAL frames");

    let error = run_post_index_maintenance_once(&writer)
        .expect_err("pinned reader should block complete checkpoint");

    assert!(error.to_string().contains("WAL checkpoint incomplete"));
    reader
        .execute_batch("ROLLBACK;")
        .expect("reader transaction should close");
    cleanup_database_path(&path);
}

#[tokio::test]
async fn file_backed_connection_applies_large_repository_pragmas() {
    let path = unique_database_path("pragma");
    let store = SqliteGraphStore::open(&path).expect("store should open");

    let pragmas = store
        .run(|connection| {
            Ok((
                query_string(connection, "PRAGMA journal_mode")?,
                query_i64(connection, "PRAGMA synchronous")?,
                query_i64(connection, "PRAGMA cache_size")?,
                query_i64(connection, "PRAGMA temp_store")?,
                query_i64(connection, "PRAGMA mmap_size")?,
                query_i64(connection, "PRAGMA page_size")?,
                query_i64(connection, "PRAGMA wal_autocheckpoint")?,
                query_i64(connection, "PRAGMA busy_timeout")?,
            ))
        })
        .await
        .expect("pragmas should be readable");

    assert_eq!(pragmas.0, "wal");
    assert_eq!(pragmas.1, 1);
    assert_eq!(pragmas.2, SQLITE_CACHE_SIZE_KIB);
    assert_eq!(pragmas.3, 2);
    assert_eq!(pragmas.4, SQLITE_MMAP_SIZE_BYTES);
    assert!(pragmas.5 >= 512);
    assert_eq!(pragmas.6, SQLITE_WAL_AUTOCHECKPOINT_BYTES / pragmas.5);
    assert_eq!(pragmas.7, 5_000);
    let read_busy_timeout = store
        .run_read(|connection| query_i64(connection, "PRAGMA busy_timeout"))
        .await
        .expect("read busy timeout should be readable");
    assert_eq!(
        read_busy_timeout,
        i64::try_from(READ_SQLITE_BUSY_TIMEOUT.as_millis()).expect("timeout should fit")
    );
    cleanup_database_path(&path);
}

#[tokio::test]
async fn snapshot_apply_records_post_index_maintenance_diagnostics() {
    let (store, path) = registered_file_store("snapshot").await;

    store
        .apply_code_index_snapshot(empty_snapshot("git_snapshot:maintenance-snapshot"))
        .await
        .expect("snapshot should apply");

    let graph = store
        .inspect_graph()
        .await
        .expect("graph diagnostics should load");
    assert_eq!(graph.sqlite.journal_mode, "wal");
    assert!(graph.sqlite.wal_size_bytes.is_some());
    assert!(graph.sqlite.last_maintenance_at_ms.is_some());
    assert_eq!(graph.sqlite.last_maintenance_error, None);
    let attempted_at_ms = graph.sqlite.last_maintenance_at_ms;
    drop(store);
    let reopened = SqliteGraphStore::open(&path).expect("store should reopen");
    let reopened_graph = reopened
        .inspect_graph()
        .await
        .expect("reopened graph diagnostics should load");
    assert_eq!(
        reopened_graph.sqlite.last_maintenance_at_ms,
        attempted_at_ms
    );
    assert_eq!(reopened_graph.sqlite.last_maintenance_error, None);
    cleanup_database_path(&path);
}

#[tokio::test]
async fn finalized_code_index_session_records_post_index_maintenance_diagnostics() {
    let (store, path) = registered_file_store("finalize").await;
    let session = empty_session("git_snapshot:maintenance-finalize");

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let graph = store
        .inspect_graph()
        .await
        .expect("graph diagnostics should load");
    assert_eq!(graph.sqlite.journal_mode, "wal");
    assert!(graph.sqlite.last_maintenance_at_ms.is_some());
    assert_eq!(graph.sqlite.last_maintenance_error, None);
    cleanup_database_path(&path);
}

#[tokio::test]
async fn code_index_task_fenced_finalization_defers_maintenance_until_terminal_completion() {
    let (store, path) = registered_file_store("fenced-finalize").await;
    let source_scope = "git_snapshot:maintenance-fenced-finalize";
    let session = empty_session(source_scope);
    let observed_now_ms = epoch_millis();
    let queued = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: "repo".to_owned(),
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: session.resolved_commit_sha.clone(),
            tree_hash: session.tree_hash.clone(),
            source_scope: source_scope.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::Full,
            input_fingerprint: "maintenance-fenced-finalize".to_owned(),
            resource_budget: session.resource_budget,
            payload_json: "{}".to_owned(),
            now_ms: observed_now_ms,
        })
        .await
        .expect("task should queue");
    let claimed = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "maintenance-worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms: epoch_millis(),
        })
        .await
        .expect("task claim should run")
        .expect("task should claim");
    let fence = CodeIndexPublicationFence {
        repository_id: claimed.repository_id.clone(),
        task_id: claimed.task_id.clone(),
        lease_owner: "maintenance-worker".to_owned(),
        attempt_count: claimed.attempt_count,
        generation: claimed.publication_generation,
    };

    store
        .begin_code_index_session_with_fence(session.clone(), fence.clone())
        .await
        .expect("fenced session should begin");
    store
        .finalize_code_index_session_with_fence(session, fence.clone())
        .await
        .expect("fenced session should finalize without maintenance");
    assert_eq!(
        store
            .inspect_graph()
            .await
            .expect("prepublication diagnostics should load")
            .sqlite
            .last_maintenance_at_ms,
        None
    );

    crate::storage::stage_empty_business_projection_with_fence_for_test(
        &store,
        claimed.repository_id.clone(),
        source_scope.to_owned(),
        claimed.resolved_commit_sha.clone(),
        fence.clone(),
    )
    .await
    .expect("business projection should stage before publication");
    store
        .refresh_software_global_projection_with_fence(source_scope.to_owned(), fence)
        .await
        .expect("fenced publication should complete");
    store
        .complete_code_index_task(CodeIndexTaskCompletion {
            task_id: claimed.task_id,
            lease_owner: "maintenance-worker".to_owned(),
            attempt_count: claimed.attempt_count,
            publication_generation: claimed.publication_generation,
            now_ms: epoch_millis(),
        })
        .await
        .expect("task should become terminal");
    assert_eq!(
        store
            .inspect_graph()
            .await
            .expect("terminal diagnostics should load")
            .sqlite
            .last_maintenance_at_ms,
        None,
        "terminal task transition itself must not hold the maintenance writer"
    );

    store
        .run_code_index_post_maintenance("repo".to_owned(), source_scope.to_owned())
        .await
        .expect("post-terminal maintenance should run");
    assert!(
        store
            .inspect_graph()
            .await
            .expect("maintained diagnostics should load")
            .sqlite
            .last_maintenance_at_ms
            .is_some()
    );
    cleanup_database_path(&path);
}

async fn registered_file_store(label: &str) -> (SqliteGraphStore, PathBuf) {
    let path = unique_database_path(label);
    let store = SqliteGraphStore::open(&path).expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");

    (store, path)
}

fn empty_snapshot(source_scope: &str) -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        workspaces: Vec::new(),
        full_replace: true,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: Vec::new(),
        symbols: Vec::new(),
        references: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn empty_session(source_scope: &str) -> CodeIndexSession {
    CodeIndexSession {
        repository_id: "repo".to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        workspaces: Vec::new(),
        full_replace: true,
        total_path_count: 0,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        changed_paths: Vec::new(),
        tombstones: Vec::new(),
        resource_budget: CodeIndexResourceBudget::default(),
    }
}

fn query_i64(connection: &Connection, sql: &str) -> Result<i64, StorageError> {
    connection
        .query_row(sql, [], |row| row.get::<_, i64>(0))
        .map_err(StorageError::from)
}

fn query_string(connection: &Connection, sql: &str) -> Result<String, StorageError> {
    connection
        .query_row(sql, [], |row| row.get::<_, String>(0))
        .map_err(StorageError::from)
}

fn epoch_millis() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock should follow Unix epoch")
            .as_millis(),
    )
    .expect("epoch milliseconds should fit u64")
}

fn unique_database_path(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be monotonic")
        .as_nanos();
    let path = std::env::temp_dir()
        .join("relay-knowledge-tests")
        .join(format!(
            "sqlite-maintenance-{label}-{}-{suffix}.sqlite",
            std::process::id()
        ));
    std::fs::create_dir_all(path.parent().expect("database path should have parent"))
        .expect("database parent should be created");
    path
}

fn cleanup_database_path(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(wal_path(path));
    let mut shm_path = path.as_os_str().to_owned();
    shm_path.push("-shm");
    let _ = std::fs::remove_file(PathBuf::from(shm_path));
}
