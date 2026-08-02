mod implementations;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, TryLockError},
    time::Instant,
};

use rusqlite::Connection;

use crate::storage::{StorageError, StorageFuture};

use super::{
    code,
    connection_runtime::{
        maintenance::{SqliteMaintenanceState, configure_writer_connection},
        read_pool::{
            ReadConnectionPool, lock_any_read_connection, lock_any_read_connection_until,
            lock_connection_until, try_lock_any_read_connection,
        },
    },
    schema::{initialization, marker, migration},
};

/// SQLite implementation of graph facts, mutation log, and index metadata.
#[derive(Debug, Clone)]
pub struct SqliteGraphStore {
    pub(super) connection: Arc<Mutex<Connection>>,
    pub(super) read_pool: Option<Arc<ReadConnectionPool>>,
    pub(super) database_path: Option<PathBuf>,
    pub(super) maintenance: Arc<Mutex<SqliteMaintenanceState>>,
}

impl SqliteGraphStore {
    /// Opens a SQLite database and initializes the current schema.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(&path)?;
        configure_writer_connection(&connection)?;
        if !marker::schema_initialization_is_current(&connection)? {
            migration::prepare_existing_database(&connection)?;
            initialization::initialize_schema(&connection)?;
            marker::mark_schema_initialization_current(&connection)?;
        }
        let read_pool = ReadConnectionPool::open(&path)?;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            read_pool: Some(Arc::new(read_pool)),
            database_path: Some(path),
            maintenance: Arc::new(Mutex::new(SqliteMaintenanceState::default())),
        })
    }

    /// Opens an in-memory database for isolated tests.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let connection = Connection::open_in_memory()?;
        configure_writer_connection(&connection)?;
        initialization::initialize_schema(&connection)?;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            read_pool: None,
            database_path: None,
            maintenance: Arc::new(Mutex::new(SqliteMaintenanceState::default())),
        })
    }

    pub(in crate::storage) fn run<T, F>(&self, operation: F) -> StorageFuture<'_, T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StorageError> + Send + 'static,
    {
        let connection = Arc::clone(&self.connection);

        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut guard = connection.lock().map_err(|_| StorageError::LockPoisoned)?;

                operation(&mut guard)
            })
            .await?
        })
    }

    pub(super) fn run_read<T, F>(&self, operation: F) -> StorageFuture<'_, T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StorageError> + Send + 'static,
    {
        if let Some(read_pool) = &self.read_pool {
            let connections = read_pool.connections();
            return Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    let mut guard = lock_any_read_connection(&connections)?;

                    operation(&mut guard)
                })
                .await?
            });
        }

        self.run(operation)
    }

    pub(super) fn run_read_until<T, F>(
        &self,
        deadline: Instant,
        timeout_message: &'static str,
        operation: F,
    ) -> StorageFuture<'_, T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StorageError> + Send + 'static,
    {
        if let Some(read_pool) = &self.read_pool {
            let connections = read_pool.connections();
            return Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    let mut guard =
                        lock_any_read_connection_until(&connections, deadline, timeout_message)?;

                    operation(&mut guard)
                })
                .await?
            });
        }

        let connection = Arc::clone(&self.connection);
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut guard = lock_connection_until(&connection, deadline, timeout_message)?;

                operation(&mut guard)
            })
            .await?
        })
    }

    pub(super) fn try_run_read<T, F>(&self, operation: F) -> StorageFuture<'_, T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StorageError> + Send + 'static,
    {
        if let Some(read_pool) = &self.read_pool {
            let connections = read_pool.connections();
            return Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    let mut guard = try_lock_any_read_connection(&connections)?;

                    operation(&mut guard)
                })
                .await?
            });
        }

        let connection = Arc::clone(&self.connection);
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut guard = match connection.try_lock() {
                    Ok(guard) => guard,
                    Err(TryLockError::Poisoned(_)) => return Err(StorageError::LockPoisoned),
                    Err(TryLockError::WouldBlock) => {
                        return Err(StorageError::Busy(
                            "sqlite write connection is currently occupied".to_owned(),
                        ));
                    }
                };

                operation(&mut guard)
            })
            .await?
        })
    }

    pub(in crate::storage) fn import_code_repository_from_database(
        &self,
        source_path: PathBuf,
        repository_id: String,
        source_scope: Option<String>,
    ) -> StorageFuture<'_, ()> {
        self.run(move |connection| {
            code::import_repository_from_database(
                connection,
                &source_path,
                &repository_id,
                source_scope.as_deref(),
            )
        })
    }

    pub(in crate::storage) fn code_repository_totals_excluding(
        &self,
        excluded_repository_ids: Vec<String>,
    ) -> StorageFuture<'_, crate::domain::CodeRepositoryTotals> {
        self.run_read(move |connection| {
            code::repository_totals_excluding(connection, &excluded_repository_ids)
        })
    }

    pub(in crate::storage) fn prune_code_repository_scopes_with_retained(
        &self,
        request: crate::storage::CodeScopeRetentionRequest,
        extra_retained_scopes: Vec<String>,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        self.run(move |connection| {
            code::prune_scopes_with_retained(connection, request, extra_retained_scopes)
        })
    }
}

#[cfg(test)]
mod mod_tests;
