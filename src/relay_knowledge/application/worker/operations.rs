use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use crate::{
    api::{
        ApiError, ApiMetadata, AuditQueryApiRequest, AuditQueryResponse, IngestRequest,
        InterfaceKind, ProposalDecisionApiRequest, ProposalDecisionResponse,
        ProposalListApiRequest, ProposalListResponse, ProposalShowResponse, RequestContext,
        ServiceDefinitionWriteResponse, ServiceOperatorResponse, ServicePlanRequest,
        ServicePlanResponse, WorkerRunRequest, WorkerRunResponse, WorkerStatusRequest,
        WorkerStatusResponse,
    },
    domain::{
        AuditStatus, EvidenceModality, EvidenceRecord, GraphVersion, ProposalState,
        ServiceManagerAction, ServiceOperatorState, WorkerBackendState, WorkerKind, WorkerStatus,
        WorkerTaskRecord, normalize_actor,
    },
    storage::{
        AuditQueryRequest, KnowledgeStore, NewAuditEvent, NewProposal, ProposalDecision,
        ProposalListRequest, ServiceOperatorUpdate, StorageError, WorkerTaskClaimRequest,
        WorkerTaskCompletion, WorkerTaskSeed,
    },
};

use super::proposals::{fallback_proposal, proposal_from_worker_response, worker_request_payload};
use crate::application::{
    knowledge::{
        index_refresh::{metadata_for_indexes, refresh_index_kinds},
        ingest::mutation_batch_from_request,
    },
    service::RelayKnowledgeService,
};

const WORKER_LEASE_MS: u64 = 30_000;
const WORKER_MAX_ATTEMPTS: u32 = 3;
const PROPOSAL_LIST_LIMIT: usize = 50;
const AUDIT_QUERY_LIMIT: usize = 100;

struct AuditRecordInput<'a> {
    operation: &'static str,
    context: &'a RequestContext,
    status: AuditStatus,
    actor: Option<String>,
    source_scope: Option<String>,
    graph_version: GraphVersion,
    detail: serde_json::Value,
}

impl RelayKnowledgeService {
    /// Returns worker queue and backend readiness status.
    pub async fn worker_status(
        &self,
        request: WorkerStatusRequest,
        context: RequestContext,
    ) -> Result<WorkerStatusResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let mut workers = overlay_worker_runtime(
            store.worker_statuses().await.map_err(storage_api_error)?,
            &self.runtime.workers,
        );
        if let Some(kind) = request.kind {
            workers.retain(|status| status.kind == kind);
        }
        self.record_audit(
            &store,
            AuditRecordInput {
                operation: "worker.status",
                context: &context,
                status: AuditStatus::Completed,
                actor: None,
                source_scope: None,
                graph_version,
                detail: json!({"worker_count": workers.len()}),
            },
        )
        .await;

