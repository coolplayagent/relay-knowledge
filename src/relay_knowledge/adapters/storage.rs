//! SQLite storage construction behind the application factory contract.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{
    paths::{RuntimePaths, StorageDirectoryAccess},
    storage::{
        KnowledgeStore, KnowledgeStoreFactory, KnowledgeStoreFactoryFuture,
        PartitionedSqliteKnowledgeStore, SqliteGraphStore, SqliteTopologyReader, StorageError,
        StorageTopology, StorageTopologySnapshot,
    },
};

/// Configured SQLite factory assembled by the outer bootstrap layer.
#[derive(Debug, Clone)]
pub struct SqliteKnowledgeStoreFactory {
    database_path: PathBuf,
    paths: RuntimePaths,
    topology: StorageTopology,
    validation_lock: Arc<tokio::sync::Mutex<()>>,
    catalog_reader: Arc<Mutex<Option<SqliteTopologyReader>>>,
}

impl SqliteKnowledgeStoreFactory {
    /// Captures validated paths and topology without opening storage eagerly.
    pub fn new(paths: RuntimePaths, topology: StorageTopology) -> Self {
        Self {
            database_path: paths.database_file(),
            paths,
            topology,
            validation_lock: Arc::new(tokio::sync::Mutex::new(())),
            catalog_reader: Arc::new(Mutex::new(None)),
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
            tokio::task::spawn_blocking(move || {
                let store = open_store(config.clone())?;
                let reader = SqliteTopologyReader::open(&config.database_path, config.paths)?;
                *config
                    .catalog_reader
                    .lock()
                    .map_err(|_| StorageError::LockPoisoned)? = Some(reader);
                Ok(store)
            })
            .await
            .map_err(StorageError::from)?
        })
    }

    fn topology_snapshot(&self) -> KnowledgeStoreFactoryFuture<'_, StorageTopologySnapshot> {
        let config = self.clone();
        Box::pin(async move {
            let validation_lock = Arc::clone(&config.validation_lock);
            let validation = validation_lock.lock().await;
            let catalog = Arc::clone(&config.catalog_reader);
            if let Some(snapshot) = tokio::task::spawn_blocking(move || {
                catalog
                    .lock()
                    .map_err(|_| StorageError::LockPoisoned)?
                    .as_mut()
                    .map(SqliteTopologyReader::snapshot)
                    .transpose()
            })
            .await??
            {
                return Ok(snapshot);
            }
            // A cold snapshot needs a fresh pathname-based connection, with
            // current validation. Its permission result is never cached.
            config
                .paths
                .ensure_storage_database_access(
                    &config.database_path,
                    StorageDirectoryAccess::ExistingOnly,
                )
                .await
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
            drop(validation);
            tokio::task::spawn_blocking(move || {
                // The cancellable async check above already admitted this open.
                // Do not launch another security process inside this worker.
                if !config.database_path.exists() {
                    return Ok(StorageTopologySnapshot::default());
                }
                SqliteTopologyReader::open(&config.database_path, config.paths)?.snapshot()
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
