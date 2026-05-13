use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use serde::Serialize;

use crate::{
    api::{
        ApiError, ApiMetadata, GraphInspectionRequest, GraphInspectionResponse, HealthResponse,
        HybridRetrievalRequest, HybridRetrievalResponse, IndexRefreshRequest, IndexRefreshResponse,
        IngestRequest, IngestResponse, MultimodalExtractionRequest, MultimodalExtractionResponse,
        ProjectStatusResponse, RequestContext, ServiceRecoveryReport, ServiceStatusResponse,
    },
    domain::{
        ContextGraphPath, ContextPackItem, FreshnessPolicy, FusionDiagnostics, IndexKind,
        ProposalState, RECIPROCAL_RANK_FUSION_K, RetrievalBackendStatus, RetrievalBudgetUsed,
        RetrievalHit, RetrievalMode, RetrievedContextPack, RetrieverSource, SourceScope,
    },
    env::EnvironmentConfig,
    project::{
        DATABASE_FILE_NAME, LINUX_SERVICE_DEFINITION_FILE_NAME, MACOS_SERVICE_DEFINITION_FILE_NAME,
        PROJECT_NAME, WINDOWS_SERVICE_DEFINITION_FILE_NAME,
    },
    retrieval::{RetrievalPlan, read_model_backend_statuses},
    storage::{
        GraphSearchRequest, KnowledgeStore, ProposalListRequest, SqliteGraphStore, StorageError,
    },
};

use super::{
    RuntimeConfiguration, RuntimeConfigurationError,
    index_refresh::{
        filter_outcome_to_read_models, index_refresh_outcome, metadata_for_indexes,
        reconcile_index_refreshes, recover_index_kinds, refresh_index_kinds,
    },
    ingest::mutation_batch_from_request,
    multimodal::extraction_ingest_request,
    status::{agent_protocol_status, runtime_status},
};

#[cfg(test)]
use super::ingest::generated_evidence_id;

/// Shared application service used by CLI, Web, and future API adapters.
#[derive(Clone)]
pub struct RelayKnowledgeService {
    pub(super) runtime: RuntimeConfiguration,
    pub(super) storage: StorageProvider,
}

impl RelayKnowledgeService {
    /// Creates a service from already validated foundational configuration.
    pub fn new(runtime: RuntimeConfiguration) -> Self {
        let database_path = runtime.paths.data_dir.join(DATABASE_FILE_NAME);

        Self {
            runtime,
            storage: StorageProvider::sqlite(database_path),
        }
    }

    /// Creates a service backed by an explicit store for deterministic tests.
    pub fn with_store(runtime: RuntimeConfiguration, store: Arc<dyn KnowledgeStore>) -> Self {
        Self {
            runtime,
            storage: StorageProvider::ready(store),
        }
    }

    /// Creates a service by reading the current process environment once.
    pub async fn from_process_environment() -> Result<Self, RuntimeConfigurationError> {
        RuntimeConfiguration::from_process_environment()
            .await
            .map(Self::new)
    }

    /// Creates a service from a deterministic environment snapshot.
    pub async fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, RuntimeConfigurationError> {
        RuntimeConfiguration::from_environment(environment)
            .await
            .map(Self::new)
    }

    /// Applies network-related settings from a typed environment snapshot.
    pub async fn refresh_network_from_environment(
        &self,
        environment: &EnvironmentConfig,
    ) -> Result<(), RuntimeConfigurationError> {
        self.runtime
            .network
            .refresh_from_environment(environment)
            .map(|_| ())
            .map_err(RuntimeConfigurationError::Network)
    }

    /// Re-reads process environment variables and applies network changes.
    pub async fn refresh_network_from_process_environment(
        &self,
    ) -> Result<(), RuntimeConfigurationError> {
        self.runtime
            .network
            .refresh_from_process_environment()
            .map(|_| ())
            .map_err(RuntimeConfigurationError::NetworkRuntime)
    }

