//! Owns cached shard handles and security validation before their first open.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::{
    paths::{RuntimePaths, StorageDirectoryAccess},
    storage::{SqliteGraphStore, StorageError},
};

pub(in crate::storage::partitioned) fn open_control_store(
    path: &Path,
) -> Result<Arc<SqliteGraphStore>, StorageError> {
    crate::storage::sqlite::validate_new_database_access(
        path,
        StorageDirectoryAccess::OpenOrCreate,
    )?;
    let control = Arc::new(SqliteGraphStore::open(path)?);
    super::initialize_catalog_schema(path)?;
    Ok(control)
}

pub(super) fn open_cached_repository_store(
    cache: &Arc<Mutex<HashMap<String, Arc<SqliteGraphStore>>>>,
    repository_id: String,
    db_path: PathBuf,
    control_path: PathBuf,
    paths: &RuntimePaths,
) -> Result<Arc<SqliteGraphStore>, StorageError> {
    let cached = cache
        .lock()
        .map_err(|_| StorageError::LockPoisoned)?
        .get(&repository_id)
        .cloned();
    if let Some(store) = cached {
        return Ok(store);
    }

    // This function runs only in explicit blocking workers. Re-enter the async
    // process boundary here so validation stays adjacent to the first SQLite
    // open, while cached handles do not launch repeated security processes.
    if paths.windows_data_sid.is_some() || cfg!(windows) {
        tokio::runtime::Handle::current()
            .block_on(
                paths
                    .ensure_storage_database_access(&db_path, StorageDirectoryAccess::OpenOrCreate),
            )
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    }
    // Do not hold the shared cache lock across the security process. Another
    // worker may have opened this repository while validation was running.
    let mut cache = cache.lock().map_err(|_| StorageError::LockPoisoned)?;
    if let Some(store) = cache.get(&repository_id) {
        return Ok(Arc::clone(store));
    }
    let store = Arc::new(SqliteGraphStore::open_with_publication_authority(
        &db_path,
        control_path,
        paths.clone(),
    )?);
    cache.insert(repository_id, Arc::clone(&store));
    Ok(store)
}
