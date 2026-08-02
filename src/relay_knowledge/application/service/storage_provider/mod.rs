use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use crate::{
    application::RuntimeConfiguration,
    paths::RuntimePaths,
    storage::{
        KnowledgeStore, PartitionedSqliteKnowledgeStore, SqliteGraphStore, StorageError,
        StorageTopology, StorageTopologySnapshot,
    },
};

#[derive(Clone)]
pub(in crate::application) struct StorageProvider {
    config: Option<StorageProviderConfig>,
    ready: Arc<OnceLock<Arc<dyn KnowledgeStore>>>,
    init_lock: Arc<tokio::sync::Mutex<()>>,
}

impl StorageProvider {
    pub(super) fn configured(runtime: &RuntimeConfiguration) -> Self {
        Self {
            config: Some(StorageProviderConfig {
                database_path: runtime.paths.database_file(),
                paths: runtime.paths.clone(),
                topology: runtime.storage.topology,
            }),
            ready: Arc::new(OnceLock::new()),
            init_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub(super) fn ready(store: Arc<dyn KnowledgeStore>) -> Self {
        let ready = OnceLock::new();
        let _ = ready.set(store);

        Self {
            config: None,
            ready: Arc::new(ready),
            init_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub(in crate::application) async fn get(
        &self,
    ) -> Result<Arc<dyn KnowledgeStore>, StorageError> {
        if let Some(store) = self.ready.get() {
            return Ok(Arc::clone(store));
        }
        let _guard = self.init_lock.lock().await;
        if let Some(store) = self.ready.get() {
            return Ok(Arc::clone(store));
        }

        let Some(config) = self.config.clone() else {
            return Err(StorageError::InvalidInput(
                "storage provider was not initialized".to_owned(),
            ));
        };
        let ready = Arc::clone(&self.ready);
        tokio::task::spawn_blocking(move || {
            if let Some(store) = ready.get() {
                return Ok(Arc::clone(store));
            }
            let store = open_store(config)?;
            let _ = ready.set(Arc::clone(&store));
            Ok(store)
        })
        .await?
    }

    pub(in crate::application) fn ready_store(&self) -> Option<Arc<dyn KnowledgeStore>> {
        self.ready.get().map(Arc::clone)
    }

    pub(in crate::application) async fn topology_snapshot(
        &self,
    ) -> Result<StorageTopologySnapshot, StorageError> {
        let Some(config) = self.config.clone() else {
            return Ok(StorageTopologySnapshot::default());
        };
        match config.topology {
            StorageTopology::SingleSqlite | StorageTopology::PartitionedSqlite => {
                tokio::task::spawn_blocking(move || {
                    PartitionedSqliteKnowledgeStore::topology_snapshot_from_catalog(
                        config.database_path,
                        &config.paths,
                    )
                })
                .await?
            }
        }
    }
}

#[derive(Clone)]
struct StorageProviderConfig {
    database_path: PathBuf,
    paths: RuntimePaths,
    topology: StorageTopology,
}

fn open_store(config: StorageProviderConfig) -> Result<Arc<dyn KnowledgeStore>, StorageError> {
    match config.topology {
        StorageTopology::SingleSqlite => {
            if PartitionedSqliteKnowledgeStore::has_active_catalog(&config.database_path)? {
                return Err(StorageError::InvalidInput(
                    "single_sqlite cannot open a database with active partitioned_sqlite shards; set RELAY_KNOWLEDGE_STORAGE_TOPOLOGY=partitioned_sqlite or migrate the shard catalog before rollback".to_owned(),
                ));
            }
            Ok(Arc::new(SqliteGraphStore::open(config.database_path)?) as Arc<dyn KnowledgeStore>)
        }
        StorageTopology::PartitionedSqlite => Ok(Arc::new(PartitionedSqliteKnowledgeStore::open(
            config.database_path,
            config.paths,
        )?) as Arc<dyn KnowledgeStore>),
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
