use crate::{
    application::{AgentRuntimeConfig, RelayKnowledgeService},
    interfaces::agent::{AgentAuditLog, AgentAuditSink},
    net::{NetworkRuntime, qos::QosRuntime},
    observability::AgentProtocolMetrics,
};

#[cfg(test)]
use crate::interfaces::agent::AgentAuditEvent;

use super::{
    super::scope_authorization::RuntimeScopeAuthorizer,
    super::state::{CancellationRegistry, SessionRegistry},
};

/// MCP Streamable HTTP server state shared by route handlers.
#[derive(Clone)]
pub struct McpServer {
    pub(in crate::interfaces::agent::mcp) service: RelayKnowledgeService,
    pub(in crate::interfaces::agent::mcp) network: NetworkRuntime,
    pub(in crate::interfaces::agent::mcp) agent: AgentRuntimeConfig,
    pub(in crate::interfaces::agent::mcp) qos: QosRuntime,
    pub(in crate::interfaces::agent::mcp) audit: AgentAuditLog,
    pub(in crate::interfaces::agent::mcp) metrics: AgentProtocolMetrics,
    pub(in crate::interfaces::agent::mcp) cancellations: CancellationRegistry,
    pub(in crate::interfaces::agent::mcp) sessions: SessionRegistry,
    pub(in crate::interfaces::agent::mcp) scope_authorizer: RuntimeScopeAuthorizer,
}

impl McpServer {
    /// Creates MCP server state from validated runtime boundaries.
    pub fn new(
        service: RelayKnowledgeService,
        network: NetworkRuntime,
        agent: AgentRuntimeConfig,
    ) -> Self {
        let metrics = service.observability().agent_metrics();
        let qos = network.qos_runtime();
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
            cancellations: CancellationRegistry::default(),
            sessions: SessionRegistry::default(),
            scope_authorizer: RuntimeScopeAuthorizer::default(),
        }
    }

    #[cfg(test)]
    pub fn qos_snapshot(&self) -> crate::net::qos::QosSnapshot {
        self.qos.snapshot()
    }

    #[cfg(test)]
    pub fn audit_snapshot(&self) -> Vec<AgentAuditEvent> {
        self.audit.snapshot()
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
