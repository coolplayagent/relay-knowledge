//! Pure domain model types.

mod business;
mod code;
mod core;
mod graph;
mod knowledge;
mod operations;

pub use business::{
    BUSINESS_GLOSSARY_MAX_BYTES, BUSINESS_GLOSSARY_MAX_DOMAINS, BUSINESS_GLOSSARY_MAX_TERMS,
    BUSINESS_GLOSSARY_SCHEMA_VERSION, BUSINESS_TERM_MAX_ALIASES, BUSINESS_TERM_MAX_MAPPINGS,
    BusinessAlias, BusinessAliasKind, BusinessDefinitionFact, BusinessDomain,
    BusinessDomainDefinition, BusinessEvidence, BusinessGlossary, BusinessKnowledgeConflict,
    BusinessKnowledgeProjection, BusinessKnowledgeProjectionInput, BusinessKnowledgeQueryKind,
    BusinessKnowledgeQueryRequest, BusinessKnowledgeResolution, BusinessKnowledgeSource,
    BusinessKnowledgeStatus, BusinessMappingRelation, BusinessSemantics, BusinessTechnicalMapping,
    BusinessTechnicalMappingDefinition, BusinessTerm, BusinessTermDefinition, BusinessTermStatus,
    OntologyEntityKind, OntologyIdentity, TechnicalTargetKind,
};
pub(crate) use code::call_targets as code_call_targets;
pub(crate) use code::{
    CODE_QUERY_INDEX_PLAN_UNIT_COUNT, CodeIncrementalClonePhase, CodeQueryIndexRepair,
    CodeQueryIndexRepairResumePhase, CodeReferenceResolution,
    CodeReferenceResolutionQueryIndexRepair, CodeReferenceResolutionStage,
    CodeReferenceSearchQueryIndexRepair, CodeReferenceSearchRebuild,
    CodeReferenceSearchRebuildStage, code_incremental_clone, code_incremental_clone_state,
    code_query_index_repair, code_query_index_repair_state, code_query_index_subphase,
    code_query_index_subphase_state, code_reference_resolution,
    code_reference_resolution_cursor_digest, code_reference_resolution_query_index_repair,
    code_reference_resolution_query_index_repair_state, code_reference_resolution_state,
    code_reference_search_query_index_repair, code_reference_search_query_index_repair_state,
    code_reference_search_rebuild, code_reference_search_rebuild_state,
};
pub use code::{
    CODEGRAPH_CONTEXT_DEFAULT_LIMIT, CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES,
    CODEGRAPH_CONTEXT_MAX_BYTES, CODEGRAPH_CONTEXT_MAX_LIMIT, CODEGRAPH_CONTEXT_MIN_BYTES,
    CodeCallRecord, CodeChunkRecord, CodeDependencyRecord, CodeExtractionMetadata,
    CodeFeatureFlagGraph, CodeFeatureFlagRecord, CodeFeatureFlagRequest, CodeFeatureFlagUsage,
    CodeFileDiagnostic, CodeFileFields, CodeFileFingerprint, CodeFileRecord,
    CodeFrameworkEdgeRecord, CodeFrameworkNodeRecord, CodeGraphBatch, CodeGraphCodeExcerpt,
    CodeGraphCommitReceipt, CodeGraphContextBudget, CodeGraphContextPack,
    CodeGraphContextProvenance, CodeGraphContextRequest, CodeGraphImpactHint, CodeImpactPathGroups,
    CodeImpactRequest, CodeImportRecord, CodeIncrementalSummaryReceipt, CodeIndexBatch,
    CodeIndexCheckpoint, CodeIndexMode, CodeIndexProgressSummary, CodeIndexPublicationFence,
    CodeIndexRequest, CodeIndexResourceBudget, CodeIndexSession, CodeIndexSnapshot,
    CodeIndexSummary, CodeIndexTaskQueueStatus, CodeIndexTaskRecord, CodeIndexTaskState,
    CodeMonorepoWorkspace, CodeMonorepoWorkspaceFormat, CodeParseStatus, CodeParseStatusCounts,
    CodePathTombstone, CodeQueryKind, CodeRange, CodeReferenceFields, CodeReferenceKind,
    CodeReferenceRecord, CodeRepositoryCrossEdge, CodeRepositoryExcludedPath,
    CodeRepositoryLanguagePreview, CodeRepositoryLargestFile, CodeRepositoryLatencySample,
    CodeRepositoryRegistration, CodeRepositoryRemovalSummary, CodeRepositoryReport,
    CodeRepositoryRetentionJobStatus, CodeRepositoryScopePreview, CodeRepositorySelector,
    CodeRepositorySet, CodeRepositorySetAddMemberRequest, CodeRepositorySetCreateRequest,
    CodeRepositorySetMember, CodeRepositorySetMemberStatus, CodeRepositorySetOverlayStatus,
    CodeRepositorySetQueryHit, CodeRepositorySetQueryRequest, CodeRepositorySetRefreshSummary,
    CodeRepositorySetRefreshTaskRecord, CodeRepositorySetRefreshTaskState,
    CodeRepositorySetRemoveMemberRequest, CodeRepositorySetStatus, CodeRepositoryStatus,
    CodeRepositoryTotals, CodeResolutionState, CodeRetrievalHit, CodeRetrievalLayer,
    CodeRetrievalRequest, CodeRouteRecord, CodeScopeRetentionSummary, CodeScopeRetirementJobStatus,
    CodeSymbolGenerationCounts, CodeSymbolKind, CodeSymbolRecord, CodeWorkspaceDetectionConfig,
    CodeWorkspaceMember, CodeWorkspacePackageMapping, CodebaseViewBudget, CodebaseViewCall,
    CodebaseViewDeclaredBusinessDomain, CodebaseViewDependency, CodebaseViewEdge,
    CodebaseViewEvidence, CodebaseViewFile, CodebaseViewKind, CodebaseViewNode,
    CodebaseViewRequest, CodebaseViewSection, CodebaseViewSnapshot, CodebaseViewSymbol,
    FrameworkEdgeKind, FrameworkGraph, FrameworkGraphRequest, FrameworkKind, FrameworkNodeKind,
    IndexedRepositoryDocument, REPOSITORY_GRAPH_DEFAULT_EDGE_LIMIT,
    REPOSITORY_GRAPH_DEFAULT_NODE_LIMIT, REPOSITORY_GRAPH_MAX_DEPTH,
    REPOSITORY_GRAPH_MAX_EDGE_LIMIT, REPOSITORY_GRAPH_MAX_NODE_LIMIT, RepositoryCodeChunkRecord,
    RepositoryCodeFileRecord, RepositoryCodeRange, RepositoryCodeReferenceRecord,
    RepositoryCodeSymbolRecord, RepositoryGraphEdge, RepositoryGraphNeighborhood,
    RepositoryGraphNeighborhoodRequest, RepositoryGraphNode, RouteHandlerRole, StalenessHint,
    SymbolRole, clean_git_commit_from_snapshot_identity, code_snapshot_scope_id,
    code_snapshot_scope_id_with_workspace_detection, code_snapshot_scope_is_fact_versioned,
    code_snapshot_scope_matches_identity, code_snapshot_scope_workspace_semantic,
    project_okf_neighborhood,
};
pub use core::{
    DomainError, GraphVersion, IndexCursor, IndexKind, IndexLag, IndexModality,
    IndexRefreshDiagnostics, IndexStalenessReason, IndexState, IndexStatus, KnowledgeEntity,
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
    FileContentChunk, FileContentEntry, FileContentReadModelCursor, FileContentSearchHit,
    FileContentSearchRequest, FileIndexDiagnostics, FileIndexEntry, FileIndexRoot,
    FileIndexRootStatus, FileIndexRootUpdate, FileIndexScanSummary, FileKnowledgeFactCandidate,
    FileSearchHit, FileSearchRequest, KnowledgeMap, KnowledgeMapChange, KnowledgeMapHistoryEntry,
    KnowledgeMapRoute, KnowledgeMapSource, KnowledgeMapSourceKind, KnowledgeMapTopic,
};
pub use operations::{
    AuditEventRecord, AuditStatus, GraphInspection, HealthStorageSnapshot, ProposalConflictRecord,
    ProposalConflictSeverity, ProposalKind, ProposalProvenance, ProposalRecord, ProposalState,
    ServiceDefinitionPlan, ServiceLifecycleExecutionReport, ServiceLifecycleStep,
    ServiceLifecycleStepResult, ServiceManagerAction, ServiceOperatorState, ServiceOperatorStatus,
    ServicePackageManifestCheck, ServicePermissionRequirement, SoftwareBuildTarget,
    SoftwareBuildTargetInput, SoftwareComponent, SoftwareComponentInput, SoftwareDependencyUsage,
    SoftwareDependencyUsageInput, SoftwareDesignElement, SoftwareDesignElementInput, SoftwareFile,
    SoftwareFileInput, SoftwareGlobalKind, SoftwareGlobalProjection, SoftwareGlobalRequest,
    SoftwareGlobalStatus, SoftwareIacResource, SoftwareIacResourceInput, SoftwareRelationship,
    SoftwareRelationshipInput, SoftwareSdkUsage, SoftwareSdkUsageInput, SoftwareTopic,
    SoftwareTopicInput, SqliteStorageDiagnostics, StorageHealth, WorkerBackendState, WorkerKind,
    WorkerStatus, WorkerTaskRecord, WorkerTaskState, normalize_actor,
};
