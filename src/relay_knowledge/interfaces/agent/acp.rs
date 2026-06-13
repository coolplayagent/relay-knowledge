use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::{
    api::{
        AgentProtocolKind, AgentRetrievalResult, ErrorKind, HybridRetrievalRequest, InterfaceKind,
        RequestContext, RuntimeIdentity,
    },
    application::{AgentRuntimeConfig, RelayKnowledgeService},
    domain::FreshnessPolicy,
    net::{
        NetworkRuntime,
        qos::{QosPermit, QosRuntime, RejectReason},
    },
    observability::AgentProtocolMetrics,
};

use super::{
    AgentAdapterError, AgentAdapterErrorKind, AgentAuditEvent, AgentAuditLog,
    AgentAuditQosDecision, AgentAuditSink, AgentAuditStatus, authorize_limit, authorize_scope,
};

/// Local ACP session adapter for resident relay-knowledge processes.
#[derive(Clone)]
pub struct LocalAcpSessionAdapter {
    service: RelayKnowledgeService,
    network: NetworkRuntime,
    agent: AgentRuntimeConfig,
    qos: QosRuntime,
    audit: AgentAuditLog,
    metrics: AgentProtocolMetrics,
    sessions: AcpSessionRegistry,
}

impl LocalAcpSessionAdapter {
    /// Creates an ACP local session adapter without opening sockets.
    pub fn new(
        service: RelayKnowledgeService,
        network: NetworkRuntime,
        agent: AgentRuntimeConfig,
    ) -> Self {
        let qos = network.qos_runtime();
        let metrics = service.observability().agent_metrics();
        let audit = if agent.audit_sink_enabled {
            AgentAuditSink::jsonl(service.agent_audit_log_path(), agent.audit_queue_depth)
                .map(AgentAuditLog::with_sink)
                .unwrap_or_default()
        } else {
            AgentAuditLog::default()
        };

        Self {
            service,
            network,
            agent,
            qos,
            audit,
            metrics,
            sessions: AcpSessionRegistry::default(),
        }
    }

    /// Returns the ACP initialize capability payload.
    pub fn initialize(&self) -> AcpInitializeResponse {
        AcpInitializeResponse {
            meta: AcpInitializeMeta {
                relay_knowledge: AcpRelayKnowledgeCapability {
                    graph_retrieval: true,
                    read_only: true,
                    supports_cancellation: true,
                    supports_index_refresh_permission: true,
                },
            },
        }
    }

    /// Creates a bounded local ACP session and captures untrusted client identity.
    pub fn new_session(&self, request: AcpSessionRequest) -> Result<AcpSession, AgentAdapterError> {
        let permit = self.admit_request()?;
        let session_id = generate_acp_id("acp-session")?;
        let record = AcpSessionRecord {
            client_name: normalized_optional(request.client_name),
            client_version: normalized_optional(request.client_version),
            actor_id: normalized_optional(request.actor_id),
        };
        self.sessions
            .insert_session(session_id.clone(), record.clone());
        drop(permit);

        Ok(AcpSession {
            session_id: session_id.clone(),
            runtime_identity: record.identity(&session_id, None),
            policy_id: "local-acp-policy".to_owned(),
            authorized_scope_count: self.agent.access_policy.allowed_scopes.len(),
        })
    }