    /// Returns the current project status through the unified API contract.
    pub async fn project_status(
        &self,
        context: RequestContext,
    ) -> Result<ProjectStatusResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(ProjectStatusResponse {
            project_name: PROJECT_NAME.to_owned(),
            metadata: ApiMetadata::graph_only(&context, graph_version),
            runtime: runtime_status(&self.runtime),
        })
    }

    /// Commits evidence into graph storage and refreshes all v1 index metadata.
    pub async fn ingest(
        &self,
        request: IngestRequest,
        context: RequestContext,
    ) -> Result<IngestResponse, ApiError> {
        let batch = mutation_batch_from_request(request)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let worker_evidence = batch.evidence.clone();
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let receipt = store
            .commit_mutation_batch(batch)
            .await
            .map_err(storage_api_error)?;
        self.queue_worker_tasks_for_evidence(&store, &worker_evidence, receipt.graph_version)
            .await?;
        let (indexes, metadata, index_refresh_error) = match refresh_index_kinds(
            &store,
            IndexKind::ALL,
            receipt.graph_version,
            &self.runtime.retrieval,
        )
        .await
        {
            Ok(outcome) => {
                let metadata =
                    metadata_for_indexes(&context, receipt.graph_version, &outcome.indexes);

                (outcome.indexes, metadata, None)
            }
            Err(error) => (
                Vec::new(),
                ApiMetadata::indexed(&context, receipt.graph_version, None, None, true),
                Some(error.message),
            ),
        };

        Ok(IngestResponse {
            metadata,
            receipt,
            indexes,
            index_refresh_error,
        })
    }

    /// Commits derived multimodal worker output through the same bounded ingest path.
    pub async fn commit_multimodal_extraction(
        &self,
        request: MultimodalExtractionRequest,
        context: RequestContext,
    ) -> Result<MultimodalExtractionResponse, ApiError> {
        let converted = extraction_ingest_request(request).map_err(ApiError::invalid_argument)?;
        let parent_evidence_id = converted.parent_evidence_id;
        let derived_evidence_count = converted.derived_evidence_count;
        let response = self.ingest(converted.ingest, context).await?;

        Ok(MultimodalExtractionResponse {
            metadata: response.metadata,
            parent_evidence_id,
            derived_evidence_count,
            receipt: response.receipt,
            indexes: response.indexes,
            index_refresh_error: response.index_refresh_error,
        })
    }

    /// Retrieves context through the unified hybrid retrieval contract.
    pub async fn retrieve_context(
        &self,
        request: HybridRetrievalRequest,
        context: RequestContext,
    ) -> Result<HybridRetrievalResponse, ApiError> {
        let source_scope = normalize_optional_source_scope(request.source_scope)
            .map_err(ApiError::invalid_argument)?;
        let plan = RetrievalPlan::new(
            request.query,
            source_scope,
            request.limit,
            request.freshness,
        )
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        let mut retrieval_mode = RetrievalMode::Hybrid;
        let mut indexes = Vec::new();
        let mut metadata = ApiMetadata::graph_only(&context, graph_version);
        let mut degraded_reasons = Vec::new();
        let backend_statuses = if plan.freshness == FreshnessPolicy::GraphOnly {
            retrieval_mode = RetrievalMode::GraphOnly;
            degraded_reasons.push("graph_only freshness policy selected".to_owned());
            Vec::new()
        } else {
            indexes = store.index_statuses().await.map_err(storage_api_error)?;
            let mut active_indexes = indexes
                .iter()
                .filter(|status| self.runtime.retrieval.refreshes_index(status.kind))
                .cloned()
                .collect::<Vec<_>>();
            if plan.freshness == FreshnessPolicy::WaitUntilFresh {
                let stale_kinds = active_indexes
                    .iter()
                    .filter(|status| status.is_stale_for(graph_version))
                    .map(|status| status.kind)
                    .collect::<Vec<_>>();
                if !stale_kinds.is_empty() {
                    refresh_index_kinds(
                        &store,
                        stale_kinds,
                        graph_version,
                        &self.runtime.retrieval,
                    )
                    .await?;
                    indexes = store.index_statuses().await.map_err(storage_api_error)?;
                    active_indexes = indexes
                        .iter()
                        .filter(|status| self.runtime.retrieval.refreshes_index(status.kind))
                        .cloned()
                        .collect();
                }
            }

            let stale = active_indexes
                .iter()
                .any(|status| status.is_stale_for(graph_version));
            metadata = metadata_for_indexes(&context, graph_version, &active_indexes);
            if plan.freshness == FreshnessPolicy::AllowStale && stale {
                degraded_reasons
                    .push("one or more indexes are behind the graph version".to_owned());
            }
            read_model_backend_statuses(&plan, graph_version, &indexes, &self.runtime.retrieval)
        };
        if backend_statuses
            .iter()
            .any(|status| status.state == crate::domain::RetrievalBackendState::Unavailable)
        {
            degraded_reasons.push(
                "semantic/vector retrieval backends unavailable; using bm25, graph evidence, and code graph fallback"
                    .to_owned(),
            );
        }
        let degraded_reason = (!degraded_reasons.is_empty()).then(|| degraded_reasons.join("; "));
        let mut disabled_retriever_sources = self.runtime.retrieval.disabled_retriever_sources();
        if plan.freshness == FreshnessPolicy::GraphOnly {
            for source in [RetrieverSource::Semantic, RetrieverSource::Vector] {
                if !disabled_retriever_sources.contains(&source) {
                    disabled_retriever_sources.push(source);
                }
            }
        }
        let mut results = store
            .search(GraphSearchRequest {
                query: plan.query.clone(),
                source_scope: plan.source_scope.clone(),
                graph_version,
                limit: plan.limit.saturating_add(1),
                disabled_retriever_sources,
            })
            .await
            .map_err(storage_api_error)?;
        let truncated = results.len() > plan.limit;
        results.truncate(plan.limit);
        let context_pack = RetrievedContextPack {
            graph_version,
            source_scope: plan.source_scope.clone(),
            freshness: plan.freshness,
            truncated,
            backend_statuses: backend_statuses.clone(),
            items: results
                .iter()
                .map(|hit| ContextPackItem {
                    result_id: hit.evidence_id.clone(),
                    source_scope: hit.source_scope.clone(),
                    source_path: hit.source_path.clone(),
                    source_span: hit.source_span,
                    entities: hit.entities.clone(),
                    graph_facts: hit.graph_facts.clone(),
                    graph_paths: hit
                        .graph_facts
                        .iter()
                        .map(ContextGraphPath::from_fact)
                        .collect(),
                    code_artifact: hit.code_artifact.clone(),
                    retriever_sources: hit.retriever_sources.clone(),
                    ranking: hit.ranking.clone(),
                })
                .collect(),
        };
        let budget_used = RetrievalBudgetUsed {
            limit: plan.limit,
            candidate_count: results.len() + usize::from(truncated),
            returned_count: results.len(),
            context_bytes: retrieval_context_bytes(&results, &context_pack, &backend_statuses),
        };
        let fusion = FusionDiagnostics {
            algorithm: "reciprocal_rank_fusion".to_owned(),
            k: RECIPROCAL_RANK_FUSION_K,
            candidate_count: budget_used.candidate_count,
        };

        Ok(HybridRetrievalResponse {
            metadata,
            context_pack,
            retrieval_mode,
            source_scope: plan.source_scope,
            freshness: plan.freshness,
            results,
            fusion,
            backend_statuses,
            truncated,
            budget_used,
            degraded_reason,
            indexes,
        })
    }

    /// Returns graph inspection information without exposing storage internals.
    pub async fn inspect_graph(
        &self,
        _request: GraphInspectionRequest,
        context: RequestContext,
    ) -> Result<GraphInspectionResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph = store.inspect_graph().await.map_err(storage_api_error)?;
        let repository_code_totals = store
            .code_repository_totals()
            .await
            .map_err(storage_api_error)?;

        Ok(GraphInspectionResponse {
            metadata: ApiMetadata::graph_only(&context, graph.graph_version),
            graph,
            repository_code_totals,
        })
    }

    /// Refreshes derived index metadata up to the current graph version.
    pub async fn refresh_indexes(
        &self,
        request: IndexRefreshRequest,
        context: RequestContext,
    ) -> Result<IndexRefreshResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let outcome = refresh_index_kinds(
            &store,
            request.kinds,
            graph_version,
            &self.runtime.retrieval,
        )
        .await?;
        let metadata = metadata_for_indexes(&context, graph_version, &outcome.indexes);

        Ok(IndexRefreshResponse {
            metadata,
            indexes: outcome.indexes,
            index_cursors: outcome.cursors,
            diagnostics: outcome.diagnostics,
        })
    }

    /// Returns service and data health for diagnostics.
    pub async fn health(&self, context: RequestContext) -> Result<HealthResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph = store.inspect_graph().await.map_err(storage_api_error)?;
        let repository_code_totals = store
            .code_repository_totals()
            .await
            .map_err(storage_api_error)?;
        reconcile_index_refreshes(&store, graph.graph_version, &self.runtime.retrieval).await?;
        let outcome = filter_outcome_to_read_models(
            index_refresh_outcome(&store).await?,
            &self.runtime.retrieval,
        );
        let healthy = outcome
            .indexes
            .iter()
            .all(|status| !status.is_stale_for(graph.graph_version));

        Ok(HealthResponse {
            metadata: metadata_for_indexes(&context, graph.graph_version, &outcome.indexes),
            healthy,
            graph,
            repository_code_totals,
            indexes: outcome.indexes,
            index_cursors: outcome.cursors,
            index_refresh: outcome.diagnostics,
            runtime: runtime_status(&self.runtime),
        })
    }

    /// Reconciles derived index cursors before resident service work starts.
    pub async fn reconcile_startup_indexes(
        &self,
        context: RequestContext,
    ) -> Result<ServiceRecoveryReport, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let before = store.index_statuses().await.map_err(storage_api_error)?;
        let active_before = before
            .iter()
            .filter(|status| self.runtime.retrieval.refreshes_index(status.kind))
            .cloned()
            .collect::<Vec<_>>();
        let stale_index_kinds = active_before
            .iter()
            .filter(|status| status.is_stale_for(graph_version))
            .map(|status| status.kind)
            .collect::<Vec<_>>();
        let index_lag_max = active_before
            .iter()
            .map(|status| {
                graph_version
                    .get()
                    .saturating_sub(status.indexed_graph_version.get())
            })
            .max()
            .unwrap_or(0);
        let outcome = if stale_index_kinds.is_empty() {
            index_refresh_outcome(&store).await?
        } else {
            recover_index_kinds(
                &store,
                stale_index_kinds.clone(),
                graph_version,
                &self.runtime.retrieval,
            )
            .await?
        };
        let refreshed = outcome
            .indexes
            .iter()
            .filter(|status| {
                stale_index_kinds.contains(&status.kind) && !status.is_stale_for(graph_version)
            })
            .map(|status| status.kind)
            .collect::<Vec<_>>();
        let after = outcome.indexes;
        let active_after = after
            .iter()
            .filter(|status| self.runtime.retrieval.refreshes_index(status.kind))
            .cloned()
            .collect::<Vec<_>>();
        let metadata = metadata_for_indexes(&context, graph_version, &active_after);

        Ok(ServiceRecoveryReport {
            metadata,
            graph_version: graph_version.get(),
            stale_index_kinds,
            refreshed_index_kinds: refreshed,
            index_lag_max,
            task_queue_depth: outcome.diagnostics.queue_depth,
            dead_letter_count: outcome.diagnostics.dead_letter_count,
            heartbeat_state: "ready".to_owned(),
        })
    }

    /// Returns the managed background service definition location and defaults.
    pub async fn service_status(
        &self,
        context: RequestContext,
    ) -> Result<ServiceStatusResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let index_refresh =
            reconcile_index_refreshes(&store, graph_version, &self.runtime.retrieval).await?;
        let service_definition_path = self
            .runtime
            .paths
            .service_dir
            .join(service_definition_filename())
            .display()
            .to_string();
        let operator = store
            .service_operator_status()
            .await
            .map_err(storage_api_error)?;
        let workers = super::operations::overlay_worker_runtime(
            store.worker_statuses().await.map_err(storage_api_error)?,
            &self.runtime.workers,
        );
        let proposal_backlog = store
            .list_proposals(ProposalListRequest {
                state: Some(ProposalState::Proposed),
                limit: usize::MAX,
            })
            .await
            .map_err(storage_api_error)?
            .len();
        let audit_event_count = store.audit_event_count().await.map_err(storage_api_error)?;

        Ok(ServiceStatusResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            service_name: PROJECT_NAME.to_owned(),
            mode: operator.state.as_str().to_owned(),
            background_enabled: operator.state != crate::domain::ServiceOperatorState::Disabled,
            silent_updates_enabled: operator.silent_updates_enabled,
            service_definition_path,
            index_refresh,
            agent_protocols: agent_protocol_status(&self.runtime),
            operator,
            workers,
            proposal_backlog,
            audit_sink: crate::api::AuditSinkStatus {
                durable: true,
                event_count: audit_event_count,
                last_error: None,
            },
        })
    }

    pub(super) async fn store(&self) -> Result<Arc<dyn KnowledgeStore>, StorageError> {
        self.storage.get().await
    }
}