        Ok(WorkerStatusResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            workers,
        })
    }

    /// Runs one bounded worker task in the foreground.
    pub async fn run_worker_once(
        &self,
        request: WorkerRunRequest,
        context: RequestContext,
    ) -> Result<WorkerRunResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let lease_owner = format!("worker-run-once-{}", std::process::id());
        let Some(task) = store
            .claim_worker_task(WorkerTaskClaimRequest {
                kind: request.kind,
                lease_owner: lease_owner.clone(),
                lease_duration_ms: WORKER_LEASE_MS,
                max_attempts: WORKER_MAX_ATTEMPTS,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?
        else {
            let workers = overlay_worker_runtime(
                store.worker_statuses().await.map_err(storage_api_error)?,
                &self.runtime.workers,
            );
            return Ok(WorkerRunResponse {
                metadata: ApiMetadata::graph_only(&context, graph_version),
                task: None,
                proposals: Vec::new(),
                workers,
                degraded_reason: None,
            });
        };

        let (proposal, degraded_reason) = self.proposal_from_worker_task(&task).await?;
        let proposal = store
            .insert_proposal(proposal)
            .await
            .map_err(storage_api_error)?;
        let completed = store
            .complete_worker_task(WorkerTaskCompletion {
                task_id: task.task_id.clone(),
                lease_owner,
                attempt_count: task.attempt_count,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?;
        let workers = overlay_worker_runtime(
            store.worker_statuses().await.map_err(storage_api_error)?,
            &self.runtime.workers,
        );
        self.record_audit(
            &store,
            AuditRecordInput {
                operation: "worker.run_once",
                context: &context,
                status: AuditStatus::Completed,
                actor: None,
                source_scope: Some(task.source_scope.clone()),
                graph_version,
                detail: json!({"task_id": task.task_id, "proposal_id": proposal.proposal_id}),
            },
        )
        .await;

        Ok(WorkerRunResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            task: Some(completed),
            proposals: vec![proposal],
            workers,
            degraded_reason,
        })
    }

    /// Lists proposals awaiting or past manual lifecycle decisions.
    pub async fn list_proposals(
        &self,
        request: ProposalListApiRequest,
        context: RequestContext,
    ) -> Result<ProposalListResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let proposals = store
            .list_proposals(ProposalListRequest {
                state: request.state,
                limit: bounded_limit(request.limit, PROPOSAL_LIST_LIMIT),
            })
            .await
            .map_err(storage_api_error)?;

        Ok(ProposalListResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            proposals,
        })
    }

    /// Shows one proposal and its conflict details.
    pub async fn show_proposal(
        &self,
        proposal_id: String,
        context: RequestContext,
    ) -> Result<ProposalShowResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let proposal = store
            .proposal_by_id(proposal_id.clone())
            .await
            .map_err(storage_api_error)?
            .ok_or_else(|| {
                ApiError::invalid_argument(format!("proposal '{proposal_id}' not found"))
            })?;
        let conflicts = store
            .proposal_conflicts(proposal_id)
            .await
            .map_err(storage_api_error)?;
        let payload = proposal.payload_value();

        Ok(ProposalShowResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            proposal,
            conflicts,
            payload,
        })
    }

    /// Accepts a proposal by committing its payload through the graph pipeline.
    pub async fn accept_proposal(
        &self,
        proposal_id: String,
        request: ProposalDecisionApiRequest,
        context: RequestContext,
    ) -> Result<ProposalDecisionResponse, ApiError> {
        let actor = normalize_actor(request.actor)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let proposal = store
            .proposal_by_id(proposal_id.clone())
            .await
            .map_err(storage_api_error)?
            .ok_or_else(|| {
                ApiError::invalid_argument(format!("proposal '{proposal_id}' not found"))
            })?;
        let ingest = serde_json::from_str::<IngestRequest>(&proposal.payload_json)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let batch = mutation_batch_from_request(ingest)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let evidence = batch.evidence.clone();
        let receipt = store
            .commit_mutation_batch(batch)
            .await
            .map_err(storage_api_error)?;
        self.queue_worker_tasks_for_evidence(&store, &evidence, receipt.graph_version)
            .await?;
        let decided = store
            .decide_proposal(ProposalDecision {
                proposal_id,
                next_state: ProposalState::Accepted,
                actor,
                reason: request.reason,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?;
        let (metadata, index_refresh_error) = match refresh_index_kinds(
            &store,
            crate::domain::IndexKind::ALL,
            receipt.graph_version,
            &self.runtime.retrieval,
        )
        .await
        {
            Ok(outcome) => (
                metadata_for_indexes(&context, receipt.graph_version, &outcome.indexes),
                None,
            ),
            Err(error) => (
                ApiMetadata::indexed(&context, receipt.graph_version, None, None, true),
                Some(error.message),
            ),
        };

        Ok(ProposalDecisionResponse {
            metadata,
            proposal: decided,
            receipt: Some(receipt),
            index_refresh_error,
        })
    }

    /// Rejects or supersedes a proposal without mutating graph facts.
    pub async fn decide_proposal_without_commit(
        &self,
        proposal_id: String,
        next_state: ProposalState,
        request: ProposalDecisionApiRequest,
        context: RequestContext,
    ) -> Result<ProposalDecisionResponse, ApiError> {
        if next_state == ProposalState::Accepted {
            return Err(ApiError::invalid_argument(
                "accept decisions must use accept_proposal".to_owned(),
            ));
        }
        let actor = normalize_actor(request.actor)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let proposal = store
            .decide_proposal(ProposalDecision {
                proposal_id,
                next_state,
                actor,
                reason: request.reason,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?;

        Ok(ProposalDecisionResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            proposal,
            receipt: None,
            index_refresh_error: None,
        })
    }

    /// Queries the durable audit sink.
    pub async fn query_audit(
        &self,
        request: AuditQueryApiRequest,
        context: RequestContext,
    ) -> Result<AuditQueryResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let events = store
            .query_audit_events(AuditQueryRequest {
                operation: request.operation,
                limit: bounded_limit(request.limit, AUDIT_QUERY_LIMIT),
            })
            .await
            .map_err(storage_api_error)?;

        Ok(AuditQueryResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            events,
        })
    }

    /// Generates a service-manager plan without executing privileged commands.
    pub async fn service_plan(
        &self,
        request: ServicePlanRequest,
        context: RequestContext,
    ) -> Result<ServicePlanResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let plan = self
            .render_service_plan_for_request(&request)
            .map_err(ApiError::invalid_argument)?;
        let execution = if request.execute {
            Some(self.execute_service_plan(&plan).await?)
        } else {
            None
        };

        Ok(ServicePlanResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            plan,
            execution,
        })
    }

    /// Writes the generated service definition into the service directory.
    pub async fn write_service_definition(
        &self,
        context: RequestContext,
    ) -> Result<ServiceDefinitionWriteResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let plan = self
            .render_service_plan_for_request(&ServicePlanRequest {
                action: ServiceManagerAction::Install,
                dry_run: true,
                execute: false,
                target_version: None,
                install_dir: None,
            })
            .map_err(ApiError::invalid_argument)?;
        self.write_service_definition_from_plan(&plan).await?;

        Ok(ServiceDefinitionWriteResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            plan,
            written: true,
        })
    }

    /// Returns persisted silent-update operator status.
    pub async fn service_operator_status(
        &self,
        context: RequestContext,
    ) -> Result<ServiceOperatorResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let operator = store
            .service_operator_status()
            .await
            .map_err(storage_api_error)?;

        Ok(ServiceOperatorResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            operator,
        })
    }

    /// Pauses or resumes the silent-update operator.
    pub async fn set_service_operator_state(
        &self,
        state: ServiceOperatorState,
        context: RequestContext,
    ) -> Result<ServiceOperatorResponse, ApiError> {
        let store = self.storage.get().await.map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let current = store
            .service_operator_status()
            .await
            .map_err(storage_api_error)?;
        let operator = store
            .update_service_operator(ServiceOperatorUpdate {
                state,
                silent_updates_enabled: self.runtime.workers.silent_updates_enabled,
                allowed_scopes: current.allowed_scopes,
                last_error: current.last_error,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?;

        Ok(ServiceOperatorResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            operator,
        })
    }

    pub(in crate::application) async fn queue_worker_tasks_for_evidence(
        &self,
        store: &Arc<dyn KnowledgeStore>,
        evidence: &[EvidenceRecord],
        graph_version: GraphVersion,
    ) -> Result<(), ApiError> {
        let now_ms = now_millis();
        let seeds = evidence
            .iter()
            .flat_map(|record| worker_task_seeds(record, graph_version, now_ms))
            .collect::<Vec<_>>();
        if seeds.is_empty() {
            return Ok(());
        }
        store
            .queue_worker_tasks(seeds)
            .await
            .map(|_| ())
            .map_err(storage_api_error)
    }

    async fn proposal_from_worker_task(
        &self,
        task: &WorkerTaskRecord,
    ) -> Result<(NewProposal, Option<String>), ApiError> {
        let fallback = fallback_proposal(task, WORKER_LEASE_MS, WORKER_MAX_ATTEMPTS)
            .map_err(ApiError::invalid_argument)?;
        let Some(endpoint) = self.runtime.workers.endpoint_for(task.kind) else {
            return Ok((fallback, None));
        };
        let network = self.runtime.network.current();
        let timeout_ms =
            u64::try_from(network.http.request_timeout.as_millis()).unwrap_or(u64::MAX);
        let payload = worker_request_payload(
            task,
            timeout_ms,
            WORKER_LEASE_MS,
            WORKER_MAX_ATTEMPTS,
            self.runtime.workers.max_in_flight,
        );
        let response = crate::net::http::post_json(&network.http, endpoint, &payload).await;
        match response {
            Ok(value) => match proposal_from_worker_response(
                task,
                value,
                WORKER_LEASE_MS,
                WORKER_MAX_ATTEMPTS,
            ) {
                Ok(proposal) => Ok((proposal, None)),
                Err(_) => Ok((
                    fallback,
                    Some(
                        "worker response did not match proposal contract; deterministic fallback used"
                            .to_owned(),
                    ),
                )),
            },
            Err(error) => Ok((
                fallback,
                Some(format!("external worker unavailable: {error}; deterministic fallback used")),
            )),
        }
    }

    async fn record_audit(&self, store: &Arc<dyn KnowledgeStore>, input: AuditRecordInput<'_>) {
        let detail_json = serde_json::to_string(&input.detail).unwrap_or_else(|_| "{}".to_owned());
        let _ = store
            .insert_audit_event(NewAuditEvent {
                operation: input.operation.to_owned(),
                interface: interface_label(input.context.interface).to_owned(),
                request_id: input.context.request_id.clone(),
                trace_id: input.context.trace_id.clone(),
                status: input.status,
                actor: input.actor,
                source_scope: input.source_scope,
                graph_version: input.graph_version.get(),
                detail_json,
                message: None,
                now_ms: now_millis(),
            })
            .await;
    }
}

