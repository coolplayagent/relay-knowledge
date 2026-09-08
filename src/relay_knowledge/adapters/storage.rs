//! SQLite storage construction behind the application factory contract.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    paths::{RuntimePaths, StorageDirectoryAccess},
    storage::{
        KnowledgeStore, KnowledgeStoreFactory, KnowledgeStoreFactoryFuture,
        PartitionedSqliteKnowledgeStore, SqliteGraphStore, StorageError, StorageTopology,
        StorageTopologySnapshot,
    },
};

/// Configured SQLite factory assembled by the outer bootstrap layer.
#[derive(Debug, Clone)]
pub struct SqliteKnowledgeStoreFactory {
    database_path: PathBuf,
    paths: RuntimePaths,
    topology: StorageTopology,
    store_opened: Arc<AtomicBool>,
    validation_lock: Arc<tokio::sync::Mutex<()>>,
}

impl SqliteKnowledgeStoreFactory {
    /// Captures validated paths and topology without opening storage eagerly.
    pub fn new(paths: RuntimePaths, topology: StorageTopology) -> Self {
        Self {
            database_path: paths.database_file(),
            paths,
            topology,
            store_opened: Arc::new(AtomicBool::new(false)),
            validation_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

impl KnowledgeStoreFactory for SqliteKnowledgeStoreFactory {
    fn validate_lifecycle_storage(&self) -> KnowledgeStoreFactoryFuture<'_, ()> {
        let config = self.clone();
        Box::pin(async move {
            let validation_lock = Arc::clone(&config.validation_lock);
            let _validation = validation_lock.lock().await;
            if !config
                .paths
                .database_file_exists()
                .await
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?
            {
                return Ok(());
            }
            config
                .paths
                .ensure_storage_access(StorageDirectoryAccess::ExistingOnly)
                .await
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
            tokio::task::spawn_blocking(move || validate_configured_topology(&config))
                .await
                .map_err(StorageError::from)?
        })
    }

    fn open(&self) -> KnowledgeStoreFactoryFuture<'_, Arc<dyn KnowledgeStore>> {
        let config = self.clone();
        Box::pin(async move {
            let validation_lock = Arc::clone(&config.validation_lock);
            let _validation = validation_lock.lock().await;
            config
                .paths
                .ensure_storage_access(StorageDirectoryAccess::OpenOrCreate)
                .await
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
            let opened = Arc::clone(&config.store_opened);
            let store = tokio::task::spawn_blocking(move || open_store(config))
                .await
                .map_err(StorageError::from)??;
            // Only a successful open enables diagnostics to reuse validation.
            // A read-only probe or a failed open cannot authorize a later open.
            opened.store(true, Ordering::Release);
            Ok(store)
        })
    }

    fn topology_snapshot(&self) -> KnowledgeStoreFactoryFuture<'_, StorageTopologySnapshot> {
        let config = self.clone();
        Box::pin(async move {
            let validation_lock = Arc::clone(&config.validation_lock);
            let validation = validation_lock.lock().await;
            if !config.store_opened.load(Ordering::Acquire) {
                config
                    .paths
                    .ensure_storage_access(StorageDirectoryAccess::ExistingOnly)
                    .await
                    .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
            }
            drop(validation);
            tokio::task::spawn_blocking(move || {
                PartitionedSqliteKnowledgeStore::topology_snapshot_from_catalog(
                    config.database_path,
                    &config.paths,
                )
            })
            .await
            .map_err(StorageError::from)?
        })
    }
}

fn open_store(
    config: SqliteKnowledgeStoreFactory,
) -> Result<Arc<dyn KnowledgeStore>, StorageError> {
    validate_configured_topology(&config)?;
    match config.topology {
        StorageTopology::SingleSqlite => {
            Ok(Arc::new(SqliteGraphStore::open(config.database_path)?) as Arc<dyn KnowledgeStore>)
        }
        StorageTopology::PartitionedSqlite => Ok(Arc::new(PartitionedSqliteKnowledgeStore::open(
            config.database_path,
            config.paths,
        )?) as Arc<dyn KnowledgeStore>),
    }
}

fn validate_configured_topology(config: &SqliteKnowledgeStoreFactory) -> Result<(), StorageError> {
    let active_catalog =
        PartitionedSqliteKnowledgeStore::has_active_catalog(&config.database_path)?;
    if config.topology == StorageTopology::SingleSqlite && active_catalog {
        return Err(StorageError::InvalidInput(
            "single_sqlite cannot open a database with active partitioned_sqlite shards; set RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite or migrate the shard catalog before rollback".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