    /// Runs an ACP prompt turn, returning progress updates and a context artifact.
    pub async fn prompt(
        &self,
        session_id: &str,
        mut request: AcpPromptRequest,
    ) -> AcpPromptResponse {
        let started = Instant::now();
        let request_id = request.request_id.take().unwrap_or_else(|| {
            generate_acp_id("acp-request").unwrap_or_else(|_| "acp-request-unavailable".to_owned())
        });
        let mut updates = vec![AcpSessionUpdate::pending(&request_id, "accepted")];
        let Some(session) = self.sessions.session(session_id) else {
            return failed_prompt(
                session_id,
                request_id,
                updates,
                AgentAdapterError::new(
                    AgentAdapterErrorKind::InvalidArgument,
                    "unknown ACP session",
                ),
                elapsed_millis(started),
            );
        };
        let permit = match self.admit_request() {
            Ok(permit) => permit,
            Err(error) => {
                self.record_audit(AcpAuditInput {
                    operation: "session/prompt",
                    request_id: &request_id,
                    session_id,
                    session: &session,
                    qos_decision: AgentAuditQosDecision::Rejected,
                    status: AgentAuditStatus::Failed,
                    source_scope: None,
                    freshness: None,
                    limit: None,
                    result_count: None,
                    truncated: false,
                    elapsed_ms: elapsed_millis(started),
                    error_kind: Some(error.kind.as_str()),
                });
                return failed_prompt(
                    session_id,
                    request_id,
                    updates,
                    error,
                    elapsed_millis(started),
                );
            }
        };
        updates.push(AcpSessionUpdate::in_progress(
            &request_id,
            "retrieval request mapped",
        ));

        let mapped = match map_prompt_request(&self.agent, request) {
            Ok(mapped) => mapped,
            Err(error) => {
                drop(permit);
                self.record_audit(AcpAuditInput {
                    operation: "session/prompt",
                    request_id: &request_id,
                    session_id,
                    session: &session,
                    qos_decision: AgentAuditQosDecision::Admitted,
                    status: AgentAuditStatus::Failed,
                    source_scope: None,
                    freshness: None,
                    limit: None,
                    result_count: None,
                    truncated: false,
                    elapsed_ms: elapsed_millis(started),
                    error_kind: Some(error.kind.as_str()),
                });
                return failed_prompt(
                    session_id,
                    request_id,
                    updates,
                    error,
                    elapsed_millis(started),
                );
            }
        };
        updates.push(AcpSessionUpdate::meta(
            &request_id,
            "freshness checked",
            json!({
                "relayKnowledge": {
                    "freshness": crate::api::freshness_label(mapped.freshness),
                    "source_scope": mapped.source_scope
                }
            }),
        ));

        let (mut cancellation, registration) = self
            .sessions
            .register_request(session_id, request_id.clone());
        let identity = session.identity(session_id, Some(request_id.clone()));
        let context = RequestContext::with_ids(
            InterfaceKind::Acp,
            request_id.clone(),
            format!("trace-acp-{request_id}"),
        );
        let service = self.service.clone();
        let request_timeout = Duration::from_millis(self.agent.access_policy.max_runtime_ms);
        let source_scope = mapped.source_scope.clone();
        let freshness = mapped.freshness;
        let limit = mapped.limit;
        let max_context_bytes = self.agent.access_policy.max_context_bytes;
        let retrieval = service.retrieve_context(mapped.into_retrieval_request(), context);

        let response = tokio::select! {
            result = tokio::time::timeout(request_timeout, retrieval) => {
                match result {
                    Ok(Ok(response)) => {
                        let result = AgentRetrievalResult::from_retrieval(
                            response,
                            identity,
                            max_context_bytes,
                            elapsed_millis(started),
                        );
                        let artifact_id = format!("relay-context:{session_id}:{request_id}");
                        updates.push(AcpSessionUpdate::meta(
                            &request_id,
                            "context ready",
                            json!({"relayKnowledge": {"artifact_id": artifact_id}}),
                        ));
                        updates.push(AcpSessionUpdate::completed(&request_id, "completed"));
                        self.record_audit(AcpAuditInput {
                            operation: "session/prompt",
                            request_id: &request_id,
                            session_id,
                            session: &session,
                            qos_decision: AgentAuditQosDecision::Admitted,
                            status: AgentAuditStatus::Completed,
                            source_scope: source_scope.as_deref(),
                            freshness: Some(crate::api::freshness_label(freshness)),
                            limit: Some(limit),
                            result_count: Some(result.results.len()),
                            truncated: result.truncated,
                            elapsed_ms: elapsed_millis(started),
                            error_kind: None,
                        });
                        AcpPromptResponse {
                            session_id: session_id.to_owned(),
                            request_id: request_id.clone(),
                            updates,
                            context_artifact: Some(AcpContextArtifact {
                                artifact_id,
                                result,
                            }),
                            stop_reason: AcpStopReason::Completed,
                            error: None,
                        }
                    }
                    Ok(Err(error)) => {
                        let adapter_error = AgentAdapterError::new(
                            api_error_kind(error.error_kind),
                            error.message,
                        );
                        self.record_audit(AcpAuditInput {
                            operation: "session/prompt",
                            request_id: &request_id,
                            session_id,
                            session: &session,
                            qos_decision: AgentAuditQosDecision::Admitted,
                            status: AgentAuditStatus::Failed,
                            source_scope: source_scope.as_deref(),
                            freshness: Some(crate::api::freshness_label(freshness)),
                            limit: Some(limit),
                            result_count: None,
                            truncated: false,
                            elapsed_ms: elapsed_millis(started),
                            error_kind: Some(adapter_error.kind.as_str()),
                        });
                        failed_prompt(session_id, request_id.clone(), updates, adapter_error, elapsed_millis(started))
                    }
                    Err(_) => {
                        self.qos.record_timed_out();
                        let adapter_error = AgentAdapterError::new(
                            AgentAdapterErrorKind::Timeout,
                            "ACP prompt exceeded max_runtime_ms",
                        );
                        self.record_audit(AcpAuditInput {
                            operation: "session/prompt",
                            request_id: &request_id,
                            session_id,
                            session: &session,
                            qos_decision: AgentAuditQosDecision::Admitted,
                            status: AgentAuditStatus::Failed,
                            source_scope: source_scope.as_deref(),
                            freshness: Some(crate::api::freshness_label(freshness)),
                            limit: Some(limit),
                            result_count: None,
                            truncated: false,
                            elapsed_ms: elapsed_millis(started),
                            error_kind: Some(adapter_error.kind.as_str()),
                        });
                        failed_prompt(session_id, request_id.clone(), updates, adapter_error, elapsed_millis(started))
                    }
                }
            }
            _ = wait_for_cancellation(&mut cancellation) => {
                self.qos.record_cancelled();
                let adapter_error = AgentAdapterError::new(
                    AgentAdapterErrorKind::Cancelled,
                    "ACP prompt was cancelled",
                );
                self.record_audit(AcpAuditInput {
                    operation: "session/prompt",
                    request_id: &request_id,
                    session_id,
                    session: &session,
                    qos_decision: AgentAuditQosDecision::Admitted,
                    status: AgentAuditStatus::Cancelled,
                    source_scope: source_scope.as_deref(),
                    freshness: Some(crate::api::freshness_label(freshness)),
                    limit: Some(limit),
                    result_count: None,
                    truncated: false,
                    elapsed_ms: elapsed_millis(started),
                    error_kind: Some(adapter_error.kind.as_str()),
                });
                failed_prompt(session_id, request_id.clone(), updates, adapter_error, elapsed_millis(started))
            }
        };

        registration.release();
        drop(permit);
        response
    }

