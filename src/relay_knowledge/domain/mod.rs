//! Pure domain model types.

mod code;
mod code_repository;
mod entity;
mod error;
mod graph_version;
mod index;
mod mutation;
mod retrieval;
mod source;

pub use code::{
    CodeChunkRecord, CodeExtractionMetadata, CodeFileFields, CodeFileRecord, CodeGraphBatch,
    CodeGraphCommitReceipt, CodeParseStatus, CodeParseStatusCounts, CodeRange, CodeReferenceFields,
    CodeReferenceKind, CodeReferenceRecord, CodeResolutionState, CodeSymbolKind, CodeSymbolRecord,
};
pub use code_repository::{
    CodeCallRecord, CodeFileDiagnostic, CodeFileFingerprint, CodeImpactRequest, CodeImportRecord,
    CodeIndexMode, CodeIndexRequest, CodeIndexSnapshot, CodeIndexSummary, CodePathTombstone,
    CodeQueryKind, CodeRepositoryRegistration, CodeRepositorySelector, CodeRepositoryStatus,
    CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest, RepositoryCodeChunkRecord,
    RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeReferenceRecord,
    RepositoryCodeSymbolRecord,
};
pub use entity::KnowledgeEntity;
pub use error::DomainError;
pub use graph_version::GraphVersion;
pub use index::{IndexKind, IndexModality, IndexState, IndexStatus};
pub use mutation::{
    ClaimRecord, CommitReceipt, ConfidenceScore, EventRecord, EvidenceRecord, EvidenceSpan,
    FactStatus, GraphMutationBatch, GraphRelationRecord, GraphVersionRange,
};
pub use retrieval::{
    ContextPackItem, FreshnessPolicy, FusionDiagnostics, RECIPROCAL_RANK_FUSION_K, RankingSignal,
    RetrievalBudgetUsed, RetrievalHit, RetrievalMode, RetrievedContextPack, RetrieverSource,
};
pub use source::SourceScope;
