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
#[path = "maintenance_tests.rs"]
mod tests;