    /// Cancels an active prompt request if the session still owns it.
    pub fn cancel(&self, session_id: &str, request_id: &str) -> bool {
        self.sessions.cancel_request(session_id, request_id)
    }

    /// Returns agent audit events retained by the bounded in-process log.
    pub fn audit_snapshot(&self) -> Vec<AgentAuditEvent> {
        self.audit.snapshot()
    }

    #[cfg(test)]
    pub fn qos_snapshot(&self) -> crate::net::qos::QosSnapshot {
        self.qos.snapshot()
    }

    #[cfg(test)]
    pub fn qos_diagnostics_snapshot(&self) -> crate::net::qos::QosDiagnosticsSnapshot {
        self.qos.diagnostics_snapshot()
    }

    fn admit_request(&self) -> Result<QosPermit, AgentAdapterError> {
        let policy = self.network.current().qos;
        self.qos.admit_queued_request(&policy).map_err(qos_error)
    }

    fn record_audit(&self, input: AcpAuditInput<'_>) {
        let event = AgentAuditEvent {
            sequence: 0,
            protocol: AgentProtocolKind::Acp,
            operation: input.operation.to_owned(),
            request_id: input.request_id.to_owned(),
            trace_id: format!("trace-acp-{}", input.request_id),
            runtime_identity: input
                .session
                .identity(input.session_id, Some(input.request_id.to_owned())),
            qos_decision: input.qos_decision,
            status: input.status,
            source_scope: input.source_scope.map(str::to_owned),
            freshness: input.freshness.map(str::to_owned),
            limit: input.limit,
            result_count: input.result_count,
            truncated: input.truncated,
            elapsed_ms: input.elapsed_ms,
            error_kind: input.error_kind.map(str::to_owned),
        };
        self.audit.record(event.clone());
        if input.qos_decision == AgentAuditQosDecision::Rejected {
            self.metrics
                .record_rejection("acp", input.error_kind.unwrap_or("qos_rejected"));
            return;
        }
        let status_label = match event.status {
            AgentAuditStatus::Completed => "completed",
            AgentAuditStatus::Failed => "failed",
            AgentAuditStatus::Cancelled => "cancelled",
        };
        self.metrics.record_request(
            "acp",
            input.operation,
            status_label,
            input.elapsed_ms,
            input.truncated,
        );
        if event.status == AgentAuditStatus::Cancelled {
            self.metrics.record_cancelled("acp");
        }
    }
}

