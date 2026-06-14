use serde::{Deserialize, Serialize};

use crate::domain::{
    CodebaseViewBudget, CodebaseViewEdge, CodebaseViewEvidence, CodebaseViewNode,
    CodebaseViewRequest, CodebaseViewSection,
};

use super::{ApiMetadata, CodeRepositoryFreshnessDiagnostics, CodeRepositoryScopeMetadata};

/// Repository understanding view derived from indexed code graph facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodebaseViewResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    #[serde(default = "CodeRepositoryFreshnessDiagnostics::legacy_unknown")]
    pub freshness: CodeRepositoryFreshnessDiagnostics,
    pub request: CodebaseViewRequest,
    pub graph_version: u64,
    pub nodes: Vec<CodebaseViewNode>,
    pub edges: Vec<CodebaseViewEdge>,
    pub sections: Vec<CodebaseViewSection>,
    pub evidence: Vec<CodebaseViewEvidence>,
    pub budget: CodebaseViewBudget,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}
