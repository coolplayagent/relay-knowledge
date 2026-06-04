use std::time::Duration;

use crate::{
    api::{
        ApiError, ApiMetadata, RequestContext, StorageShardDiagnostics, StorageTopologyDiagnostics,
        StorageTopologyResponse,
    },
    application::service::{RelayKnowledgeService, storage_api_error},
    domain::GraphVersion,
    storage::StorageTopologySnapshot,
};

const STORAGE_TOPOLOGY_BUDGET: Duration = Duration::from_millis(500);

impl RelayKnowledgeService {
    /// Returns storage topology diagnostics without exposing concrete storage handles.
    pub async fn storage_topology_diagnostics(&self) -> StorageTopologyDiagnostics {
        self.storage_topology_diagnostics_with_budget(STORAGE_TOPOLOGY_BUDGET)
            .await
    }

    pub(super) async fn storage_topology_diagnostics_with_budget(
        &self,
        budget: Duration,
    ) -> StorageTopologyDiagnostics {
        match tokio::time::timeout(budget, self.storage.topology_snapshot()).await {
            Ok(Ok(snapshot)) => self.storage_diagnostics_from_snapshot(snapshot, None),
            Ok(Err(error)) => self.storage_diagnostics_from_snapshot(
                StorageTopologySnapshot::default(),
                Some(error.to_string()),
            ),
            Err(_) => self.storage_diagnostics_from_snapshot(
                StorageTopologySnapshot::default(),
                Some("storage_topology_busy: topology snapshot timed out".to_owned()),
            ),
        }
    }

    pub async fn storage_topology_status(
        &self,
        context: RequestContext,
    ) -> Result<StorageTopologyResponse, ApiError> {
        let graph_version = match self.storage.ready_store() {
            Some(store) => store
                .current_graph_version()
                .await
                .map_err(storage_api_error)?,
            None => GraphVersion::ZERO,
        };

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
        let has_partitioned_catalog = !snapshot.shards.is_empty();
        let configured_partitioned =
            self.runtime.storage.topology == crate::storage::StorageTopology::PartitionedSqlite;
        let repository_shards_dir =
            (configured_partitioned || has_partitioned_catalog).then(|| {
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
            if !configured_partitioned && active_shard_count > 0 {
                Some(
                    "single_sqlite configuration found active partitioned_sqlite shard catalog"
                        .to_owned(),
                )
            } else {
                (missing_shard_count > 0).then(|| {
                    "partitioned_sqlite shard catalog references missing shard files".to_owned()
                })
            }
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