struct AcpAuditInput<'a> {
    operation: &'a str,
    request_id: &'a str,
    session_id: &'a str,
    session: &'a AcpSessionRecord,
    qos_decision: AgentAuditQosDecision,
    status: AgentAuditStatus,
    source_scope: Option<&'a str>,
    freshness: Option<&'a str>,
    limit: Option<usize>,
    result_count: Option<usize>,
    truncated: bool,
    elapsed_ms: u64,
    error_kind: Option<&'a str>,
}

/// ACP initialize response with relay-knowledge capability metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpInitializeResponse {
    #[serde(rename = "_meta")]
    pub meta: AcpInitializeMeta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpInitializeMeta {
    #[serde(rename = "relayKnowledge")]
    pub relay_knowledge: AcpRelayKnowledgeCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpRelayKnowledgeCapability {
    #[serde(rename = "graphRetrieval")]
    pub graph_retrieval: bool,
    #[serde(rename = "readOnly")]
    pub read_only: bool,
    #[serde(rename = "supportsCancellation")]
    pub supports_cancellation: bool,
    #[serde(rename = "supportsIndexRefreshPermission")]
    pub supports_index_refresh_permission: bool,
}

/// Local ACP session request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpSessionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
}

/// Created ACP session metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpSession {
    pub session_id: String,
    pub runtime_identity: RuntimeIdentity,
    pub policy_id: String,
    pub authorized_scope_count: usize,
}

/// ACP prompt request with structured relay metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpPromptRequest {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<AcpPromptMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpPromptMeta {
    #[serde(rename = "relayKnowledge", skip_serializing_if = "Option::is_none")]
    pub relay_knowledge: Option<AcpRelayKnowledgePrompt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpRelayKnowledgePrompt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness: Option<String>,
}

