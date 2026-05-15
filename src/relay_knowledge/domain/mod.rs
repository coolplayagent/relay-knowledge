//! Pure domain model types.

mod code;
mod code_repository;
mod entity;
mod error;
mod graph_version;
mod index;
mod multimodal;
mod mutation;
mod operational;
mod retrieval;
mod source;

pub use code::{
    CodeChunkRecord, CodeExtractionMetadata, CodeFileFields, CodeFileRecord, CodeGraphBatch,
    CodeGraphCommitReceipt, CodeParseStatus, CodeParseStatusCounts, CodeRange, CodeReferenceFields,
    CodeReferenceKind, CodeReferenceRecord, CodeResolutionState, CodeSymbolKind, CodeSymbolRecord,
};
pub use code_repository::{
    CodeCallRecord, CodeFileDiagnostic, CodeFileFingerprint, CodeImpactPathGroups,
    CodeImpactRequest, CodeImportRecord, CodeIndexMode, CodeIndexProgressSummary, CodeIndexRequest,
    CodeIndexSnapshot, CodeIndexSummary, CodePathTombstone, CodeQueryKind,
    CodeRepositoryExcludedPath, CodeRepositoryLanguagePreview, CodeRepositoryLargestFile,
    CodeRepositoryLatencySample, CodeRepositoryRegistration, CodeRepositoryReport,
    CodeRepositoryScopePreview, CodeRepositorySelector, CodeRepositoryStatus, CodeRepositoryTotals,
    CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest, RepositoryCodeChunkRecord,
    RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeReferenceRecord,
    RepositoryCodeSymbolRecord, code_snapshot_scope_id,
};
pub use entity::KnowledgeEntity;
pub use error::DomainError;
pub use graph_version::GraphVersion;
pub use index::{IndexKind, IndexModality, IndexState, IndexStatus};
pub use multimodal::{
    EvidenceExtractionMetadata, EvidenceModality, ExtractionDiagnostic, ExtractionStatus,
    LayoutRegion,
};
pub use mutation::{
    ClaimRecord, CommitReceipt, ConfidenceScore, EventRecord, EvidenceRecord, EvidenceSpan,
    FactStatus, GraphMutationBatch, GraphRelationRecord, GraphVersionRange,
};
pub use operational::{
    AuditEventRecord, AuditStatus, ProposalConflictRecord, ProposalConflictSeverity, ProposalKind,
    ProposalRecord, ProposalState, ServiceDefinitionPlan, ServiceManagerAction,
    ServiceOperatorState, ServiceOperatorStatus, WorkerBackendState, WorkerKind, WorkerStatus,
    WorkerTaskRecord, WorkerTaskState, normalize_actor,
};
pub use retrieval::{
    CodeGraphArtifact, CodeGraphArtifactKind, ContextEntity, ContextGraphFact,
    ContextGraphFactKind, ContextGraphPath, ContextGraphPathEdge, ContextPackItem, FreshnessPolicy,
    FusionDiagnostics, RECIPROCAL_RANK_FUSION_K, RankingSignal, RerankDiagnostics, RerankMode,
    RerankModeError, RerankSignal, RetrievalBackendState, RetrievalBackendStatus,
    RetrievalBudgetUsed, RetrievalHit, RetrievalMode, RetrievedContextPack, RetrieverSource,
};
pub use source::SourceScope;
