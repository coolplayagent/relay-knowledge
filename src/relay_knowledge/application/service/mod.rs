use std::{path::PathBuf, sync::Arc, time::Instant};

use serde::Serialize;

use crate::{
    api::{
        AgentProtocolStatus, ApiError, ApiMetadata, CodeIndexWorkerRunRequest,
        CodeIndexWorkerRunResponse, EmbeddingProviderProbeResponse, GRAPH_CANVAS_MAX_LIMIT,
        GraphCanvasEdge, GraphCanvasKind, GraphCanvasNode, GraphCanvasRequest, GraphCanvasResponse,
        GraphCanvasSummary, GraphInspectionRequest, GraphInspectionResponse, HealthResponse,
        HybridRetrievalRequest, HybridRetrievalResponse, IndexRefreshRequest, IndexRefreshResponse,
        IngestRequest, IngestResponse, MultimodalExtractionRequest, MultimodalExtractionResponse,
        ProjectStatusResponse, RequestContext, ServiceRecoveryReport,
    },
    domain::{
        AuditStatus, CodeParseStatusCounts, CodeRepositoryTotals, ContextGraphPath,
        ContextPackItem, FreshnessPolicy, FusionDiagnostics, IndexKind, RECIPROCAL_RANK_FUSION_K,
        RetrievalBackendStatus, RetrievalBudgetUsed, RetrievalHit, RetrievalMode,
        RetrievedContextPack, RetrieverSource, SourceScope,
    },
    env::EnvironmentConfig,
    model_provider::ModelProviderConfigService,
    observability::ObservabilityRuntime,
    project::{
        LINUX_SERVICE_DEFINITION_FILE_NAME, MACOS_SERVICE_DEFINITION_FILE_NAME, PROJECT_NAME,
        WINDOWS_SERVICE_DEFINITION_FILE_NAME,
    },
    retrieval::{
        RetrievalPlan,
        provider::{EmbeddingRequest, ProviderRetryClass, embedding_provider},
        read_model_backend_statuses,
    },
    storage::{
        FileIndexDiagnostics, GraphCanvasSelection, GraphCanvasStorageRequest, GraphInspection,
        GraphSearchRequest, KnowledgeStore, NewAuditEvent, StorageError,
    },
};

use storage_provider::StorageProvider;

use super::{
    RuntimeConfiguration, RuntimeConfigurationError,
    knowledge::{
        index_refresh::{
            index_refresh_outcome, metadata_for_indexes, recover_index_kinds, refresh_index_kinds,
        },
        ingest::mutation_batch_from_request,
        multimodal::extraction_ingest_request,
    },
    status::{agent_protocol_status, runtime_status, runtime_status_with_model_profiles},
    update::{VersionCheckResponse, check_for_updates},
};

#[cfg(test)]
use super::knowledge::ingest::generated_evidence_id;

/// Shared application service used by CLI, Web, and future API adapters.
#[derive(Clone)]
pub struct RelayKnowledgeService {
    pub(super) runtime: RuntimeConfiguration,
    pub(super) storage: StorageProvider,
    pub(super) health_cache: Arc<tokio::sync::RwLock<Option<HealthResponse>>>,
}

