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
    IndexKind, IndexStatus, RetrievalHit,
};

pub use code::{CodeImpactChanges, CodeRepositoryStore};
pub use sqlite::SqliteGraphStore;

pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'a>>;

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
}
