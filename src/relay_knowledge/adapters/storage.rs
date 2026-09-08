//! SQLite storage construction behind the application factory contract.

use std::{path::PathBuf, sync::Arc};

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
    access_verified: Arc<tokio::sync::OnceCell<()>>,
}

impl SqliteKnowledgeStoreFactory {
    /// Captures validated paths and topology without opening storage eagerly.
    pub fn new(paths: RuntimePaths, topology: StorageTopology) -> Self {
        Self {
            database_path: paths.database_file(),
            paths,
            topology,
            access_verified: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// Serializes initial ACL work and reuses it for diagnostics after open,
    /// keeping repeated status queries free of security subprocess launches.
    async fn ensure_access(&self, access: StorageDirectoryAccess) -> Result<(), StorageError> {
        self.access_verified
            .get_or_try_init(|| async {
                self.paths
                    .ensure_storage_access(access)
                    .await
                    .map_err(|error| StorageError::InvalidInput(error.to_string()))
            })
            .await?;
        Ok(())
    }
}

impl KnowledgeStoreFactory for SqliteKnowledgeStoreFactory {
    fn open(&self) -> KnowledgeStoreFactoryFuture<'_, Arc<dyn KnowledgeStore>> {
        let config = self.clone();
        Box::pin(async move {
            config
                .ensure_access(StorageDirectoryAccess::OpenOrCreate)
                .await?;
            tokio::task::spawn_blocking(move || open_store(config))
                .await
                .map_err(StorageError::from)?
        })
    }

    fn topology_snapshot(&self) -> KnowledgeStoreFactoryFuture<'_, StorageTopologySnapshot> {
        let config = self.clone();
        Box::pin(async move {
            config
                .ensure_access(StorageDirectoryAccess::ExistingOnly)
                .await?;
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
#[path = "storage_tests.rs"]
mod tests;
