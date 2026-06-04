use std::path::Path;

use crate::paths::RuntimePaths;
use crate::storage::{StorageError, StorageTopologySnapshot};

use super::{
    PartitionedSqliteKnowledgeStore,
    catalog::{catalog_has_active_repositories, catalog_topology_snapshot},
};

impl PartitionedSqliteKnowledgeStore {
    pub fn has_active_catalog(control_path: impl AsRef<Path>) -> Result<bool, StorageError> {
        catalog_has_active_repositories(control_path.as_ref())
    }

    pub async fn topology_snapshot(&self) -> Result<StorageTopologySnapshot, StorageError> {
        self.catalog.topology_snapshot().await
    }

    pub fn topology_snapshot_from_catalog(
        control_path: impl AsRef<Path>,
        paths: &RuntimePaths,
    ) -> Result<StorageTopologySnapshot, StorageError> {
        catalog_topology_snapshot(control_path.as_ref(), paths)
    }
}
