use serde::{Deserialize, Serialize};

use crate::domain::{
    AuditEventRecord, GraphVersion, IndexKind, IndexModality, IndexStatus, ProposalConflictRecord,
    ProposalRecord, ProposalState, ServiceOperatorState, ServiceOperatorStatus, WorkerStatus,
    WorkerTaskRecord,
};

use super::{
    AuditQueryRequest, FileContentSearchHit, FileContentSearchRequest, FileIndexDiagnostics,
    FileIndexRoot, FileIndexRootStatus, FileIndexRootUpdate, FileSearchHit, FileSearchRequest,
    NewAuditEvent, NewProposal, ProposalDecision, ProposalListRequest, ServiceOperatorUpdate,
    StorageError, StorageFuture, WorkerTaskClaimRequest, WorkerTaskCompletion, WorkerTaskFailure,
    WorkerTaskSeed,
};

/// Synthetic scope used for graph-wide index work that is not tied to evidence.
pub const DEFAULT_INDEX_SOURCE_SCOPE: &str = "graph";

/// Derived index metadata and operational persistence contract.
pub trait IndexStore: Send + Sync {
    fn index_statuses(&self) -> StorageFuture<'_, Vec<IndexStatus>>;

    fn mark_refresh_complete(
        &self,
        kind: IndexKind,
        graph_version: GraphVersion,
    ) -> StorageFuture<'_, IndexStatus>;

    fn index_cursors(&self) -> StorageFuture<'_, Vec<IndexCursor>> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "index cursor storage is unavailable".to_owned(),
            ))
        })
    }

    fn queue_index_refreshes(
        &self,
        _request: IndexRefreshQueueRequest,
    ) -> StorageFuture<'_, IndexRefreshDiagnostics> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "index refresh task storage is unavailable".to_owned(),
            ))
        })
    }

    fn claim_index_refresh_task(
        &self,
        _request: IndexRefreshClaimRequest,
    ) -> StorageFuture<'_, Option<IndexRefreshTask>> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "index refresh task storage is unavailable".to_owned(),
            ))
        })
    }

    fn complete_index_refresh_task(
        &self,
        _request: IndexRefreshCompletion,
    ) -> StorageFuture<'_, IndexRefreshTask> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "index refresh task storage is unavailable".to_owned(),
            ))
        })
    }

    fn fail_index_refresh_task(
        &self,
        _request: IndexRefreshFailure,
    ) -> StorageFuture<'_, IndexRefreshTask> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "index refresh task storage is unavailable".to_owned(),
            ))
        })
    }

    fn index_refresh_diagnostics(
        &self,
        _now_ms: u64,
    ) -> StorageFuture<'_, IndexRefreshDiagnostics> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "index refresh diagnostics are unavailable".to_owned(),
            ))
        })
    }

    fn queue_worker_tasks(
        &self,
        _tasks: Vec<WorkerTaskSeed>,
    ) -> StorageFuture<'_, Vec<WorkerTaskRecord>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn worker_statuses(&self) -> StorageFuture<'_, Vec<WorkerStatus>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn claim_worker_task(
        &self,
        _request: WorkerTaskClaimRequest,
    ) -> StorageFuture<'_, Option<WorkerTaskRecord>> {
        Box::pin(async { Ok(None) })
    }

    fn complete_worker_task(
        &self,
        _request: WorkerTaskCompletion,
    ) -> StorageFuture<'_, WorkerTaskRecord> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "worker task storage is unavailable".to_owned(),
            ))
        })
    }

    fn fail_worker_task(&self, _request: WorkerTaskFailure) -> StorageFuture<'_, WorkerTaskRecord> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "worker task storage is unavailable".to_owned(),
            ))
        })
    }

    fn insert_proposal(&self, _proposal: NewProposal) -> StorageFuture<'_, ProposalRecord> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "proposal storage is unavailable".to_owned(),
            ))
        })
    }

    fn list_proposals(
        &self,
        _request: ProposalListRequest,
    ) -> StorageFuture<'_, Vec<ProposalRecord>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn proposal_count(&self, _state: Option<ProposalState>) -> StorageFuture<'_, usize> {
        Box::pin(async { Ok(0) })
    }

    fn proposal_by_id(&self, _proposal_id: String) -> StorageFuture<'_, Option<ProposalRecord>> {
        Box::pin(async { Ok(None) })
    }

    fn proposal_conflicts(
        &self,
        _proposal_id: String,
    ) -> StorageFuture<'_, Vec<ProposalConflictRecord>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn decide_proposal(&self, _request: ProposalDecision) -> StorageFuture<'_, ProposalRecord> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "proposal storage is unavailable".to_owned(),
            ))
        })
    }

    fn insert_audit_event(&self, _event: NewAuditEvent) -> StorageFuture<'_, AuditEventRecord> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "audit storage is unavailable".to_owned(),
            ))
        })
    }

    fn query_audit_events(
        &self,
        _request: AuditQueryRequest,
    ) -> StorageFuture<'_, Vec<AuditEventRecord>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn audit_event_count(&self) -> StorageFuture<'_, usize> {
        Box::pin(async { Ok(0) })
    }

    fn service_operator_status(&self) -> StorageFuture<'_, ServiceOperatorStatus> {
        Box::pin(async {
            Ok(ServiceOperatorStatus {
                state: ServiceOperatorState::Disabled,
                silent_updates_enabled: false,
                allowed_scopes: Vec::new(),
                last_run_at_ms: None,
                next_retry_at_ms: None,
                last_error: None,
                updated_at_ms: 0,
            })
        })
    }

    fn update_service_operator(
        &self,
        _request: ServiceOperatorUpdate,
    ) -> StorageFuture<'_, ServiceOperatorStatus> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "service operator storage is unavailable".to_owned(),
            ))
        })
    }

    fn replace_file_index_root(
        &self,
        _update: FileIndexRootUpdate,
    ) -> StorageFuture<'_, FileIndexRootStatus> {
        unavailable_file_index_storage()
    }

    fn mark_file_index_roots_unconfigured(
        &self,
        _active_roots: Vec<FileIndexRoot>,
        _now_ms: u64,
    ) -> StorageFuture<'_, FileIndexDiagnostics> {
        unavailable_file_index_storage()
    }

    fn search_files(&self, _request: FileSearchRequest) -> StorageFuture<'_, Vec<FileSearchHit>> {
        unavailable_file_index_storage()
    }

    fn search_file_content(
        &self,
        _request: FileContentSearchRequest,
    ) -> StorageFuture<'_, Vec<FileContentSearchHit>> {
        unavailable_file_index_storage()
    }

    fn file_index_diagnostics(&self) -> StorageFuture<'_, FileIndexDiagnostics> {
        unavailable_file_index_storage()
    }
}

