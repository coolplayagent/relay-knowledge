use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::storage::{SqliteStorageDiagnostics, StorageError};

pub(super) const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const READ_SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(50);
const SQLITE_CACHE_SIZE_KIB: i64 = -64_000;
const SQLITE_MMAP_SIZE_BYTES: i64 = 268_435_456;
const MAINTENANCE_DIAGNOSTICS_ID: i64 = 1;

#[derive(Debug, Clone, Default)]
pub(super) struct SqliteMaintenanceState {
    last_maintenance_at_ms: Option<u64>,
    last_maintenance_error: Option<String>,
}

pub(in crate::storage) fn configure_connection(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
    configure_common_pragmas(connection)
}

pub(super) fn configure_writer_connection(connection: &Connection) -> Result<(), StorageError> {
    configure_connection(connection)?;
    let _journal_mode = connection.query_row("PRAGMA journal_mode = WAL", [], |row| {
        row.get::<_, String>(0)
    })?;

    Ok(())
}

pub(super) fn configure_read_connection(connection: &Connection) -> Result<(), StorageError> {
    connection.busy_timeout(READ_SQLITE_BUSY_TIMEOUT)?;
    configure_common_pragmas(connection)?;
    connection.execute_batch("PRAGMA query_only = ON;")?;

    Ok(())
}

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS relay_sqlite_maintenance_diagnostics (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            last_maintenance_at_ms INTEGER,
            last_maintenance_error TEXT
        );
        ",
    )?;

    Ok(())
}

pub(super) fn run_post_index_maintenance(
    connection: &Connection,
    state: &Arc<Mutex<SqliteMaintenanceState>>,
) {
    let attempted_at_ms = current_time_millis();
    let maintenance_error = run_post_index_maintenance_once(connection)
        .err()
        .map(|error| error.to_string());
    let recorded_error =
        match persist_maintenance_result(connection, attempted_at_ms, maintenance_error.as_deref())
        {
            Ok(()) => maintenance_error,
            Err(error) => Some(match maintenance_error {
                Some(maintenance_error) => {
                    format!(
                        "{maintenance_error}; failed to persist maintenance diagnostics: {error}"
                    )
                }
                None => format!("failed to persist maintenance diagnostics: {error}"),
            }),
        };
    record_post_index_maintenance_result(state, attempted_at_ms, recorded_error);
}

pub(super) fn diagnostics(
    connection: &Connection,
    database_path: Option<&Path>,
    state: &Arc<Mutex<SqliteMaintenanceState>>,
) -> Result<SqliteStorageDiagnostics, StorageError> {
    let mut diagnostics = connection_diagnostics(connection, database_path)?;
    let state_diagnostics = state_diagnostics(state);
    if diagnostics.last_maintenance_at_ms.is_none() && diagnostics.last_maintenance_error.is_none()
    {
        diagnostics.last_maintenance_at_ms = state_diagnostics.last_maintenance_at_ms;
        diagnostics.last_maintenance_error = state_diagnostics.last_maintenance_error;
    } else if let Some(lock_error) = state_diagnostics
        .last_maintenance_error
        .filter(|error| error == "sqlite maintenance state lock was poisoned")
    {
        diagnostics.last_maintenance_error =
            append_error(diagnostics.last_maintenance_error, lock_error);
    }

    Ok(diagnostics)
}

