use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

use crate::api::{AgentProtocolKind, RuntimeIdentity};

const MAX_AUDIT_EVENTS: usize = 1024;

/// In-memory bounded audit log shared by resident agent adapters.
#[derive(Clone, Default)]
pub struct AgentAuditLog {
    inner: Arc<Mutex<AgentAuditState>>,
}

#[derive(Default)]
struct AgentAuditState {
    entries: VecDeque<AgentAuditEvent>,
    next_sequence: u64,
}

/// QoS decision captured before agent adapter work reaches application services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAuditQosDecision {
    Admitted,
    Rejected,
}

/// Final status captured for an agent protocol operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAuditStatus {
    Completed,
    Failed,
    Cancelled,
}

/// Redacted audit event for agent protocol requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAuditEvent {
    pub sequence: u64,
    pub protocol: AgentProtocolKind,
    pub operation: String,
    pub request_id: String,
    pub trace_id: String,
    pub runtime_identity: RuntimeIdentity,
    pub qos_decision: AgentAuditQosDecision,
    pub status: AgentAuditStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_count: Option<usize>,
    pub truncated: bool,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
}

impl AgentAuditLog {
    /// Records an event and returns its monotonic in-process sequence number.
    pub fn record(&self, mut event: AgentAuditEvent) -> u64 {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.next_sequence = state.next_sequence.wrapping_add(1).max(1);
        event.sequence = state.next_sequence;
        state.entries.push_back(event);
        while state.entries.len() > MAX_AUDIT_EVENTS {
            state.entries.pop_front();
        }

        state.next_sequence
    }

    /// Returns a stable snapshot for diagnostics and tests.
    pub fn snapshot(&self) -> Vec<AgentAuditEvent> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .iter()
            .cloned()
            .collect()
    }
}