fn unavailable_file_index_storage<T>() -> StorageFuture<'static, T> {
    Box::pin(async {
        Err(StorageError::InvalidInput(
            "file index storage is unavailable".to_owned(),
        ))
    })
}

/// Scoped cursor for a derived index read model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexCursor {
    pub kind: IndexKind,
    pub source_scope: String,
    pub modality: IndexModality,
    pub index_version: u64,
    pub indexed_graph_version: GraphVersion,
    pub state: crate::domain::IndexState,
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_dimension: Option<u32>,
}

/// Persistent index refresh task lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexRefreshTaskState {
    Queued,
    Running,
    Succeeded,
    Retrying,
    Failed,
    DeadLetter,
}

impl IndexRefreshTaskState {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Retrying => "retrying",
            Self::Failed => "failed",
            Self::DeadLetter => "dead_letter",
        }
    }
}

/// Persistent task used by foreground refresh and startup recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRefreshTask {
    pub task_id: String,
    pub kind: IndexKind,
    pub source_scope: String,
    pub modality: IndexModality,
    pub target_graph_version: GraphVersion,
    pub state: IndexRefreshTaskState,
    pub lease_owner: Option<String>,
    pub lease_expires_at_ms: Option<u64>,
    pub attempt_count: u32,
    pub next_retry_at_ms: u64,
    pub input_fingerprint: String,
    pub cursor_before: GraphVersion,
    pub cursor_after: Option<GraphVersion>,
    pub last_error_kind: Option<String>,
    pub last_error_message: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Queue request created by refresh APIs or the reconciler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRefreshQueueRequest {
    pub kinds: Vec<IndexKind>,
    pub target_graph_version: GraphVersion,
    pub max_queue_depth: usize,
    pub reset_dead_letter_tasks: bool,
    pub now_ms: u64,
}

/// Lease acquisition request for bounded foreground workers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRefreshClaimRequest {
    pub lease_owner: String,
    pub lease_duration_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Completion report guarded by the active task lease and attempt token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRefreshCompletion {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub indexed_graph_version: GraphVersion,
    pub model_name: Option<String>,
    pub model_dimension: Option<u32>,
    pub now_ms: u64,
}

/// Failure report for retry backoff and dead-letter isolation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRefreshFailure {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub error_kind: String,
    pub error_message: String,
    pub retry_backoff_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Per-kind lag included in diagnostics snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexLag {
    pub kind: IndexKind,
    pub lag_versions: u64,
}

/// Structured reason explaining why an index family or scoped cursor is stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStalenessReason {
    pub kind: IndexKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modality: Option<IndexModality>,
    pub reason: String,
    pub lag_versions: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Snapshot for queue, dead-letter, and stale-index diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRefreshDiagnostics {
    pub queue_depth: usize,
    pub running_count: usize,
    pub retrying_count: usize,
    pub dead_letter_count: usize,
    pub oldest_unfinished_age_ms: Option<u64>,
    pub index_lag_by_kind: Vec<IndexLag>,
    pub max_index_lag_versions: u64,
    pub stale_index_count: usize,
    pub stale_reasons: Vec<IndexStalenessReason>,
}

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
