use std::path::Path;

use crate::storage::{StorageError, StorageTopologySnapshot};

use super::{PartitionedSqliteKnowledgeStore, catalog::catalog_has_active_repositories};

impl PartitionedSqliteKnowledgeStore {
    pub fn has_active_catalog(control_path: impl AsRef<Path>) -> Result<bool, StorageError> {
        catalog_has_active_repositories(control_path.as_ref())
    }

    pub async fn topology_snapshot(&self) -> Result<StorageTopologySnapshot, StorageError> {
        self.catalog.topology_snapshot().await
    }
}
