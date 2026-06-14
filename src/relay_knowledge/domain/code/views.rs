use serde::{Deserialize, Serialize};

use super::{
    CodeCallRecord, CodeFeatureFlagRecord, CodeImportRecord, CodeRepositorySelector,
    CodeRetrievalLayer, CodeRouteRecord, DomainError, FreshnessPolicy, RepositoryCodeRange,
    error::required_text,
};

const MAX_CODEBASE_VIEW_CHANGED_PATHS: usize = 200;

/// Deterministic repository understanding view kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodebaseViewKind {
    ArchitectureLayers,
    BusinessDomains,
    DependencyTour,
    ProcessFlow,
    AffectedScope,
}

impl CodebaseViewKind {
    /// Stable CLI, API, and MCP representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArchitectureLayers => "architecture_layers",
            Self::BusinessDomains => "business_domains",
            Self::DependencyTour => "dependency_tour",
            Self::ProcessFlow => "process_flow",
            Self::AffectedScope => "affected_scope",
        }
    }
}

/// Request for a graph-derived repository understanding view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodebaseViewRequest {
    pub repository: CodeRepositorySelector,
    pub view_kind: CodebaseViewKind,
    pub freshness_policy: FreshnessPolicy,
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_paths: Vec<String>,
}

impl CodebaseViewRequest {
    /// Validates view inputs and bounds result fan-out.
    pub fn new(
        repository: CodeRepositorySelector,
        view_kind: CodebaseViewKind,
        freshness_policy: FreshnessPolicy,
        limit: usize,
        changed_paths: Vec<String>,
    ) -> Result<Self, DomainError> {
        let limit = match limit {
            1..=100 => limit,
            0 => return Err(DomainError::invalid("limit", "must be greater than zero")),
            _ => return Err(DomainError::invalid("limit", "must be 100 or less")),
        };
        let changed_paths = changed_paths
            .into_iter()
            .map(|path| required_text("changed_path", path))
            .collect::<Result<Vec<_>, _>>()?;
        if changed_paths.len() > MAX_CODEBASE_VIEW_CHANGED_PATHS {
            return Err(DomainError::invalid(
                "changed_paths",
                "must contain 200 or fewer entries",
            ));
        }

        Ok(Self {
            repository,
            view_kind,
            freshness_policy,
            limit,
            changed_paths,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_request_allows_changed_paths_beyond_output_limit() {
        let selector = CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap();

        let request = CodebaseViewRequest::new(
            selector,
            CodebaseViewKind::AffectedScope,
            FreshnessPolicy::AllowStale,
            1,
            vec!["src/a.rs".to_owned(), "src/b.rs".to_owned()],
        )
        .unwrap();

        assert_eq!(request.changed_paths.len(), 2);
    }

    #[test]
    fn view_request_rejects_changed_paths_beyond_input_cap() {
        let selector = CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap();
        let changed_paths = (0..=MAX_CODEBASE_VIEW_CHANGED_PATHS)
            .map(|index| format!("src/{index}.rs"))
            .collect();

        let error = CodebaseViewRequest::new(
            selector,
            CodebaseViewKind::AffectedScope,
            FreshnessPolicy::AllowStale,
            100,
            changed_paths,
        )
        .unwrap_err();

        assert!(error.to_string().contains("changed_paths"));
    }
}

/// Bounded raw graph rows used to derive codebase views.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CodebaseViewSnapshot {
    pub files: Vec<CodebaseViewFile>,
    pub symbols: Vec<CodebaseViewSymbol>,
    pub imports: Vec<CodeImportRecord>,
    pub calls: Vec<CodebaseViewCall>,
    pub routes: Vec<CodeRouteRecord>,
    pub dependencies: Vec<CodebaseViewDependency>,
    pub feature_flags: Vec<CodeFeatureFlagRecord>,
    pub truncated: bool,
}

/// File evidence row in a codebase view snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodebaseViewFile {
    pub path: String,
    pub language_id: String,
    pub parse_status: String,
    pub line_count: usize,
    pub is_generated: bool,
}

/// Symbol evidence row in a codebase view snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodebaseViewSymbol {
    pub symbol_snapshot_id: String,
    pub path: String,
    pub language_id: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub line_range: RepositoryCodeRange,
}

/// Call evidence row with optional resolved target path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodebaseViewCall {
    pub call: CodeCallRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callee_path: Option<String>,
}

/// Dependency evidence row from manifests and lockfiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodebaseViewDependency {
    pub dependency_id: String,
    pub path: String,
    pub language_id: String,
    pub ecosystem: String,
    pub package_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requirement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_version: Option<String>,
    pub dependency_group: String,
    pub source_kind: String,
    pub line_range: RepositoryCodeRange,
}

/// Graph-derived view node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodebaseViewNode {
    pub id: String,
    pub label: String,
    pub node_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub confidence: f64,
    pub evidence_ids: Vec<String>,
}

/// Graph-derived view edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodebaseViewEdge {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub edge_kind: String,
    pub confidence: f64,
    pub evidence_ids: Vec<String>,
}

/// Narrative section derived from graph facts and evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodebaseViewSection {
    pub id: String,
    pub title: String,
    pub narrative: String,
    pub confidence: f64,
    pub node_ids: Vec<String>,
    pub edge_ids: Vec<String>,
    pub evidence_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

/// Evidence reference backing a node, edge, or section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodebaseViewEvidence {
    pub id: String,
    pub evidence_kind: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_range: Option<RepositoryCodeRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieval_layer: Option<CodeRetrievalLayer>,
    pub detail: String,
}

/// View derivation budget and truncation metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodebaseViewBudget {
    pub requested_limit: usize,
    pub snapshot_row_limit: usize,
    pub snapshot_truncated: bool,
    pub nodes_truncated: bool,
    pub edges_truncated: bool,
    pub sections_truncated: bool,
    pub evidence_truncated: bool,
}

impl CodebaseViewBudget {
    /// Records the bounded work performed while deriving a view.
    pub const fn new(
        requested_limit: usize,
        snapshot_row_limit: usize,
        snapshot_truncated: bool,
    ) -> Self {
        Self {
            requested_limit,
            snapshot_row_limit,
            snapshot_truncated,
            nodes_truncated: false,
            edges_truncated: false,
            sections_truncated: false,
            evidence_truncated: false,
        }
    }
}
