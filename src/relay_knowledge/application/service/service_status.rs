use crate::{
    api::{
        ApiError, ApiMetadata, AuditSinkStatus, CodeIndexWorkerStatus, RequestContext,
        ServiceStatusResponse,
    },
    domain::{CodeIndexTaskQueueStatus, GraphVersion, ProposalState, ServiceOperatorState},
    project::PROJECT_NAME,
    storage::{FileIndexDiagnostics, IndexRefreshDiagnostics},
};

use super::{
    RelayKnowledgeService, file_index_diagnostics_or_default, service_definition_filename,
    storage_api_error,
};
use crate::application::{
    knowledge::index_refresh::{
        filter_outcome_to_read_models, index_refresh_outcome, reconcile_index_refreshes,
    },
    status::agent_protocol_status,
    worker::operations::overlay_worker_runtime,
};

#[derive(Clone, Copy)]
enum ServiceStatusRefreshMode {
    Reconcile,
    Observe,
}

impl RelayKnowledgeService {
    /// Returns the managed background service definition location and defaults.
    pub async fn service_status(
        &self,
        context: RequestContext,
    ) -> Result<ServiceStatusResponse, ApiError> {
        self.service_status_with_refresh_mode(context, ServiceStatusRefreshMode::Reconcile)
            .await
    }

    /// Returns control-plane service diagnostics without opening cold storage.
    pub async fn read_only_service_status(
        &self,
        context: RequestContext,
    ) -> Result<ServiceStatusResponse, ApiError> {
        if self.storage.ready_store().is_none() {
            return Ok(self.storage_free_service_status(context).await);
        }

        self.service_status_with_refresh_mode(context, ServiceStatusRefreshMode::Observe)
            .await
    }

    async fn service_status_with_refresh_mode(
        &self,
        context: RequestContext,
        refresh_mode: ServiceStatusRefreshMode,
    ) -> Result<ServiceStatusResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let index_refresh = match refresh_mode {
            ServiceStatusRefreshMode::Reconcile => {
                reconcile_index_refreshes(&store, graph_version, &self.runtime.retrieval).await?
            }
            ServiceStatusRefreshMode::Observe => {
                filter_outcome_to_read_models(
                    index_refresh_outcome(&store).await?,
                    &self.runtime.retrieval,
                )
                .diagnostics
            }
        };
        let file_index = file_index_diagnostics_or_default(&store).await?;
        let storage = self.storage_topology_diagnostics().await;
        let operator = store
            .service_operator_status()
            .await
            .map_err(storage_api_error)?;
        let workers = overlay_worker_runtime(
            store.worker_statuses().await.map_err(storage_api_error)?,
            &self.runtime.workers,
        );
        let code_index_workers = match refresh_mode {
            ServiceStatusRefreshMode::Reconcile => self.code_index_worker_status(&store).await?,
            ServiceStatusRefreshMode::Observe => {
                self.read_only_code_index_worker_status(&store).await?
            }
        };
        let proposal_backlog = store
            .proposal_count(Some(ProposalState::Proposed))
            .await
            .map_err(storage_api_error)?;
        let audit_event_count = store.audit_event_count().await.map_err(storage_api_error)?;

        Ok(self.service_status_response(ServiceStatusParts {
            context,
            graph_version,
            storage,
            index_refresh,
            file_index,
            operator,
            workers,
            code_index_workers,
            proposal_backlog,
            audit_sink: AuditSinkStatus {
                durable: true,
                event_count: audit_event_count,
                last_error: None,
            },
        }))
    }

    async fn storage_free_service_status(&self, context: RequestContext) -> ServiceStatusResponse {
        let operator = crate::domain::ServiceOperatorStatus {
            state: ServiceOperatorState::Disabled,
            silent_updates_enabled: self.runtime.workers.silent_updates_enabled,
            allowed_scopes: Vec::new(),
            last_run_at_ms: None,
            next_retry_at_ms: None,
            last_error: None,
            updated_at_ms: 0,
        };

        self.service_status_response(ServiceStatusParts {
            context,
            graph_version: GraphVersion::ZERO,
            storage: self.storage_topology_diagnostics().await,
            index_refresh: IndexRefreshDiagnostics::default(),
            file_index: FileIndexDiagnostics::default(),
            operator,
            workers: overlay_worker_runtime(Vec::new(), &self.runtime.workers),
            code_index_workers: CodeIndexWorkerStatus::from_queue(
                self.runtime.workers.code_index_max_in_flight,
                CodeIndexTaskQueueStatus::default(),
            ),
            proposal_backlog: 0,
            audit_sink: AuditSinkStatus {
                durable: true,
                event_count: 0,
                last_error: Some(
                    "audit event count not sampled because storage is not open".to_owned(),
                ),
            },
        })
    }

    fn service_status_response(&self, parts: ServiceStatusParts) -> ServiceStatusResponse {
        let ServiceStatusParts {
            context,
            graph_version,
            storage,
            index_refresh,
            file_index,
            operator,
            workers,
            code_index_workers,
            proposal_backlog,
            audit_sink,
        } = parts;

        ServiceStatusResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            service_name: PROJECT_NAME.to_owned(),
            mode: operator.state.as_str().to_owned(),
            background_enabled: operator.state != ServiceOperatorState::Disabled,
            silent_updates_enabled: operator.silent_updates_enabled,
            service_definition_path: self
                .runtime
                .paths
                .service_dir
                .join(service_definition_filename())
                .display()
                .to_string(),
            storage,
            index_refresh,
            file_index,
            agent_protocols: agent_protocol_status(&self.runtime),
            operator,
            workers,
            code_index_workers,
            proposal_backlog,
            audit_sink,
        }
    }
}

struct ServiceStatusParts {
    context: RequestContext,
    graph_version: GraphVersion,
    storage: crate::api::StorageTopologyDiagnostics,
    index_refresh: IndexRefreshDiagnostics,
    file_index: FileIndexDiagnostics,
    operator: crate::domain::ServiceOperatorStatus,
    workers: Vec<crate::domain::WorkerStatus>,
    code_index_workers: CodeIndexWorkerStatus,
    proposal_backlog: usize,
    audit_sink: AuditSinkStatus,
}
