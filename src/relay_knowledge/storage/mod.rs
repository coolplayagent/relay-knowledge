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
    CodeChunkRecord, CodeGraphBatch, CodeGraphCommitReceipt, CodeParseStatusCounts,
    CodeReferenceRecord, CodeSymbolRecord, CommitReceipt, GraphMutationBatch, GraphVersion,
    IndexKind, IndexModality, IndexStatus, RetrievalHit,
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
}
