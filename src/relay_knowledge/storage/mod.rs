//! Storage contracts and SQLite-backed graph state.
//!
//! Storage owns persisted graph facts, mutation log entries, derived index
//! metadata, and health snapshots. Domain and interface modules must not depend
//! on SQL or concrete database types.

mod code;
mod sqlite;

use std::{error::Error, fmt, future::Future, pin::Pin};

use serde::{Deserialize, Serialize};

use crate::domain::{
    AuditEventRecord, AuditStatus, CodeChunkRecord, CodeGraphBatch, CodeGraphCommitReceipt,
    CodeParseStatusCounts, CodeReferenceRecord, CodeSymbolRecord, CommitReceipt,
    GraphMutationBatch, GraphVersion, IndexKind, IndexModality, IndexStatus,
    ProposalConflictRecord, ProposalConflictSeverity, ProposalKind, ProposalRecord, ProposalState,
    RetrievalHit, RetrieverSource, ServiceOperatorState, ServiceOperatorStatus, WorkerKind,
    WorkerStatus, WorkerTaskRecord,
};

pub use code::{CodeImpactChanges, CodeRepositoryStore};
pub use sqlite::SqliteGraphStore;

pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'a>>;

/// Synthetic scope used for graph-wide index work that is not tied to evidence.
pub const DEFAULT_INDEX_SOURCE_SCOPE: &str = "graph";

/// Graph fact persistence and query contract.
pub trait GraphStore: Send + Sync {
    fn commit_mutation_batch(&self, batch: GraphMutationBatch) -> StorageFuture<'_, CommitReceipt>;

    fn inspect_graph(&self) -> StorageFuture<'_, GraphInspection>;

    fn search(&self, request: GraphSearchRequest) -> StorageFuture<'_, Vec<RetrievalHit>>;

    fn current_graph_version(&self) -> StorageFuture<'_, GraphVersion>;
}

/// Mutation log contract consumed by reconcilers and indexers.
pub trait MutationLogStore: Send + Sync {
    fn read_after(
        &self,
        graph_version: GraphVersion,
        limit: usize,
    ) -> StorageFuture<'_, Vec<MutationLogEntry>>;
}

/// Derived index metadata contract.
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
}

/// Code graph fact persistence and query contract for tree-sitter output.
pub trait CodeGraphStore: Send + Sync {
    fn commit_code_graph_batch(
        &self,
        batch: CodeGraphBatch,
    ) -> StorageFuture<'_, CodeGraphCommitReceipt>;

    fn search_code_symbols(
        &self,
        request: CodeSymbolSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeSymbolRecord>>;

    fn search_code_references(
        &self,
        request: CodeReferenceSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeReferenceRecord>>;

    fn search_code_chunks(
        &self,
        request: CodeChunkSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeChunkRecord>>;
}

/// Combined storage facade used by the application service.
pub trait KnowledgeStore:
    GraphStore + MutationLogStore + IndexStore + CodeGraphStore + CodeRepositoryStore
{
}

impl<T> KnowledgeStore for T where
    T: GraphStore + MutationLogStore + IndexStore + CodeGraphStore + CodeRepositoryStore
{
}

/// Bounded graph search request against an explicit graph snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSearchRequest {
    pub query: String,
    pub source_scope: Option<String>,
    pub graph_version: GraphVersion,
    pub limit: usize,
    pub disabled_retriever_sources: Vec<RetrieverSource>,
}

impl GraphSearchRequest {
    /// Returns whether storage may execute a retriever family for this request.
    pub fn allows_retriever_source(&self, source: RetrieverSource) -> bool {
        !self.disabled_retriever_sources.contains(&source)
    }
}

/// Bounded code symbol search against an explicit graph snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSymbolSearchRequest {
    pub source_scope: Option<String>,
    pub path: Option<String>,
    pub name: Option<String>,
    pub graph_version: GraphVersion,
    pub limit: usize,
}

/// Bounded code reference search against an explicit graph snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeReferenceSearchRequest {
    pub source_scope: Option<String>,
    pub path: Option<String>,
    pub symbol_text: Option<String>,
    pub target_symbol_id: Option<String>,
    pub graph_version: GraphVersion,
    pub limit: usize,
}