/// ACP prompt response containing bounded progress and an optional context artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcpPromptResponse {
    pub session_id: String,
    pub request_id: String,
    pub updates: Vec<AcpSessionUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_artifact: Option<AcpContextArtifact>,
    pub stop_reason: AcpStopReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AcpErrorPayload>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcpContextArtifact {
    pub artifact_id: String,
    pub result: AgentRetrievalResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpStopReason {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpErrorPayload {
    pub error_kind: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcpSessionUpdate {
    pub request_id: String,
    pub kind: AcpSessionUpdateKind,
    pub status: AcpSessionUpdateStatus,
    pub message: String,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl AcpSessionUpdate {
    fn pending(request_id: &str, message: &str) -> Self {
        Self::new(
            request_id,
            AcpSessionUpdateKind::ToolCallUpdate,
            AcpSessionUpdateStatus::Pending,
            message,
            None,
        )
    }

    fn in_progress(request_id: &str, message: &str) -> Self {
        Self::new(
            request_id,
            AcpSessionUpdateKind::ToolCallUpdate,
            AcpSessionUpdateStatus::InProgress,
            message,
            None,
        )
    }

    fn meta(request_id: &str, message: &str, meta: Value) -> Self {
        Self::new(
            request_id,
            AcpSessionUpdateKind::SessionUpdate,
            AcpSessionUpdateStatus::InProgress,
            message,
            Some(meta),
        )
    }

    fn completed(request_id: &str, message: &str) -> Self {
        Self::new(
            request_id,
            AcpSessionUpdateKind::ToolCallUpdate,
            AcpSessionUpdateStatus::Completed,
            message,
            None,
        )
    }

    fn failed(request_id: &str, message: &str, status: AcpSessionUpdateStatus) -> Self {
        Self::new(
            request_id,
            AcpSessionUpdateKind::ToolCallUpdate,
            status,
            message,
            None,
        )
    }

    fn new(
        request_id: &str,
        kind: AcpSessionUpdateKind,
        status: AcpSessionUpdateStatus,
        message: &str,
        meta: Option<Value>,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            kind,
            status,
            message: message.to_owned(),
            meta,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpSessionUpdateKind {
    SessionUpdate,
    ToolCallUpdate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpSessionUpdateStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Default)]
struct AcpSessionRegistry {
    inner: Arc<Mutex<AcpSessionState>>,
}

#[derive(Default)]
struct AcpSessionState {
    sessions: HashMap<String, AcpSessionRecord>,
    active_requests: HashMap<String, watch::Sender<bool>>,
}

#[derive(Debug, Clone)]
struct AcpSessionRecord {
    client_name: Option<String>,
    client_version: Option<String>,
    actor_id: Option<String>,
}

impl AcpSessionRecord {
    fn identity(&self, session_id: &str, request_id: Option<String>) -> RuntimeIdentity {
        RuntimeIdentity::acp(
            self.client_name.clone(),
            self.client_version.clone(),
            self.actor_id.clone(),
            session_id.to_owned(),
            request_id,
        )
    }
}

struct ActiveAcpRequest {
    registry: AcpSessionRegistry,
    key: String,
    released: bool,
}

impl ActiveAcpRequest {
    fn release(mut self) {
        self.registry.remove_request(&self.key);
        self.released = true;
    }
}

impl Drop for ActiveAcpRequest {
    fn drop(&mut self) {
        if !self.released {
            self.registry.remove_request(&self.key);
        }
    }
}

impl AcpSessionRegistry {
    fn insert_session(&self, session_id: String, record: AcpSessionRecord) {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .sessions
            .insert(session_id, record);
    }

    fn session(&self, session_id: &str) -> Option<AcpSessionRecord> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .sessions
            .get(session_id)
            .cloned()
    }

    fn register_request(
        &self,
        session_id: &str,
        request_id: String,
    ) -> (watch::Receiver<bool>, ActiveAcpRequest) {
        let (sender, receiver) = watch::channel(false);
        let key = active_request_key(session_id, &request_id);
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_requests
            .insert(key.clone(), sender);

        (
            receiver,
            ActiveAcpRequest {
                registry: self.clone(),
                key,
                released: false,
            },
        )
    }

    fn cancel_request(&self, session_id: &str, request_id: &str) -> bool {
        let key = active_request_key(session_id, request_id);
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_requests
            .get(&key)
            .is_some_and(|sender| sender.send(true).is_ok())
    }

    fn remove_request(&self, key: &str) {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_requests
            .remove(key);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MappedPromptRequest {
    query: String,
    source_scope: Option<String>,
    limit: usize,
    freshness: FreshnessPolicy,
}

impl MappedPromptRequest {
    fn into_retrieval_request(self) -> HybridRetrievalRequest {
        HybridRetrievalRequest {
            query: self.query,
            source_scope: self.source_scope,
            limit: self.limit,
            freshness: self.freshness,
        }
    }
}

fn map_prompt_request(
    agent: &AgentRuntimeConfig,
    request: AcpPromptRequest,
) -> Result<MappedPromptRequest, AgentAdapterError> {
    let relay = request
        .meta
        .and_then(|meta| meta.relay_knowledge)
        .unwrap_or(AcpRelayKnowledgePrompt {
            query: None,
            source_scope: None,
            limit: None,
            freshness: None,
        });
    let query = relay.query.unwrap_or(request.prompt);
    let source_scope = authorize_scope(relay.source_scope, &agent.access_policy)?;
    let limit = authorize_limit(relay.limit, &agent.access_policy)?;
    let freshness = parse_freshness(relay.freshness.as_deref())?;

    if query.trim().is_empty() {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            "ACP prompt query must not be empty",
        ));
    }

    Ok(MappedPromptRequest {
        query,
        source_scope,
        limit,
        freshness,
    })
}

fn parse_freshness(value: Option<&str>) -> Result<FreshnessPolicy, AgentAdapterError> {
    match value.unwrap_or("allow-stale") {
        "allow-stale" => Ok(FreshnessPolicy::AllowStale),
        "wait-until-fresh" => Ok(FreshnessPolicy::WaitUntilFresh),
        "graph-only" => Ok(FreshnessPolicy::GraphOnly),
        other => Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!("invalid freshness '{other}'"),
        )),
    }
}

async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    while cancellation.changed().await.is_ok() {
        if *cancellation.borrow() {
            return;
        }
    }

    std::future::pending::<()>().await;
}

fn failed_prompt(
    session_id: &str,
    request_id: String,
    mut updates: Vec<AcpSessionUpdate>,
    error: AgentAdapterError,
    _elapsed_ms: u64,
) -> AcpPromptResponse {
    let stop_reason = if error.kind == AgentAdapterErrorKind::Cancelled {
        AcpStopReason::Cancelled
    } else {
        AcpStopReason::Failed
    };
    let status = if error.kind == AgentAdapterErrorKind::Cancelled {
        AcpSessionUpdateStatus::Cancelled
    } else {
        AcpSessionUpdateStatus::Failed
    };
    updates.push(AcpSessionUpdate::failed(
        &request_id,
        &error.message,
        status,
    ));

    AcpPromptResponse {
        session_id: session_id.to_owned(),
        request_id,
        updates,
        context_artifact: None,
        stop_reason,
        error: Some(AcpErrorPayload {
            error_kind: error.kind.as_str().to_owned(),
            message: error.message,
        }),
    }
}

fn qos_error(reason: RejectReason) -> AgentAdapterError {
    let message = match reason {
        RejectReason::ConnectionBudgetExceeded => "connection budget exhausted",
        RejectReason::RequestBudgetExceeded => "request budget exhausted",
        RejectReason::QueueBudgetExceeded => "queue budget exhausted",
    };

    AgentAdapterError::new(AgentAdapterErrorKind::QosRejected, message)
}

fn api_error_kind(kind: ErrorKind) -> AgentAdapterErrorKind {
    match kind {
        ErrorKind::InvalidArgument => AgentAdapterErrorKind::InvalidArgument,
        ErrorKind::StorageUnavailable => AgentAdapterErrorKind::StorageUnavailable,
        ErrorKind::QosRejected => AgentAdapterErrorKind::QosRejected,
        ErrorKind::Timeout => AgentAdapterErrorKind::Timeout,
        ErrorKind::Internal => AgentAdapterErrorKind::Internal,
    }
}

fn active_request_key(session_id: &str, request_id: &str) -> String {
    format!("{session_id}|{request_id}")
}

fn normalized_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

fn generate_acp_id(prefix: &str) -> Result<String, AgentAdapterError> {
    let mut entropy = [0_u8; 16];
    getrandom::getrandom(&mut entropy).map_err(|_| {
        AgentAdapterError::new(
            AgentAdapterErrorKind::Internal,
            "OS session entropy is unavailable",
        )
    })?;

    Ok(format!("{prefix}-{}", lowercase_hex(&entropy)))
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }

    output
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

impl fmt::Debug for LocalAcpSessionAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalAcpSessionAdapter")
            .field("agent", &self.agent)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "acp_tests.rs"]
mod acp_tests;
