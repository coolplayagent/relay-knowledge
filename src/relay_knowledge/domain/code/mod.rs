pub(crate) mod call_targets;
mod context;
mod dependencies;
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
    clean_git_commit_from_snapshot_identity, code_snapshot_expected_scope_id,
    code_snapshot_scope_id, code_snapshot_scope_is_fact_versioned,
};
pub use repository_graph::{
    IndexedRepositoryDocument, REPOSITORY_GRAPH_DEFAULT_EDGE_LIMIT,
    REPOSITORY_GRAPH_DEFAULT_NODE_LIMIT, REPOSITORY_GRAPH_MAX_DEPTH,
    REPOSITORY_GRAPH_MAX_EDGE_LIMIT, REPOSITORY_GRAPH_MAX_NODE_LIMIT, RepositoryGraphEdge,
    RepositoryGraphNeighborhood, RepositoryGraphNeighborhoodRequest, RepositoryGraphNode,
    project_okf_neighborhood,
};
pub use repository_index::{
    CodeIndexBatch, CodeIndexCheckpoint, CodeIndexProgressSummary, CodeIndexPublicationFence,
    CodeIndexResourceBudget, CodeIndexSession, CodeIndexSnapshot, CodeIndexSummary,
    CodeIndexTaskQueueStatus, CodeIndexTaskRecord, CodeIndexTaskState,
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
    CodebaseViewBudget, CodebaseViewCall, CodebaseViewDependency, CodebaseViewEdge,
    CodebaseViewEvidence, CodebaseViewFile, CodebaseViewKind, CodebaseViewNode,
    CodebaseViewRequest, CodebaseViewSection, CodebaseViewSnapshot, CodebaseViewSymbol,
};
pub use workspace::{
    CodeMonorepoWorkspace, CodeMonorepoWorkspaceFormat, CodeWorkspaceDetectionConfig,
    CodeWorkspaceMember, CodeWorkspacePackageMapping,
};
