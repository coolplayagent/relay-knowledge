use serde::{Deserialize, Serialize};

use crate::{
    api::{ApiMetadata, CodeRepositoryFreshnessDiagnostics, CodeRepositoryScopeMetadata},
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeImpactPathGroups, CodeImpactRequest,
        CodeIndexCheckpoint, CodeIndexSummary, CodeIndexTaskRecord, CodeRepositoryRegistration,
        CodeRepositoryRemovalSummary, CodeRepositoryReport, CodeRepositoryScopePreview,
        CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest, CodeScopeRetentionSummary,
        RepositoryGraphEdge, RepositoryGraphNeighborhoodRequest, RepositoryGraphNode,
        SoftwareBuildTarget, SoftwareComponent, SoftwareDependencyUsage, SoftwareDesignElement,
        SoftwareFile, SoftwareGlobalRequest, SoftwareGlobalStatus, SoftwareIacResource,
        SoftwareRelationship, SoftwareSdkUsage, SoftwareTopic,
    },
};

/// Code repository registration request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryRegisterRequest {
    pub root_path: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub path_filters: Vec<String>,
    #[serde(default)]
    pub language_filters: Vec<String>,
}

/// Requests an incremental update from the last published Git snapshot.
///
/// The service resolves both optional refs to immutable commit identities
/// before durable work is queued. Omitting `base_ref` selects the last
/// successfully published clean commit; omitting `head_ref` selects `HEAD`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryUpdateRequest {
    #[serde(default)]
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_ref: Option<String>,
}

/// Code repository registration response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryRegisterResponse {
    pub metadata: ApiMetadata,
    pub registration: CodeRepositoryRegistration,
    pub status: CodeRepositoryStatus,
}

/// Indexed code repository list response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryListResponse {
    pub metadata: ApiMetadata,
    pub repositories: Vec<CodeRepositoryStatus>,
}

/// Code repository removal response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryRemoveResponse {
    pub metadata: ApiMetadata,
    pub removed_status: CodeRepositoryStatus,
    pub summary: CodeRepositoryRemovalSummary,
}

/// Code repository index response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryIndexResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub summary: CodeIndexSummary,
    pub status: CodeRepositoryStatus,
}

/// Code repository index start response for queued or no-op index requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryIndexStartResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<CodeIndexSummary>,
    pub status: CodeRepositoryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<CodeIndexTaskRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<CodeIndexCheckpoint>,
}

/// Code repository index task reset response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryIndexResetResponse {
    pub metadata: ApiMetadata,
    pub status: CodeRepositoryStatus,
    pub reset_task_count: usize,
    pub reset_tasks: Vec<CodeIndexTaskRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task: Option<CodeIndexTaskRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<CodeIndexCheckpoint>,
    pub retention: CodeScopeRetentionSummary,
}

/// Code repository scope preview response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryScopePreviewResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub preview: CodeRepositoryScopePreview,
}

/// Code repository retrieval response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryQueryResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    #[serde(default = "CodeRepositoryFreshnessDiagnostics::legacy_unknown")]
    pub freshness: CodeRepositoryFreshnessDiagnostics,
    pub request: CodeRetrievalRequest,
    pub results: Vec<CodeRetrievalHit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Versioned, snapshot-bound repository graph neighborhood.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryGraphNeighborhoodResponseV1 {
    pub schema_version: u8,
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub request: RepositoryGraphNeighborhoodRequest,
    pub nodes: Vec<RepositoryGraphNode>,
    pub edges: Vec<RepositoryGraphEdge>,
    pub truncated: bool,
}

/// Code repository feature-flag graph response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryFeatureFlagsResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    #[serde(default = "CodeRepositoryFreshnessDiagnostics::legacy_unknown")]
    pub freshness: CodeRepositoryFreshnessDiagnostics,
    pub request: CodeFeatureFlagRequest,
    pub flags: Vec<CodeFeatureFlagGraph>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Code repository impact response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryImpactResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub request: CodeImpactRequest,
    pub path_groups: CodeImpactPathGroups,
    pub results: Vec<CodeRetrievalHit>,
}

/// Code repository status response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryStatusResponse {
    pub metadata: ApiMetadata,
    pub status: CodeRepositoryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task: Option<CodeIndexTaskRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<CodeIndexCheckpoint>,
    pub retention: CodeScopeRetentionSummary,
}

/// Code repository operations report response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryReportResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub report: CodeRepositoryReport,
}

/// Repository-scoped software global model projection response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareGlobalResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub request: SoftwareGlobalRequest,
    pub status: SoftwareGlobalStatus,
    pub components: Vec<SoftwareComponent>,
    pub dependency_usages: Vec<SoftwareDependencyUsage>,
    pub sdk_usages: Vec<SoftwareSdkUsage>,
    pub files: Vec<SoftwareFile>,
    pub topics: Vec<SoftwareTopic>,
    pub relationships: Vec<SoftwareRelationship>,
    pub build_targets: Vec<SoftwareBuildTarget>,
    pub iac_resources: Vec<SoftwareIacResource>,
    pub design_elements: Vec<SoftwareDesignElement>,
}
