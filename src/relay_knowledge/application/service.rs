use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use serde::Serialize;

use crate::{
    api::{
        ApiError, ApiMetadata, GraphInspectionRequest, GraphInspectionResponse, HealthResponse,
        HybridRetrievalRequest, HybridRetrievalResponse, IndexRefreshRequest, IndexRefreshResponse,
        IngestRequest, IngestResponse, ProjectStatusResponse, RequestContext,
        ServiceRecoveryReport, ServiceStatusResponse,
    },
    domain::{
        ContextPackItem, FreshnessPolicy, FusionDiagnostics, GraphVersion, IndexKind,
        RECIPROCAL_RANK_FUSION_K, RetrievalBackendStatus, RetrievalBudgetUsed, RetrievalHit,
        RetrievalMode, RetrievedContextPack, SourceScope,
    },
    env::EnvironmentConfig,
    project::PROJECT_NAME,
    retrieval::{DerivedRetrievalAdapter, DerivedRetrievalRequest, RetrievalPlan},
    storage::{GraphSearchRequest, KnowledgeStore, SqliteGraphStore, StorageError},
};

use super::{
    RuntimeConfiguration, RuntimeConfigurationError,
    index_refresh::{
        index_refresh_outcome, metadata_for_indexes, reconcile_index_refreshes,
        recover_index_kinds, refresh_index_kinds,
    },
    ingest::mutation_batch_from_request,
    status::{agent_protocol_status, runtime_status},
};

#[cfg(test)]
use super::ingest::generated_evidence_id;

/// Shared application service used by CLI, Web, and future API adapters.
#[derive(Clone)]
pub struct RelayKnowledgeService {
    runtime: RuntimeConfiguration,
    storage: StorageProvider,
}

