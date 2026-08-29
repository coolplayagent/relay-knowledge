pub(crate) mod call_targets;
mod context;
mod dependencies;
mod framework;
mod graph_records;
mod repository;
mod repository_graph;
mod repository_index;
mod repository_set;
mod staleness;
mod views;
mod workspace;

use super::core::{DomainError, GraphVersion, SourceScope, error};
use super::graph::FreshnessPolicy;

pub use context::{
    CODEGRAPH_CONTEXT_DEFAULT_LIMIT, CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES,
    CODEGRAPH_CONTEXT_MAX_BYTES, CODEGRAPH_CONTEXT_MAX_LIMIT, CODEGRAPH_CONTEXT_MIN_BYTES,
    CodeGraphCodeExcerpt, CodeGraphContextBudget, CodeGraphContextPack, CodeGraphContextProvenance,
    CodeGraphContextRequest, CodeGraphImpactHint,
};
pub use dependencies::CodeDependencyRecord;
pub use framework::{
    CodeFrameworkEdgeRecord, CodeFrameworkNodeRecord, FrameworkEdgeKind, FrameworkGraph,
    FrameworkGraphRequest, FrameworkKind, FrameworkNodeKind,
};
pub use graph_records::{
    CodeChunkRecord, CodeExtractionMetadata, CodeFileFields, CodeFileRecord, CodeGraphBatch,
    CodeGraphCommitReceipt, CodeParseStatus, CodeParseStatusCounts, CodeRange, CodeReferenceFields,
    CodeReferenceKind, CodeReferenceRecord, CodeResolutionState, CodeSymbolKind, CodeSymbolRecord,
    RouteHandlerRole, SymbolRole,
};
pub use repository::{
    CodeCallRecord, CodeFeatureFlagGraph, CodeFeatureFlagRecord, CodeFeatureFlagRequest,
    CodeFeatureFlagUsage, CodeFileDiagnostic, CodeFileFingerprint, CodeImpactPathGroups,
    CodeImpactRequest, CodeImportRecord, CodeIndexMode, CodeIndexRequest, CodePathTombstone,
    CodeQueryKind, CodeRepositoryExcludedPath, CodeRepositoryLanguagePreview,
    CodeRepositoryLargestFile, CodeRepositoryLatencySample, CodeRepositoryRegistration,
    CodeRepositoryRemovalSummary, CodeRepositoryReport, CodeRepositoryScopePreview,
    CodeRepositorySelector, CodeRepositoryStatus, CodeRepositoryTotals, CodeRetrievalHit,
    CodeRetrievalLayer, CodeRetrievalRequest, CodeRouteRecord, CodeSymbolGenerationCounts,
    RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
    RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
    clean_git_commit_from_snapshot_identity, code_snapshot_scope_id,
    code_snapshot_scope_id_with_workspace_detection, code_snapshot_scope_is_fact_versioned,
    code_snapshot_scope_matches_identity, code_snapshot_scope_workspace_semantic,
};
pub use repository_graph::{
    IndexedRepositoryDocument, REPOSITORY_GRAPH_DEFAULT_EDGE_LIMIT,
    REPOSITORY_GRAPH_DEFAULT_NODE_LIMIT, REPOSITORY_GRAPH_MAX_DEPTH,
    REPOSITORY_GRAPH_MAX_EDGE_LIMIT, REPOSITORY_GRAPH_MAX_NODE_LIMIT, RepositoryGraphEdge,
    RepositoryGraphNeighborhood, RepositoryGraphNeighborhoodRequest, RepositoryGraphNode,
    project_okf_neighborhood,
};
pub(crate) use repository_index::{
    CODE_QUERY_INDEX_PLAN_UNIT_COUNT, CodeIncrementalClonePhase, CodeQueryIndexRepair,
    CodeQueryIndexRepairResumePhase, CodeReferenceResolution,
    CodeReferenceResolutionQueryIndexRepair, CodeReferenceResolutionStage,
    CodeReferenceSearchQueryIndexRepair, CodeReferenceSearchRebuild,
    CodeReferenceSearchRebuildStage, CodeSoftwareProjectionPhase, SOFTWARE_PROJECTION_CHECKPOINT,
    code_incremental_clone, code_incremental_clone_state, code_query_index_repair,
    code_query_index_repair_state, code_query_index_subphase, code_query_index_subphase_state,
    code_reference_resolution, code_reference_resolution_cursor_digest,
    code_reference_resolution_query_index_repair,
    code_reference_resolution_query_index_repair_state, code_reference_resolution_state,
    code_reference_search_query_index_repair, code_reference_search_query_index_repair_state,
    code_reference_search_rebuild, code_reference_search_rebuild_state,
    code_software_projection_phase,
};
pub use repository_index::{
    CodeIncrementalSummaryReceipt, CodeIndexBatch, CodeIndexCheckpoint, CodeIndexProgressSummary,
    CodeIndexPublicationFence, CodeIndexResourceBudget, CodeIndexSession, CodeIndexSnapshot,
    CodeIndexSummary, CodeIndexTaskQueueStatus, CodeIndexTaskRecord, CodeIndexTaskState,
    CodeRepositoryRetentionJobStatus, CodeScopeRetentionSummary, CodeScopeRetirementJobStatus,
};
pub use repository_set::{
    CodeRepositoryCrossEdge, CodeRepositorySet, CodeRepositorySetAddMemberRequest,
    CodeRepositorySetCreateRequest, CodeRepositorySetMember, CodeRepositorySetMemberStatus,
    CodeRepositorySetOverlayStatus, CodeRepositorySetQueryHit, CodeRepositorySetQueryRequest,
    CodeRepositorySetRefreshSummary, CodeRepositorySetRefreshTaskRecord,
    CodeRepositorySetRefreshTaskState, CodeRepositorySetRemoveMemberRequest,
    CodeRepositorySetStatus,
};
pub use staleness::StalenessHint;
pub use views::{
    CodebaseViewBudget, CodebaseViewCall, CodebaseViewDeclaredBusinessDomain,
    CodebaseViewDependency, CodebaseViewEdge, CodebaseViewEvidence, CodebaseViewFile,
    CodebaseViewKind, CodebaseViewNode, CodebaseViewRequest, CodebaseViewSection,
    CodebaseViewSnapshot, CodebaseViewSymbol,
};
pub use workspace::{
    CodeMonorepoWorkspace, CodeMonorepoWorkspaceFormat, CodeWorkspaceDetectionConfig,
    CodeWorkspaceMember, CodeWorkspacePackageMapping,
};