fn worker_task_seeds(
    record: &EvidenceRecord,
    graph_version: GraphVersion,
    now_ms: u64,
) -> Vec<WorkerTaskSeed> {
    let kinds = match record.extraction.modality {
        EvidenceModality::TextSpan | EvidenceModality::OcrText | EvidenceModality::Caption => {
            vec![WorkerKind::Embedding, WorkerKind::Extractor]
        }
        EvidenceModality::ImageAsset => {
            vec![WorkerKind::Ocr, WorkerKind::Vision, WorkerKind::Embedding]
        }
        EvidenceModality::ImageEmbedding => Vec::new(),
        EvidenceModality::Table | EvidenceModality::LayoutRegion => vec![WorkerKind::Extractor],
    };

    kinds
        .into_iter()
        .map(|kind| {
            let payload = json!({
                "evidence_id": record.id,
                "source_scope": record.source_scope.as_str(),
                "modality": record.extraction.modality.as_str(),
                "source_path": record.source_path.as_deref(),
                "source_uri": record.extraction.source_uri.as_deref(),
                "source_hash": record.extraction.source_hash.as_deref(),
                "media_hash": record.extraction.media_hash.as_deref(),
            });
            WorkerTaskSeed {
                kind,
                source_scope: record.source_scope.as_str().to_owned(),
                evidence_id: Some(record.id.clone()),
                target_graph_version: graph_version,
                input_fingerprint: format!(
                    "{}:{}:{}",
                    kind.as_str(),
                    record.id,
                    graph_version.get()
                ),
                payload_json: serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned()),
                now_ms,
            }
        })
        .collect()
}

