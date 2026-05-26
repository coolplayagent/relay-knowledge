use std::{sync::Arc, time::Duration};

use crate::{
    api::{ApiError, ApiMetadata, HealthResponse, RequestContext},
    domain::{CodeRepositoryTotals, GraphVersion},
    storage::{
        FileIndexDiagnostics, GraphInspection, HealthStorageSnapshot, IndexRefreshDiagnostics,
        KnowledgeStore, StorageError,
    },
};

use super::{
    RelayKnowledgeService,
    index_refresh::{IndexRefreshOutcome, filter_outcome_to_read_models, metadata_for_indexes},
    service::{current_time_millis, graph_with_repository_code_totals, storage_api_error},
    status::runtime_status_with_model_profiles,
};

const HEALTH_STORAGE_BUDGET: Duration = Duration::from_millis(500);

impl RelayKnowledgeService {
    /// Returns liveness-safe service and data health diagnostics.
    pub async fn health(&self, context: RequestContext) -> Result<HealthResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        match tokio::time::timeout(HEALTH_STORAGE_BUDGET, self.storage_health_snapshot(&store))
            .await
        {
            Ok(Ok(snapshot)) => {
                let response = self
                    .health_from_storage_snapshot(context, snapshot, None)
                    .await;
                *self.health_cache.write().await = Some(response.clone());
                Ok(response)
            }
            Ok(Err(StorageError::Busy(message))) => Ok(self
                .degraded_cached_health(context, format!("storage_busy: {message}"))
                .await),
            Ok(Err(error)) => Err(storage_api_error(error)),
            Err(_) => Ok(self
                .degraded_cached_health(context, "storage_busy: health snapshot timed out")
                .await),
        }
    }

    async fn storage_health_snapshot(
        &self,
        store: &Arc<dyn KnowledgeStore>,
    ) -> Result<HealthStorageSnapshot, StorageError> {
        match store.health_snapshot(current_time_millis()).await {
            Ok(snapshot) => Ok(snapshot),
            Err(StorageError::InvalidInput(message))
                if message == "health snapshot storage is unavailable" =>
            {
                self.legacy_health_snapshot(store).await
            }
            Err(error) => Err(error),
        }
    }

    async fn health_from_storage_snapshot(
        &self,
        context: RequestContext,
        snapshot: HealthStorageSnapshot,
        degraded_reason: Option<String>,
    ) -> HealthResponse {
        let HealthStorageSnapshot {
            graph,
            repository_code_totals,
            indexes,
            index_cursors,
            index_refresh,
            file_index,
        } = snapshot;
        let graph = graph_with_repository_code_totals(graph, &repository_code_totals);
        let outcome = filter_outcome_to_read_models(
            IndexRefreshOutcome {
                indexes,
                cursors: index_cursors,
                diagnostics: index_refresh,
            },
            &self.runtime.retrieval,
        );
        let healthy = degraded_reason.is_none()
            && outcome
                .indexes
                .iter()
                .all(|status| !status.is_stale_for(graph.graph_version));

        HealthResponse {
            metadata: metadata_for_indexes(&context, graph.graph_version, &outcome.indexes),
            healthy,
            degraded_reason,
            graph,
            repository_code_totals,
            indexes: outcome.indexes,
            index_cursors: outcome.cursors,
            index_refresh: outcome.diagnostics,
            file_index,
            runtime: runtime_status_with_model_profiles(
                &self.runtime,
                self.model_provider_config()
                    .profile_summary(&self.runtime.retrieval)
                    .await,
            ),
        }
    }

    async fn legacy_health_snapshot(
        &self,
        store: &Arc<dyn KnowledgeStore>,
    ) -> Result<HealthStorageSnapshot, StorageError> {
        Ok(HealthStorageSnapshot {
            graph: store.inspect_graph().await?,
            repository_code_totals: store.code_repository_totals().await?,
            indexes: store.index_statuses().await?,
            index_cursors: store.index_cursors().await?,
            index_refresh: store
                .index_refresh_diagnostics(current_time_millis())
                .await?,
            file_index: legacy_file_index_diagnostics_or_default(store).await?,
        })
    }

    async fn degraded_cached_health(
        &self,
        context: RequestContext,
        degraded_reason: impl Into<String>,
    ) -> HealthResponse {
        let degraded_reason = degraded_reason.into();
        if let Some(cached) = self.health_cache.read().await.clone() {
            let mut response = cached;
            response.metadata.trace_id = context.trace_id;
            response.metadata.request_id = context.request_id;
            response.metadata.stale = true;
            response.healthy = false;
            response.degraded_reason = Some(degraded_reason);
            return response;
        }

        HealthResponse {
            metadata: ApiMetadata::indexed(&context, GraphVersion::ZERO, None, None, true),
            healthy: false,
            degraded_reason: Some(degraded_reason),
            graph: GraphInspection::default(),
            repository_code_totals: CodeRepositoryTotals::default(),
            indexes: Vec::new(),
            index_cursors: Vec::new(),
            index_refresh: IndexRefreshDiagnostics::default(),
            file_index: FileIndexDiagnostics::default(),
            runtime: runtime_status_with_model_profiles(
                &self.runtime,
                self.model_provider_config()
                    .profile_summary(&self.runtime.retrieval)
                    .await,
            ),
        }
    }
}

async fn legacy_file_index_diagnostics_or_default(
    store: &Arc<dyn KnowledgeStore>,
) -> Result<FileIndexDiagnostics, StorageError> {
    match store.file_index_diagnostics().await {
        Ok(diagnostics) => Ok(diagnostics),
        Err(StorageError::InvalidInput(message))
            if message == "file index storage is unavailable" =>
        {
            Ok(FileIndexDiagnostics::default())
        }
        Err(error) => Err(error),
    }
}