impl RelayKnowledgeService {
    /// Creates a service from already validated foundational configuration.
    pub fn new(runtime: RuntimeConfiguration) -> Self {
        let database_path = runtime.paths.data_dir.join("relay-knowledge.sqlite");

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
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let receipt = store
            .commit_mutation_batch(batch)
            .await
            .map_err(storage_api_error)?;
        let (indexes, metadata, index_refresh_error) =
            match refresh_index_kinds(&store, IndexKind::ALL, receipt.graph_version).await {
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
            if plan.freshness == FreshnessPolicy::WaitUntilFresh {
                let stale_kinds = indexes
                    .iter()
                    .filter(|status| status.is_stale_for(graph_version))
                    .map(|status| status.kind)
                    .collect::<Vec<_>>();
                if !stale_kinds.is_empty() {
                    refresh_index_kinds(&store, stale_kinds, graph_version).await?;
                    indexes = store.index_statuses().await.map_err(storage_api_error)?;
                }
            }

            let stale = indexes
                .iter()
                .any(|status| status.is_stale_for(graph_version));
            metadata = metadata_for_indexes(&context, graph_version, &indexes);
            if plan.freshness == FreshnessPolicy::AllowStale && stale {
                degraded_reasons
                    .push("one or more indexes are behind the graph version".to_owned());
            }
            derived_backend_statuses(&plan, graph_version).await
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
        let mut results = store
            .search(GraphSearchRequest {
                query: plan.query.clone(),
                source_scope: plan.source_scope.clone(),
                graph_version,
                limit: plan.limit.saturating_add(1),
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

        Ok(GraphInspectionResponse {
            metadata: ApiMetadata::graph_only(&context, graph.graph_version),
            graph,
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
        let outcome = refresh_index_kinds(&store, request.kinds, graph_version).await?;
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
        reconcile_index_refreshes(&store, graph.graph_version).await?;
        let outcome = index_refresh_outcome(&store).await?;
        let healthy = outcome
            .indexes
            .iter()
            .all(|status| !status.is_stale_for(graph.graph_version));

        Ok(HealthResponse {
            metadata: metadata_for_indexes(&context, graph.graph_version, &outcome.indexes),
            healthy,
            graph,
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
        let stale_index_kinds = before
            .iter()
            .filter(|status| status.is_stale_for(graph_version))
            .map(|status| status.kind)
            .collect::<Vec<_>>();
        let index_lag_max = before
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
            recover_index_kinds(&store, stale_index_kinds.clone(), graph_version).await?
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
        let metadata = metadata_for_indexes(&context, graph_version, &after);

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
        let index_refresh = reconcile_index_refreshes(&store, graph_version).await?;
        let service_definition_path = self
            .runtime
            .paths
            .service_dir
            .join(service_definition_filename())
            .display()
            .to_string();

        Ok(ServiceStatusResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            service_name: PROJECT_NAME.to_owned(),
            mode: "disabled".to_owned(),
            background_enabled: false,
            silent_updates_enabled: false,
            service_definition_path,
            index_refresh,
            agent_protocols: agent_protocol_status(&self.runtime),
        })
    }

    pub(super) async fn store(&self) -> Result<Arc<dyn KnowledgeStore>, StorageError> {
        self.storage.get().await
    }
}

#[derive(Clone)]
struct StorageProvider {
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

    async fn get(&self) -> Result<Arc<dyn KnowledgeStore>, StorageError> {
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

async fn derived_backend_statuses(
    plan: &RetrievalPlan,
    graph_version: GraphVersion,
) -> Vec<crate::domain::RetrievalBackendStatus> {
    let request = DerivedRetrievalRequest {
        query: plan.query.clone(),
        source_scope: plan.source_scope.clone(),
        graph_version,
        limit: plan.limit,
    };
    let mut statuses = Vec::new();
    for adapter in crate::retrieval::phase1_unavailable_adapters() {
        match adapter.search(request.clone()).await {
            Ok(outcome) => statuses.push(outcome.status),
            Err(error) => statuses.push(error.status),
        }
    }

    statuses
}

fn service_definition_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "relay-knowledge-service.xml"
    } else if cfg!(target_os = "macos") {
        "com.coolplayagent.relay-knowledge.plist"
    } else {
        "relay-knowledge.service"
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
mod tests {
    use super::*;
    use crate::{
        api::{IngestEvidence, InterfaceKind},
        domain::{FreshnessPolicy, IndexKind, IndexState},
        env::PlatformKind,
        storage::{KnowledgeStore, SqliteGraphStore},
    };

    #[tokio::test]
    async fn status_includes_foundational_runtime_configuration() {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", "/home/alice"),
                ("TMPDIR", "/tmp"),
                ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
                ("RELAY_KNOWLEDGE_HTTP_BIND", "127.0.0.1:9000"),
                ("HTTPS_PROXY", "https://proxy.internal:8443"),
                ("NO_PROXY", "localhost,.internal"),
                ("SSL_VERIFY", "false"),
                ("RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH", "42"),
            ],
        )
        .expect("environment should parse");
        let service = service_with_environment(&environment).await;
        let context = RequestContext::with_ids(InterfaceKind::Cli, "req", "trace");

        let response = service
            .project_status(context)
            .await
            .expect("status should load");

        assert_eq!(response.runtime.config_dir, "/srv/relay/config");
        assert_eq!(response.runtime.data_dir, "/srv/relay/data");
        assert_eq!(response.runtime.http_bind, "127.0.0.1:9000");
        assert!(response.runtime.http_proxy_configured);
        assert_eq!(response.runtime.http_no_proxy_rules, 2);
        assert!(!response.runtime.http_ssl_verify);
        assert_eq!(response.runtime.qos_max_queue_depth, 42);
    }

    #[tokio::test]
    async fn status_reflects_refreshed_network_environment() {
        let initial_environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", "/home/alice"),
                ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ],
        )
        .expect("environment should parse");
        let service = service_with_environment(&initial_environment).await;

        let refreshed_environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HTTP_PROXY", "http://proxy.internal:8080"),
                ("SSL_VERIFY", "false"),
                ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "4"),
            ],
        )
        .expect("environment should parse");

        service
            .refresh_network_from_environment(&refreshed_environment)
            .await
            .expect("network refresh should succeed");
        let response = service
            .project_status(RequestContext::with_ids(InterfaceKind::Cli, "req", "trace"))
            .await
            .expect("status should load");

        assert!(response.runtime.http_proxy_configured);
        assert!(!response.runtime.http_ssl_verify);
        assert_eq!(response.runtime.qos_max_in_flight_requests, 4);
    }

    #[tokio::test]
    async fn project_status_reports_current_graph_version() {
        let service = service_with_memory_store().await;
        service
            .ingest(
                ingest_request(vec![ingest_evidence(
                    "ev-status",
                    "Project status tracks graph versions",
                    Vec::new(),
                )]),
                RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
            )
            .await
            .expect("ingest should succeed");

        let response = service
            .project_status(RequestContext::with_ids(
                InterfaceKind::Cli,
                "req-status",
                "trace-status",
            ))
            .await
            .expect("status should load");

        assert_eq!(response.metadata.graph_version, 1);
        assert_eq!(response.metadata.trace_id, "trace-status");
    }

    #[tokio::test]
    async fn ingest_commits_graph_and_refreshes_all_indexes() {
        let service = service_with_memory_store().await;
        let context = RequestContext::with_ids(InterfaceKind::Cli, "req", "trace");

        let response = service
            .ingest(
                ingest_request(vec![ingest_evidence(
                    "ev-1",
                    "Hybrid retrieval uses BM25 and vector indexes",
                    vec!["BM25".to_owned(), "Vector".to_owned()],
                )]),
                context,
            )
            .await
            .expect("ingest should succeed");

        assert_eq!(response.metadata.graph_version, 1);
        assert!(!response.metadata.stale);
        assert_eq!(response.receipt.evidence_count, 1);
        assert_eq!(response.indexes.len(), 3);
        assert!(
            response
                .indexes
                .iter()
                .all(|status| status.state == IndexState::Fresh)
        );
    }

    #[tokio::test]
    async fn retrieve_context_reports_results_and_index_freshness() {
        let service = service_with_memory_store().await;
        let context = RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest");
        service
            .ingest(
                ingest_request(vec![ingest_evidence(
                    "ev-1",
                    "Rust async services isolate blocking SQLite work",
                    vec!["Rust".to_owned()],
                )]),
                context,
            )
            .await
            .expect("ingest should succeed");

        let response = service
            .retrieve_context(
                HybridRetrievalRequest {
                    query: "SQLite".to_owned(),
                    source_scope: Some("docs".to_owned()),
                    limit: 5,
                    freshness: FreshnessPolicy::WaitUntilFresh,
                },
                RequestContext::with_ids(InterfaceKind::Web, "req-query", "trace-query"),
            )
            .await
            .expect("query should succeed");

        assert_eq!(response.metadata.trace_id, "trace-query");
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].evidence_id, "ev-1");
        assert_eq!(response.context_pack.items.len(), 1);
        assert_eq!(
            response.context_pack.freshness,
            FreshnessPolicy::WaitUntilFresh
        );
        assert!(!response.truncated);
        assert_eq!(response.fusion.algorithm, "reciprocal_rank_fusion");
        assert!(
            response.results[0]
                .ranking
                .iter()
                .any(|signal| signal.source == crate::domain::RetrieverSource::Bm25)
        );
        assert!(!response.metadata.stale);
        assert_eq!(
            response
                .indexes
                .iter()
                .map(|status| status.kind)
                .collect::<Vec<_>>(),
            IndexKind::ALL
        );
    }

    #[tokio::test]
    async fn wait_until_fresh_query_does_not_increment_fresh_index_versions() {
        let service = service_with_memory_store().await;
        service
            .ingest(
                ingest_request(vec![ingest_evidence(
                    "ev-fresh",
                    "Fresh indexes should not refresh on read",
                    vec!["Index".to_owned()],
                )]),
                RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
            )
            .await
            .expect("ingest should succeed");

        let first = retrieve_wait_until_fresh(&service, "req-query-1").await;
        let second = retrieve_wait_until_fresh(&service, "req-query-2").await;

        assert_eq!(first.metadata.index_version, Some(1));
        assert_eq!(second.metadata.index_version, Some(1));
        assert!(
            second
                .indexes
                .iter()
                .all(|status| status.index_version == 1)
        );
    }

    #[tokio::test]
    async fn retrieve_context_reports_truncated_context_pack_budget() {
        let service = service_with_memory_store().await;
        for index in 0..3 {
            service
                .ingest(
                    ingest_request(vec![ingest_evidence(
                        format!("ev-{index}"),
                        format!("Shared BM25 retrieval candidate {index}"),
                        vec!["BM25".to_owned()],
                    )]),
                    RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
                )
                .await
                .expect("ingest should succeed");
        }

        let response = service
            .retrieve_context(
                HybridRetrievalRequest {
                    query: "BM25".to_owned(),
                    source_scope: Some("docs".to_owned()),
                    limit: 2,
                    freshness: FreshnessPolicy::WaitUntilFresh,
                },
                RequestContext::with_ids(InterfaceKind::Cli, "req-query", "trace-query"),
            )
            .await
            .expect("query should succeed");

        assert!(response.truncated);
        assert!(response.context_pack.truncated);
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.budget_used.limit, 2);
        assert_eq!(response.budget_used.returned_count, 2);
        assert_eq!(response.budget_used.candidate_count, 3);
    }

    #[tokio::test]
    async fn service_status_reports_current_graph_version() {
        let service = service_with_memory_store().await;
        service
            .ingest(
                ingest_request(vec![ingest_evidence(
                    "ev-service",
                    "Service status tracks graph versions",
                    Vec::new(),
                )]),
                RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
            )
            .await
            .expect("ingest should succeed");

        let response = service
            .service_status(RequestContext::with_ids(
                InterfaceKind::Cli,
                "req-service",
                "trace-service",
            ))
            .await
            .expect("service status should load");

        assert_eq!(response.metadata.graph_version, 1);
        assert_eq!(response.metadata.trace_id, "trace-service");
    }

    #[tokio::test]
    async fn rejects_empty_retrieval_query() {
        let service = service_with_memory_store().await;

        let error = service
            .retrieve_context(
                HybridRetrievalRequest::new(" "),
                RequestContext::with_ids(InterfaceKind::Cli, "req", "trace"),
            )
            .await
            .expect_err("empty query should fail");

        assert_eq!(error.message, "query must not be empty");
    }

    #[tokio::test]
    async fn default_service_opens_sqlite_under_resolved_data_dir() {
        let root =
            std::env::temp_dir().join(format!("relay-knowledge-service-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", "/home/alice"),
                ("TMPDIR", "/tmp"),
                (
                    "RELAY_KNOWLEDGE_HOME",
                    root.to_str().expect("temp path is UTF-8"),
                ),
            ],
        )
        .expect("environment should parse");
        let service = RelayKnowledgeService::from_environment(&environment)
            .await
            .expect("service should compose");

        let health = service
            .health(RequestContext::with_ids(InterfaceKind::Cli, "req", "trace"))
            .await
            .expect("health should initialize storage");

        assert!(health.healthy);
        assert!(root.join("data").join("relay-knowledge.sqlite").exists());
    }

    async fn service_with_memory_store() -> RelayKnowledgeService {
        let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

        service_with_store(store).await
    }

    async fn service_with_store(store: Arc<dyn KnowledgeStore>) -> RelayKnowledgeService {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", "/home/alice"),
                ("TMPDIR", "/tmp"),
                ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ],
        )
        .expect("environment should parse");

        service_with_environment_and_store(&environment, store).await
    }

    async fn service_with_environment(environment: &EnvironmentConfig) -> RelayKnowledgeService {
        let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

        service_with_environment_and_store(environment, store).await
    }

    async fn service_with_environment_and_store(
        environment: &EnvironmentConfig,
        store: Arc<dyn KnowledgeStore>,
    ) -> RelayKnowledgeService {
        let runtime = RuntimeConfiguration::from_environment(environment)
            .await
            .expect("runtime should compose");

        RelayKnowledgeService::with_store(runtime, store)
    }

    fn ingest_request(evidence: Vec<IngestEvidence>) -> IngestRequest {
        IngestRequest {
            source_scope: "docs".to_owned(),
            evidence,
            relations: Vec::new(),
            claims: Vec::new(),
            events: Vec::new(),
        }
    }

    fn ingest_evidence(
        id: impl Into<String>,
        content: impl Into<String>,
        entity_labels: Vec<String>,
    ) -> IngestEvidence {
        IngestEvidence {
            id: Some(id.into()),
            source_path: None,
            span: None,
            confidence: None,
            status: None,
            content: content.into(),
            entity_labels,
            extraction: None,
        }
    }

    async fn retrieve_wait_until_fresh(
        service: &RelayKnowledgeService,
        request_id: &str,
    ) -> HybridRetrievalResponse {
        service
            .retrieve_context(
                HybridRetrievalRequest {
                    query: "Fresh".to_owned(),
                    source_scope: Some("docs".to_owned()),
                    limit: 5,
                    freshness: FreshnessPolicy::WaitUntilFresh,
                },
                RequestContext::with_ids(InterfaceKind::Cli, request_id, "trace-query"),
            )
            .await
            .expect("query should succeed")
    }
}