impl RelayKnowledgeService {
    /// Creates a service from already validated foundational configuration.
    pub fn new(runtime: RuntimeConfiguration) -> Self {
        Self {
            storage: StorageProvider::configured(&runtime),
            runtime,
            health_cache: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Creates a service backed by an explicit store for deterministic tests.
    pub fn with_store(runtime: RuntimeConfiguration, store: Arc<dyn KnowledgeStore>) -> Self {
        Self {
            runtime,
            storage: StorageProvider::ready(store),
            health_cache: Arc::new(tokio::sync::RwLock::new(None)),
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

    /// Returns the shared observability runtime for interface adapters.
    pub fn observability(&self) -> ObservabilityRuntime {
        self.runtime.observability.clone()
    }

    /// Returns the model provider configuration service rooted in runtime paths.
    pub fn model_provider_config(&self) -> ModelProviderConfigService {
        ModelProviderConfigService::new(self.runtime.paths.clone())
    }

    /// Checks configured release sources without opening graph storage.
    pub async fn check_for_updates(&self, force_refresh: bool) -> VersionCheckResponse {
        check_for_updates(
            &self.runtime.paths,
            &self.runtime.network,
            &self.runtime.updates,
            force_refresh,
        )
        .await
    }

    /// Persists a redacted agent protocol audit event through the durable sink.
    pub async fn record_agent_audit(&self, event: AgentDurableAuditInput) -> Result<(), ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        store
            .insert_audit_event(NewAuditEvent {
                operation: event.operation,
                interface: event.interface,
                request_id: event.request_id,
                trace_id: event.trace_id,
                status: event.status,
                actor: event.actor,
                source_scope: event.source_scope,
                graph_version: event.graph_version,
                detail_json: event.detail_json,
                message: event.message,
                now_ms: current_time_millis(),
            })
            .await
            .map(|_| ())
            .map_err(storage_api_error)
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

        let model_profiles = self
            .model_provider_config()
            .profile_summary(&self.runtime.retrieval)
            .await;

        Ok(ProjectStatusResponse {
            project_name: PROJECT_NAME.to_owned(),
            metadata: ApiMetadata::graph_only(&context, graph_version),
            runtime: runtime_status_with_model_profiles(&self.runtime, model_profiles),
        })
    }

    /// Returns runtime diagnostics without opening or migrating graph storage.
    pub fn runtime_diagnostics(
        &self,
        context: RequestContext,
    ) -> (ProjectStatusResponse, AgentProtocolStatus) {
        (
            ProjectStatusResponse {
                project_name: PROJECT_NAME.to_owned(),
                metadata: ApiMetadata::graph_only(&context, crate::domain::GraphVersion::ZERO),
                runtime: runtime_status(&self.runtime),
            },
            agent_protocol_status(&self.runtime),
        )
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
        let mut disabled_retriever_sources = self.runtime.retrieval.disabled_retriever_sources();
        if plan.freshness == FreshnessPolicy::GraphOnly {
            for source in [RetrieverSource::Semantic, RetrieverSource::Vector] {
                if !disabled_retriever_sources.contains(&source) {
                    disabled_retriever_sources.push(source);
                }
            }
        }
        let candidate_limit = self.runtime.retrieval.rerank.candidate_limit(plan.limit);
        let results = store
            .search(GraphSearchRequest {
                query: plan.query.clone(),
                source_scope: plan.source_scope.clone(),
                graph_version,
                limit: candidate_limit,
                disabled_retriever_sources,
            })
            .await
            .map_err(storage_api_error)?;
        let (mut results, mut rerank) = self.runtime.retrieval.rerank.rerank(&plan.query, results);
        let truncated = results.len() > plan.limit;
        results.truncate(plan.limit);
        rerank.returned_count = results.len();
        if rerank.degraded {
            if let Some(reason) = &rerank.reason {
                degraded_reasons.push(reason.clone());
            }
        }
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
                    rerank: hit.rerank.clone(),
                })
                .collect(),
        };
        let budget_used = RetrievalBudgetUsed {
            limit: plan.limit,
            candidate_count: rerank.candidate_count,
            returned_count: results.len(),
            context_bytes: retrieval_context_bytes(&results, &context_pack, &backend_statuses),
        };
        let fusion = FusionDiagnostics {
            algorithm: "reciprocal_rank_fusion".to_owned(),
            k: RECIPROCAL_RANK_FUSION_K,
            candidate_count: budget_used.candidate_count,
        };
        let degraded_reason = (!degraded_reasons.is_empty()).then(|| degraded_reasons.join("; "));

        Ok(HybridRetrievalResponse {
            metadata,
            context_pack,
            retrieval_mode,
            source_scope: plan.source_scope,
            freshness: plan.freshness,
            results,
            fusion,
            rerank,
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
        let repository_code_totals = store
            .code_repository_totals()
            .await
            .map_err(storage_api_error)?;
        let graph = graph_with_repository_code_totals(
            store.inspect_graph().await.map_err(storage_api_error)?,
            &repository_code_totals,
        );

        Ok(GraphInspectionResponse {
            metadata: ApiMetadata::graph_only(&context, graph.graph_version),
            graph,
            repository_code_totals,
        })
    }

    /// Returns a bounded read-only graph canvas snapshot for the Web workspace.
    pub async fn graph_canvas(
        &self,
        request: GraphCanvasRequest,
        context: RequestContext,
    ) -> Result<GraphCanvasResponse, ApiError> {
        if request.limit == 0 || request.limit > GRAPH_CANVAS_MAX_LIMIT {
            return Err(ApiError::invalid_argument(format!(
                "graph canvas limit must be between 1 and {GRAPH_CANVAS_MAX_LIMIT}"
            )));
        }
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let snapshot = store
            .graph_canvas(GraphCanvasStorageRequest {
                selection: canvas_selection(request.kind),
                source_scope: request.source_scope,
                query: request.query,
                graph_version,
                limit: request.limit,
            })
            .await
            .map_err(storage_api_error)?;
        let node_count = snapshot.nodes.len();
        let edge_count = snapshot.edges.len();

        Ok(GraphCanvasResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            nodes: snapshot
                .nodes
                .into_iter()
                .map(|node| GraphCanvasNode {
                    id: node.id,
                    kind: node.kind,
                    label: node.label,
                    subtitle: node.subtitle,
                    source_scope: node.source_scope,
                    graph_version: node.graph_version.get(),
                    weight: node.weight,
                    status: node.status,
                    details: node.details,
                })
                .collect(),
            edges: snapshot
                .edges
                .into_iter()
                .map(|edge| GraphCanvasEdge {
                    id: edge.id,
                    kind: edge.kind,
                    source: edge.source,
                    target: edge.target,
                    label: edge.label,
                    graph_version: edge.graph_version.get(),
                    confidence_basis_points: edge.confidence_basis_points,
                    evidence_count: edge.evidence_count,
                    details: edge.details,
                })
                .collect(),
            summary: GraphCanvasSummary {
                kind: request.kind,
                node_count,
                edge_count,
                truncated: snapshot.truncated,
                available_kinds: snapshot.available_kinds,
            },
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

    /// Probes the configured remote embedding provider without exposing secrets.
    pub async fn probe_embedding_provider(
        &self,
        context: RequestContext,
    ) -> Result<EmbeddingProviderProbeResponse, ApiError> {
        let Some(remote) = self.runtime.retrieval.remote_embedding.clone() else {
            return Ok(EmbeddingProviderProbeResponse {
                metadata: ApiMetadata::graph_only(&context, crate::domain::GraphVersion::ZERO),
                ok: false,
                provider: None,
                model: self.runtime.retrieval.vector_model.name.clone(),
                dimension: self.runtime.retrieval.vector_model.dimension,
                latency_ms: None,
                error_code: Some("remote_embedding_not_configured".to_owned()),
                error_message: Some("remote embedding provider is not configured".to_owned()),
                retryable: Some(false),
            });
        };
        let network = self.runtime.network.current();
        let client = crate::net::http::outbound_json_client(&network.http).map_err(|error| {
            ApiError::invalid_argument(format!("failed to build HTTP client: {error}"))
        })?;
        let provider_name = remote.provider.as_str().to_owned();
        let provider = embedding_provider(remote, client);
        let started = Instant::now();
        let result = provider
            .embed(EmbeddingRequest {
                inputs: vec!["relay-knowledge provider probe".to_owned()],
                model: self.runtime.retrieval.vector_model.name.clone(),
                dimension: self.runtime.retrieval.vector_model.dimension,
            })
            .await;

        match result {
            Ok(_) => Ok(EmbeddingProviderProbeResponse {
                metadata: ApiMetadata::graph_only(&context, crate::domain::GraphVersion::ZERO),
                ok: true,
                provider: Some(provider_name),
                model: self.runtime.retrieval.vector_model.name.clone(),
                dimension: self.runtime.retrieval.vector_model.dimension,
                latency_ms: Some(duration_millis(started.elapsed())),
                error_code: None,
                error_message: None,
                retryable: None,
            }),
            Err(error) => Ok(EmbeddingProviderProbeResponse {
                metadata: ApiMetadata::graph_only(&context, crate::domain::GraphVersion::ZERO),
                ok: error.code == "rate_limited" && error.retry == ProviderRetryClass::Retryable,
                provider: Some(provider_name),
                model: self.runtime.retrieval.vector_model.name.clone(),
                dimension: self.runtime.retrieval.vector_model.dimension,
                latency_ms: Some(duration_millis(started.elapsed())),
                error_code: Some(error.code),
                error_message: Some(error.message),
                retryable: Some(error.retry == ProviderRetryClass::Retryable),
            }),
        }
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

    pub(super) async fn store(&self) -> Result<Arc<dyn KnowledgeStore>, StorageError> {
        self.storage.get().await
    }

    /// Runs one split-worker preview code-index attempt through durable task leases.
    pub async fn run_code_index_worker_preview(
        &self,
        request: CodeIndexWorkerRunRequest,
        context: RequestContext,
    ) -> Result<CodeIndexWorkerRunResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let task = self
            .run_code_index_task_once(request.task_id, context.clone())
            .await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeIndexWorkerRunResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            worker_kind: "code_index".to_owned(),
            claimed: task.is_some(),
            task,
        })
    }

    /// Returns the persistent agent audit log path resolved by the path boundary.
    pub fn agent_audit_log_path(&self) -> PathBuf {
        self.runtime.paths.agent_audit_log_file()
    }
}

/// Durable audit event input accepted from resident agent adapters.
#[derive(Debug, Clone)]
pub struct AgentDurableAuditInput {
    pub operation: String,
    pub interface: String,
    pub request_id: String,
    pub trace_id: String,
    pub status: AuditStatus,
    pub actor: Option<String>,
    pub source_scope: Option<String>,
    pub graph_version: u64,
    pub detail_json: String,
    pub message: Option<String>,
}

pub(super) fn current_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

pub(super) fn storage_api_error(error: StorageError) -> ApiError {
    ApiError::storage_unavailable(error.to_string())
}

pub(super) async fn file_index_diagnostics_or_default(
    store: &Arc<dyn KnowledgeStore>,
) -> Result<FileIndexDiagnostics, ApiError> {
    match store.file_index_diagnostics().await {
        Ok(diagnostics) => Ok(diagnostics),
        Err(StorageError::InvalidInput(message))
            if message == "file index storage is unavailable" =>
        {
            Ok(FileIndexDiagnostics::default())
        }
        Err(error) => Err(storage_api_error(error)),
    }
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

pub(super) fn graph_with_repository_code_totals(
    mut graph: GraphInspection,
    repository_totals: &CodeRepositoryTotals,
) -> GraphInspection {
    graph.code_file_count = graph
        .code_file_count
        .saturating_add(repository_totals.indexed_file_count);
    graph.code_symbol_count = graph
        .code_symbol_count
        .saturating_add(repository_totals.symbol_count);
    graph.code_reference_count = graph
        .code_reference_count
        .saturating_add(repository_totals.reference_count);
    graph.code_chunk_count = graph
        .code_chunk_count
        .saturating_add(repository_totals.chunk_count);
    graph.code_parse_status_counts = add_parse_status_counts(
        graph.code_parse_status_counts,
        repository_totals.parse_status_counts,
    );

    graph
}

fn canvas_selection(kind: GraphCanvasKind) -> GraphCanvasSelection {
    match kind {
        GraphCanvasKind::Knowledge => GraphCanvasSelection::Knowledge,
        GraphCanvasKind::Code => GraphCanvasSelection::Code,
        GraphCanvasKind::Mixed => GraphCanvasSelection::Mixed,
    }
}

fn add_parse_status_counts(
    left: CodeParseStatusCounts,
    right: CodeParseStatusCounts,
) -> CodeParseStatusCounts {
    CodeParseStatusCounts {
        parsed: left.parsed.saturating_add(right.parsed),
        partial: left.partial.saturating_add(right.partial),
        text_only: left.text_only.saturating_add(right.text_only),
        failed: left.failed.saturating_add(right.failed),
    }
}

fn serialized_context_bytes<T: Serialize + ?Sized>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX / 4)
}

fn duration_millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
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

mod health;
pub(crate) mod knowledge_map;
mod service_status;
mod storage_diagnostics;
mod storage_provider;

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
