//! Pure domain model types.

mod code;
mod core;
mod graph;
mod knowledge;
mod operations;

pub(crate) use code::call_targets as code_call_targets;
pub use code::{
    CODEGRAPH_CONTEXT_DEFAULT_LIMIT, CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES,
    CODEGRAPH_CONTEXT_MAX_BYTES, CODEGRAPH_CONTEXT_MAX_LIMIT, CODEGRAPH_CONTEXT_MIN_BYTES,
    CodeCallRecord, CodeChunkRecord, CodeDependencyRecord, CodeExtractionMetadata,
    CodeFeatureFlagGraph, CodeFeatureFlagRecord, CodeFeatureFlagRequest, CodeFeatureFlagUsage,
    CodeFileDiagnostic, CodeFileFields, CodeFileFingerprint, CodeFileRecord, CodeGraphBatch,
    CodeGraphCodeExcerpt, CodeGraphCommitReceipt, CodeGraphContextBudget, CodeGraphContextPack,
    CodeGraphContextProvenance, CodeGraphContextRequest, CodeGraphImpactHint, CodeImpactPathGroups,
    CodeImpactRequest, CodeImportRecord, CodeIndexBatch, CodeIndexCheckpoint, CodeIndexMode,
    CodeIndexProgressSummary, CodeIndexPublicationFence, CodeIndexRequest, CodeIndexResourceBudget,
    CodeIndexSession, CodeIndexSnapshot, CodeIndexSummary, CodeIndexTaskQueueStatus,
    CodeIndexTaskRecord, CodeIndexTaskState, CodeMonorepoWorkspace, CodeMonorepoWorkspaceFormat,
    CodeParseStatus, CodeParseStatusCounts, CodePathTombstone, CodeQueryKind, CodeRange,
    CodeReferenceFields, CodeReferenceKind, CodeReferenceRecord, CodeRepositoryCrossEdge,
    CodeRepositoryExcludedPath, CodeRepositoryLanguagePreview, CodeRepositoryLargestFile,
    CodeRepositoryLatencySample, CodeRepositoryRegistration, CodeRepositoryRemovalSummary,
    CodeRepositoryReport, CodeRepositoryScopePreview, CodeRepositorySelector, CodeRepositorySet,
    CodeRepositorySetAddMemberRequest, CodeRepositorySetCreateRequest, CodeRepositorySetMember,
    CodeRepositorySetMemberStatus, CodeRepositorySetOverlayStatus, CodeRepositorySetQueryHit,
    CodeRepositorySetQueryRequest, CodeRepositorySetRefreshSummary,
    CodeRepositorySetRefreshTaskRecord, CodeRepositorySetRefreshTaskState,
    CodeRepositorySetRemoveMemberRequest, CodeRepositorySetStatus, CodeRepositoryStatus,
    CodeRepositoryTotals, CodeResolutionState, CodeRetrievalHit, CodeRetrievalLayer,
    CodeRetrievalRequest, CodeRouteRecord, CodeScopeRetentionSummary, CodeScopeRetirementJobStatus,
    CodeSymbolGenerationCounts, CodeSymbolKind, CodeSymbolRecord, CodeWorkspaceDetectionConfig,
    CodeWorkspaceMember, CodeWorkspacePackageMapping, CodebaseViewBudget, CodebaseViewCall,
    CodebaseViewDependency, CodebaseViewEdge, CodebaseViewEvidence, CodebaseViewFile,
    CodebaseViewKind, CodebaseViewNode, CodebaseViewRequest, CodebaseViewSection,
    CodebaseViewSnapshot, CodebaseViewSymbol, IndexedRepositoryDocument,
    REPOSITORY_GRAPH_DEFAULT_EDGE_LIMIT, REPOSITORY_GRAPH_DEFAULT_NODE_LIMIT,
    REPOSITORY_GRAPH_MAX_DEPTH, REPOSITORY_GRAPH_MAX_EDGE_LIMIT, REPOSITORY_GRAPH_MAX_NODE_LIMIT,
    RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
    RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord, RepositoryGraphEdge,
    RepositoryGraphNeighborhood, RepositoryGraphNeighborhoodRequest, RepositoryGraphNode,
    RouteHandlerRole, StalenessHint, SymbolRole, clean_git_commit_from_snapshot_identity,
    code_snapshot_expected_scope_id, code_snapshot_scope_id, code_snapshot_scope_is_fact_versioned,
    project_okf_neighborhood,
};
pub use core::{
    DomainError, GraphVersion, IndexKind, IndexModality, IndexState, IndexStatus, KnowledgeEntity,
    SourceScope,
};
pub use graph::{
    ClaimRecord, CodeGraphArtifact, CodeGraphArtifactKind, CommitReceipt, ConfidenceScore,
    ContextEntity, ContextGraphFact, ContextGraphFactKind, ContextGraphPath, ContextGraphPathEdge,
    ContextPackItem, EventRecord, EvidenceExtractionMetadata, EvidenceModality, EvidenceRecord,
    EvidenceSpan, ExtractionDiagnostic, ExtractionStatus, FactStatus, FreshnessPolicy,
    FusionDiagnostics, GraphMutationBatch, GraphRelationRecord, GraphVersionRange, LayoutRegion,
    RECIPROCAL_RANK_FUSION_K, RankingSignal, RerankDiagnostics, RerankMode, RerankModeError,
    RerankSignal, RetrievalBackendState, RetrievalBackendStatus, RetrievalBudgetUsed, RetrievalHit,
    RetrievalMode, RetrievedContextPack, RetrieverSource, TraversalProvenanceTrace,
    TraversalRankingContribution, TraversalTraceEdge, TraversalTraceEvidence, TraversalTraceNode,
    TraversalTraceNodeKind, TraversalTraceRedaction,
};
pub use knowledge::{
    KnowledgeMap, KnowledgeMapChange, KnowledgeMapHistoryEntry, KnowledgeMapRoute,
    KnowledgeMapSource, KnowledgeMapSourceKind, KnowledgeMapTopic,
};
pub use operations::{
    AuditEventRecord, AuditStatus, ProposalConflictRecord, ProposalConflictSeverity, ProposalKind,
    ProposalProvenance, ProposalRecord, ProposalState, ServiceDefinitionPlan,
    ServiceLifecycleExecutionReport, ServiceLifecycleStep, ServiceLifecycleStepResult,
    ServiceManagerAction, ServiceOperatorState, ServiceOperatorStatus, ServicePackageManifestCheck,
    ServicePermissionRequirement, SoftwareBuildTarget, SoftwareBuildTargetInput, SoftwareComponent,
    SoftwareComponentInput, SoftwareDependencyUsage, SoftwareDependencyUsageInput,
    SoftwareDesignElement, SoftwareDesignElementInput, SoftwareFile, SoftwareFileInput,
    SoftwareGlobalKind, SoftwareGlobalProjection, SoftwareGlobalRequest, SoftwareGlobalStatus,
    SoftwareIacResource, SoftwareIacResourceInput, SoftwareRelationship, SoftwareRelationshipInput,
    SoftwareSdkUsage, SoftwareSdkUsageInput, SoftwareTopic, SoftwareTopicInput, WorkerBackendState,
    WorkerKind, WorkerStatus, WorkerTaskRecord, WorkerTaskState, normalize_actor,
};
