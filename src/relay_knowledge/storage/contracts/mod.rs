mod boundary;
mod canvas;
mod code;
mod code_graph;
mod file_index;
mod graph;
mod health;
mod index;
mod operations;
mod search;
mod topology;

pub use boundary::{StorageError, StorageFuture};
pub use canvas::{
    GraphCanvasSelection, GraphCanvasStorageEdge, GraphCanvasStorageNode,
    GraphCanvasStorageRequest, GraphCanvasStorageSnapshot,
};
pub use code::{
    CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE, CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
    CodeImpactChanges, CodeIndexTaskClaimRequest, CodeIndexTaskCompletion, CodeIndexTaskFailure,
    CodeIndexTaskLeaseRecord, CodeIndexTaskLeaseRecovery, CodeIndexTaskLeaseRenewal,
    CodeIndexTaskSeed, CodeRepositorySetEdgeSelector, CodeRepositorySetMemberSeed,
    CodeRepositorySetRefreshPublication, CodeRepositorySetRefreshTaskClaimRequest,
    CodeRepositorySetRefreshTaskCompletion, CodeRepositorySetRefreshTaskFailure,
    CodeRepositorySetRefreshTaskSeed, CodeRepositorySetSeed, CodeRepositoryStore,
    CodeScopeRetentionRequest,
};
pub use code_graph::{
    CodeChunkSearchRequest, CodeGraphStore, CodeReferenceSearchRequest, CodeSymbolSearchRequest,
};
pub use file_index::{
    FileContentChunk, FileContentEntry, FileContentReadModelCursor, FileContentSearchHit,
    FileContentSearchRequest, FileIndexDiagnostics, FileIndexEntry, FileIndexRoot,
    FileIndexRootStatus, FileIndexRootUpdate, FileIndexScanSummary, FileKnowledgeFactCandidate,
    FileSearchHit, FileSearchRequest,
};
pub use graph::{GraphStore, MutationLogEntry, MutationLogStore};
pub use health::{GraphInspection, HealthStorageSnapshot, SqliteStorageDiagnostics, StorageHealth};
pub use index::{
    DEFAULT_INDEX_SOURCE_SCOPE, IndexCursor, IndexLag, IndexRefreshClaimRequest,
    IndexRefreshCompletion, IndexRefreshDiagnostics, IndexRefreshFailure, IndexRefreshQueueRequest,
    IndexRefreshTask, IndexRefreshTaskState, IndexStalenessReason, IndexStore,
};
pub use operations::{
    AuditQueryRequest, NewAuditEvent, NewProposal, NewProposalConflict, ProposalDecision,
    ProposalListRequest, ServiceOperatorUpdate, WorkerTaskClaimRequest, WorkerTaskCompletion,
    WorkerTaskFailure, WorkerTaskSeed,
};
pub use search::{
    GraphSearchOutcome, GraphSearchRequest, MAX_GRAPH_SEARCH_FTS_CODEPOINTS,
    MAX_GRAPH_SEARCH_FTS_TOKENS, MAX_GRAPH_SEARCH_LIMIT, MAX_GRAPH_SEARCH_QUERY_CHARS,
    MAX_GRAPH_SEARCH_TOKEN_BYTES,
};
pub use topology::{StorageShardCatalogEntry, StorageTopology, StorageTopologySnapshot};

/// Combined storage facade used by the application service.
pub trait KnowledgeStore:
    GraphStore + MutationLogStore + IndexStore + CodeGraphStore + CodeRepositoryStore
{
}

impl<T> KnowledgeStore for T where
    T: GraphStore + MutationLogStore + IndexStore + CodeGraphStore + CodeRepositoryStore
{
}