pub(in crate::application) fn overlay_worker_runtime(
    mut statuses: Vec<WorkerStatus>,
    runtime: &crate::application::WorkerRuntimeConfig,
) -> Vec<WorkerStatus> {
    if statuses.is_empty() {
        statuses = WorkerKind::ALL
            .into_iter()
            .map(|kind| WorkerStatus {
                kind,
                backend_state: WorkerBackendState::Fallback,
                endpoint_configured: false,
                queue_depth: 0,
                running_count: 0,
                retrying_count: 0,
                dead_letter_count: 0,
                last_error: None,
            })
            .collect();
    }
    for status in &mut statuses {
        status.endpoint_configured = runtime.endpoint_for(status.kind).is_some();
        status.backend_state = if status.dead_letter_count > 0 {
            WorkerBackendState::Degraded
        } else if status.endpoint_configured {
            WorkerBackendState::Configured
        } else {
            WorkerBackendState::Fallback
        };
    }

    statuses
}

fn bounded_limit(value: usize, default_limit: usize) -> usize {
    if value == 0 {
        default_limit
    } else {
        value.min(default_limit)
    }
}

fn storage_api_error(error: StorageError) -> ApiError {
    ApiError::storage_unavailable(error.to_string())
}

fn interface_label(interface: InterfaceKind) -> &'static str {
    match interface {
        InterfaceKind::Cli => "cli",
        InterfaceKind::Web => "web",
        InterfaceKind::Api => "api",
        InterfaceKind::Mcp => "mcp",
        InterfaceKind::Acp => "acp",
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}
