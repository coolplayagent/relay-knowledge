use std::{
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use tokio::{io::AsyncWriteExt, sync::mpsc};

use crate::api::{AgentProtocolKind, RuntimeIdentity};

const MAX_AUDIT_EVENTS: usize = 1024;
const MAX_AUDIT_SINK_QUEUE_DEPTH: usize = 65_536;

/// In-memory bounded audit log shared by resident agent adapters.
#[derive(Clone)]
pub struct AgentAuditLog {
    inner: Arc<Mutex<AgentAuditState>>,
    sink: Option<AgentAuditSink>,
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

/// Optional async sink for durable resident-agent audit events.
#[derive(Clone)]
pub struct AgentAuditSink {
    sender: mpsc::Sender<AgentAuditEvent>,
}

impl AgentAuditSink {
    /// Spawns a bounded JSONL writer owned by the async runtime.
    pub fn jsonl(path: PathBuf, queue_depth: usize) -> Option<Self> {
        let handle = tokio::runtime::Handle::try_current().ok()?;
        let queue_depth = queue_depth.clamp(1, MAX_AUDIT_SINK_QUEUE_DEPTH);
        let (sender, mut receiver) = mpsc::channel(queue_depth);
        handle.spawn(async move {
            while let Some(event) = receiver.recv().await {
                let _ = append_jsonl_event(&path, &event).await;
            }
        });

        Some(Self { sender })
    }

    fn enqueue(&self, event: AgentAuditEvent) {
        let _ = self.sender.try_send(event);
    }
}

impl Default for AgentAuditLog {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(AgentAuditState::default())),
            sink: None,
        }
    }
}

impl AgentAuditLog {
    /// Creates a bounded in-memory log with a durable async mirror.
    pub fn with_sink(sink: AgentAuditSink) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AgentAuditState::default())),
            sink: Some(sink),
        }
    }

    /// Records an event and returns its monotonic in-process sequence number.
    pub fn record(&self, mut event: AgentAuditEvent) -> u64 {
        let sequence = {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.next_sequence = state.next_sequence.wrapping_add(1).max(1);
            event.sequence = state.next_sequence;
            state.entries.push_back(event.clone());
            while state.entries.len() > MAX_AUDIT_EVENTS {
                state.entries.pop_front();
            }

            state.next_sequence
        };
        if let Some(sink) = &self.sink {
            sink.enqueue(event);
        }

        sequence
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

async fn append_jsonl_event(path: &Path, event: &AgentAuditEvent) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    let mut line = serde_json::to_vec(event).map_err(io::Error::other)?;
    line.push(b'\n');
    file.write_all(&line).await?;
    file.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_sink_is_absent_without_entered_runtime() {
        let sink = AgentAuditSink::jsonl(PathBuf::from("/tmp/relay-audit.jsonl"), 1);

        assert!(sink.is_none());
    }

    #[tokio::test]
    async fn jsonl_sink_clamps_configured_queue_depth() {
        let sink = AgentAuditSink::jsonl(PathBuf::from("/tmp/relay-audit.jsonl"), usize::MAX)
            .expect("runtime should create audit sink");

        assert_eq!(sink.sender.max_capacity(), MAX_AUDIT_SINK_QUEUE_DEPTH);
    }
}