/// Bounded code chunk search against an explicit graph snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeChunkSearchRequest {
    pub source_scope: Option<String>,
    pub path: Option<String>,
    pub query: Option<String>,
    pub graph_version: GraphVersion,
    pub limit: usize,
}

/// Worker task input inserted after graph changes or service reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerTaskSeed {
    pub kind: WorkerKind,
    pub source_scope: String,
    pub evidence_id: Option<String>,
    pub target_graph_version: GraphVersion,
    pub input_fingerprint: String,
    pub payload_json: String,
    pub now_ms: u64,
}

/// Worker lease acquisition request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerTaskClaimRequest {
    pub kind: Option<WorkerKind>,
    pub lease_owner: String,
    pub lease_duration_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Worker completion guarded by the active lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerTaskCompletion {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub now_ms: u64,
}

/// Worker failure report for retry and dead-letter handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerTaskFailure {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub error_kind: String,
    pub error_message: String,
    pub retry_backoff_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// New proposal to persist before manual approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProposal {
    pub proposal_id: String,
    pub source_scope: String,
    pub kind: ProposalKind,
    pub title: String,
    pub summary: String,
    pub payload_json: String,
    pub origin: String,
    pub confidence_basis_points: u16,
    pub conflicts: Vec<NewProposalConflict>,
    pub now_ms: u64,
}

/// New proposal conflict to persist with a proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProposalConflict {
    pub conflict_id: String,
    pub existing_fact_kind: String,
    pub existing_fact_id: String,
    pub severity: ProposalConflictSeverity,
    pub reason: String,
}

/// Proposal list filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalListRequest {
    pub state: Option<ProposalState>,
    pub limit: usize,
}

/// Proposal decision request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalDecision {
    pub proposal_id: String,
    pub next_state: ProposalState,
    pub actor: String,
    pub reason: Option<String>,
    pub now_ms: u64,
}

/// New durable audit event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAuditEvent {
    pub operation: String,
    pub interface: String,
    pub request_id: String,
    pub trace_id: String,
    pub status: AuditStatus,
    pub actor: Option<String>,
    pub source_scope: Option<String>,
    pub graph_version: u64,
    pub detail_json: String,
    pub message: Option<String>,
    pub now_ms: u64,
}

/// Audit query filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditQueryRequest {
    pub operation: Option<String>,
    pub limit: usize,
}

/// Service operator state update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceOperatorUpdate {
    pub state: ServiceOperatorState,
    pub silent_updates_enabled: bool,
    pub allowed_scopes: Vec<String>,
    pub last_error: Option<String>,
    pub now_ms: u64,
}

/// Aggregated graph status for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphInspection {
    pub graph_version: GraphVersion,
    pub entity_count: usize,
    pub evidence_count: usize,
    pub relation_count: usize,
    pub claim_count: usize,
    pub event_count: usize,
    pub mutation_count: usize,
    pub code_file_count: usize,
    pub code_symbol_count: usize,
    pub code_reference_count: usize,
    pub code_chunk_count: usize,
    pub code_parse_status_counts: CodeParseStatusCounts,
}

/// Mutation log entry returned for replay and index refresh planning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationLogEntry {
    pub graph_version: GraphVersion,
    pub evidence_count: usize,
    pub entity_count: usize,
    pub relation_count: usize,
    pub claim_count: usize,
    pub event_count: usize,
    pub affected_scopes: Vec<String>,
    pub affected_entity_ids: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub source_hashes: Vec<String>,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Storage health surfaced to diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageHealth {
    pub graph_version: GraphVersion,
    pub healthy: bool,
    pub detail: String,
}

/// Storage boundary failure.
#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Join(tokio::task::JoinError),
    LockPoisoned,
    InvalidInput(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "storage I/O failed: {error}"),
            Self::Sqlite(error) => write!(formatter, "sqlite operation failed: {error}"),
            Self::Join(error) => write!(formatter, "storage worker failed: {error}"),
            Self::LockPoisoned => write!(formatter, "sqlite connection lock was poisoned"),
            Self::InvalidInput(message) => write!(formatter, "invalid storage input: {message}"),
        }
    }
}