pub(in crate::storage) fn read_only_database_diagnostics(
    database_path: &Path,
) -> Result<SqliteStorageDiagnostics, StorageError> {
    let connection = Connection::open_with_flags(
        database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure_read_connection(&connection)?;
    connection_diagnostics(&connection, Some(database_path))
}

fn connection_diagnostics(
    connection: &Connection,
    database_path: Option<&Path>,
) -> Result<SqliteStorageDiagnostics, StorageError> {
    let journal_mode =
        connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?;
    let persisted = persisted_maintenance_result(connection)?;

    Ok(SqliteStorageDiagnostics {
        journal_mode,
        wal_size_bytes: database_path.and_then(wal_size_bytes),
        last_maintenance_at_ms: persisted.last_maintenance_at_ms,
        last_maintenance_error: persisted.last_maintenance_error,
    })
}

fn configure_common_pragmas(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(&format!(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA synchronous = NORMAL;
        PRAGMA cache_size = {SQLITE_CACHE_SIZE_KIB};
        PRAGMA temp_store = MEMORY;
        PRAGMA mmap_size = {SQLITE_MMAP_SIZE_BYTES};
        "
    ))?;

    Ok(())
}

fn run_post_index_maintenance_once(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch("PRAGMA optimize;")?;
    let checkpoint = connection.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
        Ok(WalCheckpointResult {
            busy: row.get(0)?,
            log_frames: row.get(1)?,
            checkpointed_frames: row.get(2)?,
        })
    })?;
    if checkpoint.incomplete() {
        return Err(StorageError::InvalidInput(format!(
            "sqlite WAL checkpoint incomplete: busy={}, log_frames={}, checkpointed_frames={}",
            checkpoint.busy, checkpoint.log_frames, checkpoint.checkpointed_frames
        )));
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct WalCheckpointResult {
    busy: i64,
    log_frames: i64,
    checkpointed_frames: i64,
}

impl WalCheckpointResult {
    fn incomplete(self) -> bool {
        self.log_frames >= 0
            && self.checkpointed_frames >= 0
            && (self.busy != 0 || self.checkpointed_frames < self.log_frames)
    }
}

fn persisted_maintenance_result(
    connection: &Connection,
) -> Result<SqliteMaintenanceState, StorageError> {
    if !maintenance_table_exists(connection)? {
        return Ok(SqliteMaintenanceState::default());
    }
    connection
        .query_row(
            "
            SELECT last_maintenance_at_ms, last_maintenance_error
            FROM relay_sqlite_maintenance_diagnostics
            WHERE id = ?1
            ",
            params![MAINTENANCE_DIAGNOSTICS_ID],
            |row| {
                Ok(SqliteMaintenanceState {
                    last_maintenance_at_ms: row.get::<_, Option<u64>>(0)?,
                    last_maintenance_error: row.get::<_, Option<String>>(1)?,
                })
            },
        )
        .optional()
        .map(|row| row.unwrap_or_default())
        .map_err(StorageError::from)
}

fn persist_maintenance_result(
    connection: &Connection,
    attempted_at_ms: u64,
    maintenance_error: Option<&str>,
) -> Result<(), StorageError> {
    initialize_schema(connection)?;
    connection.execute(
        "
        INSERT INTO relay_sqlite_maintenance_diagnostics (
            id, last_maintenance_at_ms, last_maintenance_error
        )
        VALUES (?1, ?2, ?3)
        ON CONFLICT(id) DO UPDATE SET
            last_maintenance_at_ms = excluded.last_maintenance_at_ms,
            last_maintenance_error = excluded.last_maintenance_error
        ",
        params![
            MAINTENANCE_DIAGNOSTICS_ID,
            attempted_at_ms,
            maintenance_error
        ],
    )?;
    Ok(())
}

fn maintenance_table_exists(connection: &Connection) -> Result<bool, StorageError> {
    connection
        .query_row(
            "
            SELECT EXISTS (
                SELECT 1
                FROM sqlite_master
                WHERE type = 'table'
                  AND name = 'relay_sqlite_maintenance_diagnostics'
            )
            ",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

fn state_diagnostics(state: &Arc<Mutex<SqliteMaintenanceState>>) -> SqliteMaintenanceState {
    match state.lock() {
        Ok(state) => state.clone(),
        Err(_) => SqliteMaintenanceState {
            last_maintenance_at_ms: None,
            last_maintenance_error: Some("sqlite maintenance state lock was poisoned".to_owned()),
        },
    }
}

fn record_post_index_maintenance_result(
    state: &Arc<Mutex<SqliteMaintenanceState>>,
    attempted_at_ms: u64,
    maintenance_error: Option<String>,
) {
    if let Ok(mut state) = state.lock() {
        state.last_maintenance_at_ms = Some(attempted_at_ms);
        state.last_maintenance_error = maintenance_error;
    }
}

fn append_error(existing: Option<String>, error: String) -> Option<String> {
    Some(match existing {
        Some(existing) => format!("{existing}; {error}"),
        None => error,
    })
}

fn wal_size_bytes(database_path: &Path) -> Option<u64> {
    let wal_path = wal_path(database_path);
    match std::fs::metadata(wal_path) {
        Ok(metadata) => Some(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(0),
        Err(_) => None,
    }
}

fn wal_path(database_path: &Path) -> PathBuf {
    let mut path = database_path.as_os_str().to_owned();
    path.push("-wal");
    PathBuf::from(path)
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            CodeIndexResourceBudget, CodeIndexSession, CodeIndexSnapshot,
            CodeRepositoryRegistration,
        },
        storage::{CodeRepositoryStore, GraphStore},
    };
    use rusqlite::OpenFlags;

    use super::super::SqliteGraphStore;

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
        assert_eq!(pragmas.5, 5_000);
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

    async fn registered_file_store(label: &str) -> (SqliteGraphStore, PathBuf) {
        let path = unique_database_path(label);
        let store = SqliteGraphStore::open(&path).expect("store should open");
        store
            .upsert_code_repository(
                CodeRepositoryRegistration::new(
                    "repo",
                    "fixture",
                    "/tmp/repo",
                    Vec::new(),
                    Vec::new(),
                )
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
}