#[derive(Clone)]
pub(super) struct StorageProvider {
    path: Option<PathBuf>,
    ready: Arc<OnceLock<Arc<dyn KnowledgeStore>>>,
    init_lock: Arc<tokio::sync::Mutex<()>>,
}

impl StorageProvider {
    fn sqlite(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ready: Arc::new(OnceLock::new()),
            init_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    fn ready(store: Arc<dyn KnowledgeStore>) -> Self {
        let ready = OnceLock::new();
        let _ = ready.set(store);

        Self {
            path: None,
            ready: Arc::new(ready),
            init_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub(super) async fn get(&self) -> Result<Arc<dyn KnowledgeStore>, StorageError> {
        if let Some(store) = self.ready.get() {
            return Ok(Arc::clone(store));
        }
        let _guard = self.init_lock.lock().await;
        if let Some(store) = self.ready.get() {
            return Ok(Arc::clone(store));
        }

        let Some(path) = self.path.clone() else {
            return Err(StorageError::InvalidInput(
                "storage provider was not initialized".to_owned(),
            ));
        };
        let ready = Arc::clone(&self.ready);
        tokio::task::spawn_blocking(move || {
            if let Some(store) = ready.get() {
                return Ok(Arc::clone(store));
            }
            let store = Arc::new(SqliteGraphStore::open(path)?) as Arc<dyn KnowledgeStore>;
            let _ = ready.set(Arc::clone(&store));
            Ok(store)
        })
        .await?
    }
}

fn storage_api_error(error: StorageError) -> ApiError {
    ApiError::storage_unavailable(error.to_string())
}

fn normalize_optional_source_scope(value: Option<String>) -> Result<Option<String>, String> {
    value
        .map(|scope| {
            SourceScope::parse(scope)
                .map(String::from)
                .map_err(|error| error.to_string())
        })
        .transpose()
}

fn retrieval_context_bytes(
    results: &[RetrievalHit],
    context_pack: &RetrievedContextPack,
    backend_statuses: &[RetrievalBackendStatus],
) -> usize {
    serialized_context_bytes(&context_pack.backend_statuses)
        .saturating_add(serialized_context_bytes(backend_statuses))
        .saturating_add(results.iter().map(serialized_context_bytes).sum::<usize>())
        .saturating_add(
            context_pack
                .items
                .iter()
                .map(serialized_context_bytes)
                .sum::<usize>(),
        )
}

fn serialized_context_bytes<T: Serialize + ?Sized>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX / 4)
}

fn service_definition_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        WINDOWS_SERVICE_DEFINITION_FILE_NAME
    } else if cfg!(target_os = "macos") {
        MACOS_SERVICE_DEFINITION_FILE_NAME
    } else {
        LINUX_SERVICE_DEFINITION_FILE_NAME
    }
}

#[cfg(test)]
mod id_tests;

#[cfg(test)]
mod graph_only_tests;

#[cfg(test)]
mod recovery_tests;

#[cfg(test)]
mod refresh_tests;

#[cfg(test)]
mod storage_tests;

#[cfg(test)]
mod operations_tests;

#[cfg(test)]
mod tests;
