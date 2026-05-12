use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use crate::{
    api::{
        ApiError, ApiMetadata, GraphInspectionRequest, GraphInspectionResponse, HealthResponse,
        HybridRetrievalRequest, HybridRetrievalResponse, IndexRefreshRequest, IndexRefreshResponse,
        IngestRequest, IngestResponse, ProjectStatusResponse, RequestContext,
        ServiceStatusResponse,
    },
    domain::{
        EvidenceRecord, FreshnessPolicy, GraphMutationBatch, GraphVersion, IndexKind, IndexStatus,
        RetrievalMode, SourceScope,
    },
    env::EnvironmentConfig,
    indexing::IndexRefreshPlan,
    project::PROJECT_NAME,
    retrieval::RetrievalPlan,
    storage::{GraphSearchRequest, KnowledgeStore, SqliteGraphStore, StorageError},
};

use super::{RuntimeConfiguration, RuntimeConfigurationError, status::runtime_status};

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
        let source_scope = SourceScope::parse(request.source_scope)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let mut records = Vec::with_capacity(request.evidence.len());
        for evidence in request.evidence {
            let content = evidence.content.trim().to_owned();
            let id = evidence
                .id
                .unwrap_or_else(|| generated_evidence_id(source_scope.as_str(), &content));
            let record =
                EvidenceRecord::new(id, source_scope.clone(), content, evidence.entity_labels)
                    .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
            records.push(record);
        }

        let batch = GraphMutationBatch::new(records)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let receipt = store
            .commit_mutation_batch(batch)
            .await
            .map_err(storage_api_error)?;
        let (indexes, metadata, index_refresh_error) =
            match refresh_index_kinds(&store, IndexKind::ALL, receipt.graph_version).await {
                Ok(indexes) => {
                    let metadata = metadata_for_indexes(&context, receipt.graph_version, &indexes);

                    (indexes, metadata, None)
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
        let degraded_reason = if plan.freshness == FreshnessPolicy::GraphOnly {
            retrieval_mode = RetrievalMode::GraphOnly;
            Some("graph_only freshness policy selected".to_owned())
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
            (plan.freshness == FreshnessPolicy::AllowStale && stale)
                .then(|| "one or more indexes are behind the graph version".to_owned())
        };
        let results = store
            .search(GraphSearchRequest {
                query: plan.query,
                source_scope: plan.source_scope,
                graph_version,
                limit: plan.limit,
            })
            .await
            .map_err(storage_api_error)?;

        Ok(HybridRetrievalResponse {
            metadata,
            retrieval_mode,
            results,
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
        let plan = IndexRefreshPlan::from_requested(request.kinds);
        let indexes = refresh_index_kinds(&store, plan.into_kinds(), graph_version).await?;
        let metadata = metadata_for_indexes(&context, graph_version, &indexes);

        Ok(IndexRefreshResponse { metadata, indexes })
    }

    /// Returns service and data health for diagnostics.
    pub async fn health(&self, context: RequestContext) -> Result<HealthResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph = store.inspect_graph().await.map_err(storage_api_error)?;
        let indexes = store.index_statuses().await.map_err(storage_api_error)?;
        let healthy = indexes
            .iter()
            .all(|status| !status.is_stale_for(graph.graph_version));

        Ok(HealthResponse {
            metadata: metadata_for_indexes(&context, graph.graph_version, &indexes),
            healthy,
            graph,
            indexes,
            runtime: runtime_status(&self.runtime),
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
        })
    }
}

#[derive(Clone)]
struct StorageProvider {
    path: Option<PathBuf>,
    ready: Arc<OnceLock<Arc<dyn KnowledgeStore>>>,
}

impl StorageProvider {
    fn sqlite(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ready: Arc::new(OnceLock::new()),
        }
    }

    fn ready(store: Arc<dyn KnowledgeStore>) -> Self {
        let ready = OnceLock::new();
        let _ = ready.set(store);

        Self {
            path: None,
            ready: Arc::new(ready),
        }
    }

    async fn get(&self) -> Result<Arc<dyn KnowledgeStore>, StorageError> {
        if let Some(store) = self.ready.get() {
            return Ok(Arc::clone(store));
        }

        let Some(path) = self.path.clone() else {
            return Err(StorageError::InvalidInput(
                "storage provider was not initialized".to_owned(),
            ));
        };
        let store = tokio::task::spawn_blocking(move || {
            SqliteGraphStore::open(path).map(|store| Arc::new(store) as Arc<dyn KnowledgeStore>)
        })
        .await??;

        if self.ready.set(Arc::clone(&store)).is_ok() {
            return Ok(store);
        }

        self.ready.get().cloned().ok_or_else(|| {
            StorageError::InvalidInput("storage provider was not initialized".to_owned())
        })
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

async fn refresh_index_kinds(
    store: &Arc<dyn KnowledgeStore>,
    kinds: impl IntoIterator<Item = IndexKind>,
    graph_version: GraphVersion,
) -> Result<Vec<IndexStatus>, ApiError> {
    let mut statuses = Vec::new();
    for kind in kinds {
        statuses.push(
            store
                .mark_refresh_complete(kind, graph_version)
                .await
                .map_err(storage_api_error)?,
        );
    }

    Ok(statuses)
}

fn metadata_for_indexes(
    context: &RequestContext,
    graph_version: GraphVersion,
    indexes: &[IndexStatus],
) -> ApiMetadata {
    let latest_index_version = indexes.iter().map(|status| status.index_version).max();
    let lowest_indexed_graph_version = indexes
        .iter()
        .map(|status| status.indexed_graph_version)
        .min();
    let stale = indexes
        .iter()
        .any(|status| status.is_stale_for(graph_version));

    ApiMetadata::indexed(
        context,
        graph_version,
        latest_index_version,
        lowest_indexed_graph_version,
        stale,
    )
}

fn generated_evidence_id(scope: &str, content: &str) -> String {
    let mut input = Vec::with_capacity(scope.len() + content.len() + 16);
    input.extend_from_slice(&(scope.len() as u64).to_le_bytes());
    input.extend_from_slice(scope.as_bytes());
    input.extend_from_slice(&(content.len() as u64).to_le_bytes());
    input.extend_from_slice(content.as_bytes());

    format!("evidence:{:016x}", stable_hash64(&input))
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
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
mod tests {
    use super::*;
    use crate::{
        api::{IngestEvidence, InterfaceKind},
        domain::{
            EvidenceRecord, FreshnessPolicy, GraphMutationBatch, IndexKind, IndexState, SourceScope,
        },
        env::PlatformKind,
        storage::{GraphStore, KnowledgeStore, SqliteGraphStore},
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
                IngestRequest {
                    source_scope: "docs".to_owned(),
                    evidence: vec![IngestEvidence {
                        id: Some("ev-status".to_owned()),
                        content: "Project status tracks graph versions".to_owned(),
                        entity_labels: Vec::new(),
                    }],
                },
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
                IngestRequest {
                    source_scope: "docs".to_owned(),
                    evidence: vec![IngestEvidence {
                        id: Some("ev-1".to_owned()),
                        content: "Hybrid retrieval uses BM25 and vector indexes".to_owned(),
                        entity_labels: vec!["BM25".to_owned(), "Vector".to_owned()],
                    }],
                },
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
                IngestRequest {
                    source_scope: "docs".to_owned(),
                    evidence: vec![IngestEvidence {
                        id: Some("ev-1".to_owned()),
                        content: "Rust async services isolate blocking SQLite work".to_owned(),
                        entity_labels: vec!["Rust".to_owned()],
                    }],
                },
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
                IngestRequest {
                    source_scope: "docs".to_owned(),
                    evidence: vec![IngestEvidence {
                        id: Some("ev-fresh".to_owned()),
                        content: "Fresh indexes should not refresh on read".to_owned(),
                        entity_labels: vec!["Index".to_owned()],
                    }],
                },
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
    async fn health_metadata_preserves_stale_index_state() {
        let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
        let service = service_with_store(store.clone()).await;
        let evidence = EvidenceRecord::new(
            "ev-stale",
            SourceScope::parse("docs").expect("scope should parse"),
            "Direct storage writes leave indexes stale",
            vec!["Index".to_owned()],
        )
        .expect("evidence should validate");
        let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");
        store
            .commit_mutation_batch(batch)
            .await
            .expect("commit should succeed");

        let health = service
            .health(RequestContext::with_ids(
                InterfaceKind::Cli,
                "req-health",
                "trace-health",
            ))
            .await
            .expect("health should load");

        assert!(!health.healthy);
        assert!(health.metadata.stale);
        assert_eq!(health.metadata.graph_version, 1);
    }

    #[tokio::test]
    async fn service_status_reports_current_graph_version() {
        let service = service_with_memory_store().await;
        service
            .ingest(
                IngestRequest {
                    source_scope: "docs".to_owned(),
                    evidence: vec![IngestEvidence {
                        id: Some("ev-service".to_owned()),
                        content: "Service status tracks graph versions".to_owned(),
                        entity_labels: Vec::new(),
                    }],
                },
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

    #[tokio::test]
    async fn concurrent_storage_initialization_returns_canonical_store() {
        let root = std::env::temp_dir().join(format!(
            "relay-knowledge-storage-race-{}",
            std::process::id()
        ));
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
        let service = Arc::new(
            RelayKnowledgeService::from_environment(&environment)
                .await
                .expect("service should compose"),
        );
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let service = Arc::clone(&service);
            tasks.push(tokio::spawn(async move {
                service
                    .storage
                    .get()
                    .await
                    .expect("store should initialize")
            }));
        }

        let mut stores = Vec::new();
        for task in tasks {
            stores.push(task.await.expect("task should join"));
        }

        let first = stores.first().expect("stores should exist");
        assert!(stores.iter().all(|store| Arc::ptr_eq(first, store)));
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
