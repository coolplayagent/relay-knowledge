use serde::{Deserialize, Serialize};

use crate::{
    api::ApiMetadata,
    domain::{
        CodeIndexTaskQueueStatus, CodeIndexTaskRecord, ProposalRecord, WorkerKind, WorkerStatus,
        WorkerTaskRecord,
    },
};

/// Master-worker diagnostics for repository code indexing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexWorkerStatus {
    pub configured_worker_count: usize,
    pub active_worker_slots: usize,
    pub queue_depth: usize,
    pub queued_task_count: usize,
    pub running_task_count: usize,
    pub retrying_task_count: usize,
    pub dead_letter_task_count: usize,
    pub running_lease_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl CodeIndexWorkerStatus {
    /// Overlays resident master runtime configuration onto durable task queue state.
    pub fn from_queue(configured_worker_count: usize, queue: CodeIndexTaskQueueStatus) -> Self {
        let queue_depth = queue
            .queued_task_count
            .saturating_add(queue.retrying_task_count);

        Self {
            configured_worker_count,
            active_worker_slots: configured_worker_count.saturating_sub(queue.running_task_count),
            queue_depth,
            queued_task_count: queue.queued_task_count,
            running_task_count: queue.running_task_count,
            retrying_task_count: queue.retrying_task_count,
            dead_letter_task_count: queue.dead_letter_task_count,
            running_lease_count: queue.running_lease_count,
            last_error: queue.last_error,
        }
    }
}

/// Worker status filter. Missing kind means all worker families.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerStatusRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<WorkerKind>,
}

/// Worker status response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerStatusResponse {
    pub metadata: ApiMetadata,
    pub workers: Vec<WorkerStatus>,
}

/// Bounded foreground worker run request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRunRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<WorkerKind>,
}

/// Bounded foreground worker run response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRunResponse {
    pub metadata: ApiMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<WorkerTaskRecord>,
    #[serde(default)]
    pub proposals: Vec<ProposalRecord>,
    pub workers: Vec<WorkerStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Preview split-worker request for one durable code-index task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexWorkerRunRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

/// Preview split-worker result for one durable code-index task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexWorkerRunResponse {
    pub metadata: ApiMetadata,
    pub worker_kind: String,
    pub claimed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<CodeIndexTaskRecord>,
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
