use serde::{Deserialize, Serialize};

use super::super::CodeParseStatusCounts;
use super::CodeQueryKind;

/// Repository index status and diagnostics summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryStatus {
    pub repository_id: String,
    pub alias: String,
    pub root_path: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_indexed_scope_id: Option<String>,
    pub last_indexed_commit: Option<String>,
    pub tree_hash: Option<String>,
    pub state: String,
    pub indexed_file_count: usize,
    pub symbol_count: usize,
    pub reference_count: usize,
    pub chunk_count: usize,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Counts and aliases removed when a registered repository is deleted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryRemovalSummary {
    pub repository_id: String,
    pub aliases_removed: Vec<String>,
    pub removed_scope_count: usize,
    pub removed_index_task_count: usize,
    pub removed_repository_set_member_count: usize,
    pub invalidated_repository_set_count: usize,
}

/// Language bucket in a repository scope preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryLanguagePreview {
    pub language_id: String,
    pub file_count: usize,
    pub byte_count: usize,
}

/// Large file surfaced before a full repository index starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryLargestFile {
    pub path: String,
    pub byte_count: usize,
}

/// Path excluded from indexing by preset, ignore file, or request scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryExcludedPath {
    pub path: String,
    pub reason: String,
}

/// Non-mutating preview of the effective repository indexing scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryScopePreview {
    pub repository_id: String,
    pub alias: String,
    pub requested_ref: String,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub selected_file_count: usize,
    pub selected_byte_count: usize,
    pub unsupported_file_count: usize,
    pub generated_or_heavy_file_count: usize,
    pub expected_degraded_file_count: usize,
    pub language_distribution: Vec<CodeRepositoryLanguagePreview>,
    pub largest_files: Vec<CodeRepositoryLargestFile>,
    pub excluded_paths: Vec<CodeRepositoryExcludedPath>,
}

/// Aggregated totals for repository indexes separate from graph-evidence code rows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryTotals {
    pub repository_count: usize,
    pub indexed_file_count: usize,
    pub symbol_count: usize,
    #[serde(default)]
    pub handwritten_symbol_count: usize,
    #[serde(default)]
    pub generated_symbol_count: usize,
    pub reference_count: usize,
    pub chunk_count: usize,
    pub degraded_file_count: usize,
    pub parse_status_counts: CodeParseStatusCounts,
}

/// Generated/handwritten split for symbols in one code index scope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeSymbolGenerationCounts {
    #[serde(default)]
    pub handwritten_symbol_count: usize,
    #[serde(default)]
    pub generated_symbol_count: usize,
}

/// Representative query latency captured for an operations report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryLatencySample {
    pub query: String,
    pub kind: CodeQueryKind,
    pub result_count: usize,
    pub duration_ms: u64,
}

/// Reusable repository operations report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryReport {
    pub repository_id: String,
    pub alias: String,
    pub root_path: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub resolved_commit_sha: Option<String>,
    pub tree_hash: Option<String>,
    pub indexed_file_count: usize,
    pub symbol_count: usize,
    #[serde(default)]
    pub handwritten_symbol_count: usize,
    #[serde(default)]
    pub generated_symbol_count: usize,
    pub reference_count: usize,
    pub chunk_count: usize,
    pub degraded_file_count: usize,
    pub resolved_edge_count: usize,
    pub ambiguous_edge_count: usize,
    pub unresolved_edge_count: usize,
    pub degradation_summary: Vec<String>,
    pub representative_queries: Vec<String>,
    pub latency_samples: Vec<CodeRepositoryLatencySample>,
    pub freshness_state: String,
}
