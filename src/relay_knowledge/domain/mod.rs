//! Pure domain model types.

#[path = "code/graph_records.rs"]
mod code;
#[path = "code/call_targets.rs"]
pub(crate) mod code_call_targets;
#[path = "code/dependencies.rs"]
mod code_dependency;
#[path = "code/repository.rs"]
mod code_repository;
#[path = "code/repository_set.rs"]
mod code_repository_set;
#[cfg(test)]
#[path = "code/repository_tests.rs"]
mod code_repository_tests;
#[path = "code/staleness.rs"]
mod code_staleness;
#[path = "code/workspace.rs"]
mod code_workspace;
#[path = "core/entity.rs"]
mod entity;
#[path = "core/error.rs"]
mod error;
#[path = "core/graph_version.rs"]
mod graph_version;
#[path = "core/index.rs"]
mod index;
#[path = "knowledge/map.rs"]
mod knowledge_map;
#[path = "graph/multimodal.rs"]
mod multimodal;
#[path = "graph/mutation.rs"]
mod mutation;
#[path = "operations/runtime.rs"]
mod operational;
#[path = "graph/retrieval.rs"]
mod retrieval;
#[path = "operations/software.rs"]
mod software;
#[path = "core/source.rs"]
mod source;

pub use code::{
    CodeChunkRecord, CodeExtractionMetadata, CodeFileFields, CodeFileRecord, CodeGraphBatch,
    CodeGraphCommitReceipt, CodeParseStatus, CodeParseStatusCounts, CodeRange, CodeReferenceFields,
    CodeReferenceKind, CodeReferenceRecord, CodeResolutionState, CodeSymbolKind, CodeSymbolRecord,
};
pub use code_dependency::CodeDependencyRecord;
pub use code_repository::{
    CodeCallRecord, CodeFeatureFlagGraph, CodeFeatureFlagRecord, CodeFeatureFlagRequest,
    CodeFeatureFlagUsage, CodeFileDiagnostic, CodeFileFingerprint, CodeImpactPathGroups,
    CodeImpactRequest, CodeImportRecord, CodeIndexBatch, CodeIndexCheckpoint, CodeIndexMode,
    CodeIndexProgressSummary, CodeIndexRequest, CodeIndexResourceBudget, CodeIndexSession,
    CodeIndexSnapshot, CodeIndexSummary, CodeIndexTaskQueueStatus, CodeIndexTaskRecord,
    CodeIndexTaskState, CodePathTombstone, CodeQueryKind, CodeRepositoryExcludedPath,
    CodeRepositoryLanguagePreview, CodeRepositoryLargestFile, CodeRepositoryLatencySample,
    CodeRepositoryRegistration, CodeRepositoryRemovalSummary, CodeRepositoryReport,
    CodeRepositoryScopePreview, CodeRepositorySelector, CodeRepositoryStatus, CodeRepositoryTotals,
    CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest, CodeScopeRetentionSummary,
    RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
    RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord, code_snapshot_expected_scope_id,
    code_snapshot_scope_id, code_snapshot_scope_is_fact_versioned,
};
pub use code_repository_set::{
    CodeRepositoryCrossEdge, CodeRepositorySet, CodeRepositorySetAddMemberRequest,
    CodeRepositorySetCreateRequest, CodeRepositorySetMember, CodeRepositorySetMemberStatus,
    CodeRepositorySetOverlayStatus, CodeRepositorySetQueryHit, CodeRepositorySetQueryRequest,
    CodeRepositorySetRefreshSummary, CodeRepositorySetRefreshTaskRecord,
    CodeRepositorySetRefreshTaskState, CodeRepositorySetRemoveMemberRequest,
    CodeRepositorySetStatus,
};
pub use code_staleness::StalenessHint;
pub use code_workspace::{
    CodeMonorepoWorkspace, CodeMonorepoWorkspaceFormat, CodeWorkspaceDetectionConfig,
    CodeWorkspaceMember, CodeWorkspacePackageMapping,
};
pub use entity::KnowledgeEntity;
pub use error::DomainError;
pub use graph_version::GraphVersion;
pub use index::{IndexKind, IndexModality, IndexState, IndexStatus};
pub use knowledge_map::{
    KnowledgeMap, KnowledgeMapChange, KnowledgeMapHistoryEntry, KnowledgeMapRoute,
    KnowledgeMapSource, KnowledgeMapSourceKind, KnowledgeMapTopic,
};
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
    ProposalProvenance, ProposalRecord, ProposalState, ServiceDefinitionPlan, ServiceManagerAction,
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
pub use software::{
    SoftwareBuildTarget, SoftwareBuildTargetInput, SoftwareComponent, SoftwareComponentInput,
    SoftwareDependencyUsage, SoftwareDependencyUsageInput, SoftwareDesignElement,
    SoftwareDesignElementInput, SoftwareFile, SoftwareFileInput, SoftwareGlobalKind,
    SoftwareGlobalProjection, SoftwareGlobalRequest, SoftwareGlobalStatus, SoftwareIacResource,
    SoftwareIacResourceInput, SoftwareRelationship, SoftwareRelationshipInput, SoftwareSdkUsage,
    SoftwareSdkUsageInput, SoftwareTopic, SoftwareTopicInput,
};
pub use source::SourceScope;
