mod indexed_records;
mod registration;
mod repository_status;
mod retrieval_request;
mod retrieval_results;
mod scope_identity;
mod validation;

pub use indexed_records::{
    CodeCallRecord, CodeFeatureFlagRecord, CodeFileDiagnostic, CodeFileFingerprint,
    CodeImportRecord, CodePathTombstone, CodeRouteRecord, RepositoryCodeChunkRecord,
    RepositoryCodeFileRecord, RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
};
pub use registration::{
    CodeIndexMode, CodeIndexRequest, CodeRepositoryRegistration, CodeRepositorySelector,
    RepositoryCodeRange,
};
pub use repository_status::{
    CodeRepositoryExcludedPath, CodeRepositoryLanguagePreview, CodeRepositoryLargestFile,
    CodeRepositoryLatencySample, CodeRepositoryRemovalSummary, CodeRepositoryReport,
    CodeRepositoryScopePreview, CodeRepositoryStatus, CodeRepositoryTotals,
    CodeSymbolGenerationCounts,
};
pub use retrieval_request::{
    CodeFeatureFlagRequest, CodeImpactRequest, CodeQueryKind, CodeRetrievalLayer,
    CodeRetrievalRequest,
};
pub use retrieval_results::{
    CodeFeatureFlagGraph, CodeFeatureFlagUsage, CodeImpactPathGroups, CodeRetrievalHit,
};
pub use scope_identity::{
    clean_git_commit_from_snapshot_identity, code_snapshot_scope_id,
    code_snapshot_scope_id_with_workspace_detection, code_snapshot_scope_is_fact_versioned,
    code_snapshot_scope_matches_identity, code_snapshot_scope_workspace_semantic,
};