impl Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<tokio::task::JoinError> for StorageError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::Join(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MinimalIndexStore;

    impl IndexStore for MinimalIndexStore {
        fn index_statuses(&self) -> StorageFuture<'_, Vec<IndexStatus>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn mark_refresh_complete(
            &self,
            kind: IndexKind,
            graph_version: GraphVersion,
        ) -> StorageFuture<'_, IndexStatus> {
            Box::pin(async move {
                Ok(IndexStatus {
                    kind,
                    index_version: 1,
                    indexed_graph_version: graph_version,
                    state: crate::domain::IndexState::Fresh,
                    last_error: None,
                })
            })
        }
    }

    #[test]
    fn storage_errors_preserve_boundary_messages() {
        let io = StorageError::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "readonly",
        ));
        let sqlite = StorageError::from(rusqlite::Error::InvalidQuery);

        assert!(io.to_string().contains("storage I/O failed: readonly"));
        assert_eq!(
            sqlite.to_string(),
            "sqlite operation failed: Query is not read-only"
        );
        assert_eq!(
            StorageError::LockPoisoned.to_string(),
            "sqlite connection lock was poisoned"
        );
        assert_eq!(
            StorageError::InvalidInput("missing graph version".to_owned()).to_string(),
            "invalid storage input: missing graph version"
        );
    }

    #[tokio::test]
    async fn join_errors_map_to_storage_worker_failures() {
        let join_error = tokio::spawn(async { panic!("storage worker panic") })
            .await
            .expect_err("worker should panic");
        let error = StorageError::from(join_error);

        assert!(error.to_string().contains("storage worker failed"));
    }

    #[test]
    fn index_refresh_task_states_have_stable_storage_values() {
        assert_eq!(IndexRefreshTaskState::Queued.as_str(), "queued");
        assert_eq!(IndexRefreshTaskState::Running.as_str(), "running");
        assert_eq!(IndexRefreshTaskState::Succeeded.as_str(), "succeeded");
        assert_eq!(IndexRefreshTaskState::Retrying.as_str(), "retrying");
        assert_eq!(IndexRefreshTaskState::Failed.as_str(), "failed");
        assert_eq!(IndexRefreshTaskState::DeadLetter.as_str(), "dead_letter");
    }

    #[tokio::test]
    async fn default_index_refresh_queue_methods_report_unavailable_storage() {
        let store = MinimalIndexStore;

        let cursors = store
            .index_cursors()
            .await
            .expect_err("default cursor storage should be unavailable");
        let queued = store
            .queue_index_refreshes(IndexRefreshQueueRequest {
                kinds: vec![IndexKind::Bm25],
                target_graph_version: GraphVersion::new(1),
                max_queue_depth: 1,
                reset_dead_letter_tasks: false,
                now_ms: 10,
            })
            .await
            .expect_err("default task queue should be unavailable");
        let claimed = store
            .claim_index_refresh_task(IndexRefreshClaimRequest {
                lease_owner: "worker".to_owned(),
                lease_duration_ms: 100,
                max_attempts: 3,
                now_ms: 10,
            })
            .await
            .expect_err("default claim should be unavailable");
        let completed = store
            .complete_index_refresh_task(IndexRefreshCompletion {
                task_id: "task".to_owned(),
                lease_owner: "worker".to_owned(),
                attempt_count: 1,
                indexed_graph_version: GraphVersion::new(1),
                model_name: None,
                model_dimension: None,
                now_ms: 20,
            })
            .await
            .expect_err("default completion should be unavailable");
        let failed = store
            .fail_index_refresh_task(IndexRefreshFailure {
                task_id: "task".to_owned(),
                lease_owner: "worker".to_owned(),
                attempt_count: 1,
                error_kind: "indexer".to_owned(),
                error_message: "worker failed".to_owned(),
                retry_backoff_ms: 100,
                max_attempts: 2,
                now_ms: 20,
            })
            .await
            .expect_err("default failure handling should be unavailable");
        let diagnostics = store
            .index_refresh_diagnostics(30)
            .await
            .expect_err("default diagnostics should be unavailable");

        assert!(cursors.to_string().contains("index cursor storage"));
        for error in [queued, claimed, completed, failed] {
            assert!(
                error
                    .to_string()
                    .contains("index refresh task storage is unavailable")
            );
        }
        assert!(
            diagnostics
                .to_string()
                .contains("index refresh diagnostics are unavailable")
        );
    }

    #[tokio::test]
    async fn default_operational_methods_are_bounded_and_explicit() {
        let store = MinimalIndexStore;

        let tasks = store
            .queue_worker_tasks(vec![WorkerTaskSeed {
                kind: WorkerKind::Extractor,
                source_scope: "docs".to_owned(),
                evidence_id: Some("ev-1".to_owned()),
                target_graph_version: GraphVersion::new(1),
                input_fingerprint: "extractor:ev-1:1".to_owned(),
                payload_json: "{}".to_owned(),
                now_ms: 1,
            }])
            .await
            .expect("default queue is a no-op");
        let statuses = store
            .worker_statuses()
            .await
            .expect("default status is empty");
        let claimed = store
            .claim_worker_task(WorkerTaskClaimRequest {
                kind: None,
                lease_owner: "worker".to_owned(),
                lease_duration_ms: 10,
                max_attempts: 1,
                now_ms: 1,
            })
            .await
            .expect("default claim is empty");
        let proposals = store
            .list_proposals(ProposalListRequest {
                state: None,
                limit: 10,
            })
            .await
            .expect("default proposal list is empty");
        let conflicts = store
            .proposal_conflicts("proposal".to_owned())
            .await
            .expect("default conflicts are empty");
        let audit = store
            .query_audit_events(AuditQueryRequest {
                operation: None,
                limit: 10,
            })
            .await
            .expect("default audit query is empty");
        let audit_count = store
            .audit_event_count()
            .await
            .expect("default audit count is zero");
        let operator = store
            .service_operator_status()
            .await
            .expect("default operator is disabled");

        assert!(tasks.is_empty());
        assert!(statuses.is_empty());
        assert!(claimed.is_none());
        assert!(proposals.is_empty());
        assert!(conflicts.is_empty());
        assert!(audit.is_empty());
        assert_eq!(audit_count, 0);
        assert_eq!(operator.state, ServiceOperatorState::Disabled);

        for error in [
            store
                .complete_worker_task(WorkerTaskCompletion {
                    task_id: "task".to_owned(),
                    lease_owner: "worker".to_owned(),
                    attempt_count: 1,
                    now_ms: 2,
                })
                .await
                .expect_err("completion should require storage"),
            store
                .fail_worker_task(WorkerTaskFailure {
                    task_id: "task".to_owned(),
                    lease_owner: "worker".to_owned(),
                    attempt_count: 1,
                    error_kind: "worker".to_owned(),
                    error_message: "failed".to_owned(),
                    retry_backoff_ms: 10,
                    max_attempts: 1,
                    now_ms: 2,
                })
                .await
                .expect_err("failure should require storage"),
            store
                .insert_proposal(NewProposal {
                    proposal_id: "proposal".to_owned(),
                    source_scope: "docs".to_owned(),
                    kind: ProposalKind::Evidence,
                    title: "title".to_owned(),
                    summary: "summary".to_owned(),
                    payload_json: "{}".to_owned(),
                    origin: "test".to_owned(),
                    confidence_basis_points: 1,
                    conflicts: Vec::new(),
                    now_ms: 1,
                })
                .await
                .expect_err("proposal insert should require storage"),
            store
                .decide_proposal(ProposalDecision {
                    proposal_id: "proposal".to_owned(),
                    next_state: ProposalState::Rejected,
                    actor: "tester".to_owned(),
                    reason: None,
                    now_ms: 2,
                })
                .await
                .expect_err("proposal decision should require storage"),
            store
                .insert_audit_event(NewAuditEvent {
                    operation: "test".to_owned(),
                    interface: "cli".to_owned(),
                    request_id: "req".to_owned(),
                    trace_id: "trace".to_owned(),
                    status: AuditStatus::Completed,
                    actor: None,
                    source_scope: None,
                    graph_version: 0,
                    detail_json: "{}".to_owned(),
                    message: None,
                    now_ms: 1,
                })
                .await
                .expect_err("audit insert should require storage"),
            store
                .update_service_operator(ServiceOperatorUpdate {
                    state: ServiceOperatorState::Enabled,
                    silent_updates_enabled: true,
                    allowed_scopes: vec!["docs".to_owned()],
                    last_error: None,
                    now_ms: 2,
                })
                .await
                .expect_err("operator update should require storage"),
        ] {
            assert!(error.to_string().contains("storage is unavailable"));
        }
    }
}
