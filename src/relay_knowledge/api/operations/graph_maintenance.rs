use serde::{Deserialize, Serialize};

use crate::{
    api::ApiMetadata,
    domain::{CodeRepositoryTotals, IndexKind, IndexStatus},
    storage::{GraphInspection, IndexCursor, IndexRefreshDiagnostics},
};

/// Graph inspection request with optional scope filtering reserved for adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphInspectionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
}

/// Graph inspection response for diagnostics and agent adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphInspectionResponse {
    pub metadata: ApiMetadata,
    pub graph: GraphInspection,
    pub repository_code_totals: CodeRepositoryTotals,
}

/// Index refresh request. Empty `kinds` means all v1 index families.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRefreshRequest {
    #[serde(default)]
    pub kinds: Vec<IndexKind>,
}

/// Index refresh response after metadata is updated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRefreshResponse {
    pub metadata: ApiMetadata,
    pub indexes: Vec<IndexStatus>,
    pub index_cursors: Vec<IndexCursor>,
    pub diagnostics: IndexRefreshDiagnostics,
}
