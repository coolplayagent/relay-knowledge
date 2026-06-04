use crate::{
    api::{
        ApiError, ApiMetadata, RequestContext, StorageShardDiagnostics, StorageTopologyDiagnostics,
        StorageTopologyResponse,
    },
    application::service::{RelayKnowledgeService, storage_api_error},
    storage::StorageTopologySnapshot,
};

impl RelayKnowledgeService {
    /// Returns storage topology diagnostics without exposing concrete storage handles.
    pub async fn storage_topology_diagnostics(&self) -> StorageTopologyDiagnostics {
        match self.storage.topology_snapshot().await {
            Ok(snapshot) => self.storage_diagnostics_from_snapshot(snapshot, None),
            Err(error) => self.storage_diagnostics_from_snapshot(
                StorageTopologySnapshot::default(),
                Some(error.to_string()),
            ),
        }
    }

    pub async fn storage_topology_status(
        &self,
        context: RequestContext,
    ) -> Result<StorageTopologyResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(StorageTopologyResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            storage: self.storage_topology_diagnostics().await,
        })
    }

    pub(super) fn storage_diagnostics_from_snapshot(
        &self,
        snapshot: StorageTopologySnapshot,
        snapshot_error: Option<String>,
    ) -> StorageTopologyDiagnostics {
        let topology = self.runtime.storage.topology.as_str().to_owned();
        let control_database_path = self.runtime.paths.database_file().display().to_string();
        let repository_shards_dir = (self.runtime.storage.topology
            == crate::storage::StorageTopology::PartitionedSqlite)
            .then(|| {
                self.runtime
                    .paths
                    .repository_shards_dir()
                    .display()
                    .to_string()
            });
        let mut runtime_state_paths = vec![control_database_path.clone()];
        if let Some(path) = repository_shards_dir.clone() {
            runtime_state_paths.push(path);
        }

        let shards = snapshot
            .shards
            .into_iter()
            .map(|entry| StorageShardDiagnostics {
                repository_id: entry.repository_id,
                state: entry.state,
                shard_locator: entry.shard_locator,
                resolved_path: entry.resolved_path,
                source_scope_count: entry.source_scope_count,
                exists: entry.exists,
                updated_at_ms: entry.updated_at_ms,
                degraded_reason: (!entry.exists)
                    .then(|| "repository shard file is missing".to_owned()),
            })
            .collect::<Vec<_>>();
        let active_shard_count = shards
            .iter()
            .filter(|shard| shard.state == "active")
            .count();
        let staged_shard_count = shards
            .iter()
            .filter(|shard| shard.state == "staged")
            .count();
        let missing_shard_count = shards.iter().filter(|shard| !shard.exists).count();
        let degraded_reason = snapshot_error.or_else(|| {
            (missing_shard_count > 0).then(|| {
                "partitioned_sqlite shard catalog references missing shard files".to_owned()
            })
        });

        StorageTopologyDiagnostics {
            topology,
            control_database_path,
            repository_shards_dir,
            shard_catalog_active: active_shard_count > 0,
            active_shard_count,
            staged_shard_count,
            missing_shard_count,
            runtime_state_paths,
            shards,
            degraded_reason,
        }
    }
}
