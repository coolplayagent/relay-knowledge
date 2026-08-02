use serde::{Deserialize, Serialize};

use crate::{
    api::ApiMetadata,
    domain::{
        CodeRepositorySet, CodeRepositorySetAddMemberRequest, CodeRepositorySetCreateRequest,
        CodeRepositorySetMember, CodeRepositorySetQueryHit, CodeRepositorySetQueryRequest,
        CodeRepositorySetRefreshSummary, CodeRepositorySetRefreshTaskRecord,
        CodeRepositorySetRemoveMemberRequest, CodeRepositorySetStatus,
    },
};

/// Repository-set creation response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetCreateResponse {
    pub metadata: ApiMetadata,
    pub request: CodeRepositorySetCreateRequest,
    pub repository_set: CodeRepositorySet,
}

/// Repository-set member addition response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetAddResponse {
    pub metadata: ApiMetadata,
    pub request: CodeRepositorySetAddMemberRequest,
    pub member: CodeRepositorySetMember,
    pub status: CodeRepositorySetStatus,
}

/// Repository-set member removal response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetRemoveResponse {
    pub metadata: ApiMetadata,
    pub request: CodeRepositorySetRemoveMemberRequest,
    pub member: CodeRepositorySetMember,
    pub status: CodeRepositorySetStatus,
}

/// Repository-set query response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositorySetQueryResponse {
    pub metadata: ApiMetadata,
    pub request: CodeRepositorySetQueryRequest,
    pub status: CodeRepositorySetStatus,
    pub results: Vec<CodeRepositorySetQueryHit>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Repository-set status response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetStatusResponse {
    pub metadata: ApiMetadata,
    pub status: CodeRepositorySetStatus,
}

/// Repository-set overlay refresh response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetRefreshResponse {
    pub metadata: ApiMetadata,
    pub status: CodeRepositorySetStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<CodeRepositorySetRefreshSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<CodeRepositorySetRefreshTaskRecord>,
}
