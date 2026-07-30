use std::{
    fmt,
    time::{Duration, Instant},
};

use serde_json::json;
use tokio::sync::watch;

mod prompt_context;
mod prompt_mapping;
mod protocol;
mod session_registry;

use crate::{
    api::{AgentProtocolKind, ErrorKind, InterfaceKind, RequestContext},
    application::{AgentRuntimeConfig, RelayKnowledgeService},
    net::{
        NetworkRuntime,
        qos::{QosPermit, QosRuntime, RejectReason},
    },
    observability::AgentProtocolMetrics,
};

use super::{
    AgentAdapterError, AgentAdapterErrorKind, AgentAuditEvent, AgentAuditLog,
    AgentAuditQosDecision, AgentAuditSink, AgentAuditStatus,
};
use prompt_context::run_mapped_prompt;
use prompt_mapping::map_prompt_request;
pub use protocol::{
    AcpContextArtifact, AcpErrorPayload, AcpInitializeMeta, AcpInitializeResponse, AcpPromptMeta,
    AcpPromptRequest, AcpPromptResponse, AcpRelayKnowledgeCapability, AcpRelayKnowledgePrompt,
    AcpSession, AcpSessionRequest, AcpSessionUpdate, AcpSessionUpdateKind, AcpSessionUpdateStatus,
    AcpStopReason,
};
use session_registry::{AcpSessionRecord, AcpSessionRegistry};

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
        let record = AcpSessionRecord::new(
            request.client_name,
            request.client_version,
            request.actor_id,
        );
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
        let source_scope = mapped.audit_scope();
        let freshness = mapped.freshness;
        let limit = mapped.limit;
        let retrieval =
            run_mapped_prompt(service, mapped, context, identity, elapsed_millis(started));

        let response = tokio::select! {
            result = tokio::time::timeout(request_timeout, retrieval) => {
                match result {
                    Ok(Ok(result)) => {
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
                            result_count: Some(result.result_count()),
                            truncated: result.truncated(),
                            elapsed_ms: elapsed_millis(started),
                            error_kind: None,
                        });
                        AcpPromptResponse {
                            session_id: session_id.to_owned(),
                            request_id: request_id.clone(),
                            updates,
                            context_artifact: Some(AcpContextArtifact {
                                artifact_id,
                                result: result.retrieval,
                                codegraph_context: result.codegraph,
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
#[path = "mod_tests.rs"]
mod tests;
