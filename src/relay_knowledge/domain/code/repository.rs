use serde::{Deserialize, Serialize};

use super::code::SymbolRole;
use super::code_repository_helpers::{
    append_hash_list, append_hash_part, checked_u32, normalize_filter_list, stable_hash64,
};
use super::{
    CodeParseStatus, CodeParseStatusCounts, CodeWorkspaceDetectionConfig, DomainError,
    FreshnessPolicy, error::required_text,
};

const CODE_SNAPSHOT_FACT_VERSION: &str = "code-facts-js-ts-import-edges-v1-sbom-dependencies-v2-python-type-refs-v1-scope-compat-v1-workspace-imports-v1-generated-files-v1-web-routes-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldQualifiers {
    search_text: String,
    kind_filters: Vec<String>,
    language_filters: Vec<String>,
    path_substrings: Vec<String>,
    name_substrings: Vec<String>,
}

fn parse_field_qualifiers(query: &str) -> FieldQualifiers {
    let mut plain_terms = Vec::new();
    let mut qualifiers = FieldQualifiers {
        search_text: String::new(),
        kind_filters: Vec::new(),
        language_filters: Vec::new(),
        path_substrings: Vec::new(),
        name_substrings: Vec::new(),
    };

    for token in query.split_whitespace() {
        if !push_field_qualifier(token, &mut qualifiers) {
            plain_terms.push(token);
        }
    }

    qualifiers.search_text = plain_terms.join(" ");
    if qualifiers.search_text.is_empty() && !query.trim().is_empty() {
        qualifiers.search_text = query.trim().to_owned();
    }

    qualifiers
}

fn push_field_qualifier(token: &str, qualifiers: &mut FieldQualifiers) -> bool {
    let Some((prefix, value)) = token.split_once(':') else {
        return false;
    };
    if value.trim().is_empty() {
        return false;
    }
    if value.starts_with(':') {
        return false;
    }

    match prefix.to_ascii_lowercase().as_str() {
        "kind" => {
            extend_qualifier_values(&mut qualifiers.kind_filters, value, true);
            true
        }
        "lang" | "language" => {
            extend_qualifier_values(&mut qualifiers.language_filters, value, true);
            true
        }
        "path" => {
            extend_qualifier_values(&mut qualifiers.path_substrings, value, false);
            true
        }
        "name" => {
            extend_qualifier_values(&mut qualifiers.name_substrings, value, false);
            true
        }
        _ => false,
    }
}

fn extend_qualifier_values(values: &mut Vec<String>, raw_value: &str, ascii_lowercase: bool) {
    for value in raw_value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let value = if ascii_lowercase {
            value.to_ascii_lowercase()
        } else {
            value.to_owned()
        };
        if !values.contains(&value) {
            values.push(value);
        }
    }
}

/// Builds the stable source scope id for a Git snapshot partition.
pub fn code_snapshot_scope_id(
    repository_id: &str,
    tree_hash: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> String {
    let mut input = Vec::new();
    append_hash_part(&mut input, "git_snapshot");
    append_hash_part(&mut input, repository_id);
    append_hash_part(&mut input, tree_hash);
    append_hash_list(&mut input, path_filters);
    append_hash_list(&mut input, language_filters);
    append_hash_part(&mut input, CODE_SNAPSHOT_FACT_VERSION);

    format!("git_snapshot:{:016x}", stable_hash64(&input))
}

pub fn code_snapshot_expected_scope_id(
    repository_id: &str,
    tree_hash: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Option<String> {
    Some(code_snapshot_scope_id(
        repository_id,
        tree_hash,
        path_filters,
        language_filters,
    ))
}

pub fn code_snapshot_scope_is_fact_versioned(source_scope: &str) -> bool {
    let Some(scope_hash) = source_scope.strip_prefix("git_snapshot:") else {
        return false;
    };
    scope_hash.len() == 16
        && scope_hash
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

/// Inclusive byte or line range for repository code index rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCodeRange {
    pub start: u32,
    pub end: u32,
}

impl RepositoryCodeRange {
    /// Creates an ordered range using one-based lines or zero-based bytes.
    pub fn new(field: &'static str, start: usize, end: usize) -> Result<Self, DomainError> {
        if end < start {
            return Err(DomainError::invalid(
                field,
                "end must be greater than or equal to start",
            ));
        }

        Ok(Self {
            start: checked_u32(field, start)?,
            end: checked_u32(field, end)?,
        })
    }
}

/// Code repository identity persisted after registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryRegistration {
    pub repository_id: String,
    pub alias: String,
    pub root_path: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
}

impl CodeRepositoryRegistration {
    /// Validates a repository registration before storage persists it.
    pub fn new(
        repository_id: impl Into<String>,
        alias: impl Into<String>,
        root_path: impl Into<String>,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            repository_id: required_text("repository_id", repository_id)?,
            alias: required_text("alias", alias)?,
            root_path: required_text("root_path", root_path)?,
            path_filters: normalize_filter_list("path_filter", path_filters)?,
            language_filters: normalize_filter_list("language_filter", language_filters)?,
        })
    }
}

/// Repository selector accepted by code index and retrieval APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySelector {
    pub repository: String,
    pub ref_selector: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
}

impl CodeRepositorySelector {
    /// Validates a code repository selector with an explicit ref.
    pub fn new(
        repository: impl Into<String>,
        ref_selector: impl Into<String>,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            repository: required_text("repository", repository)?,
            ref_selector: required_text("ref_selector", ref_selector)?,
            path_filters: normalize_filter_list("path_filter", path_filters)?,
            language_filters: normalize_filter_list("language_filter", language_filters)?,
        })
    }
}

/// Code index mode tied to Git snapshots or diffs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeIndexMode {
    Full,
    Incremental { base_ref: String, head_ref: String },
    WorktreeOverlay,
}

impl CodeIndexMode {
    /// Validates incremental refs and preserves the mode contract.
    pub fn incremental(
        base_ref: impl Into<String>,
        head_ref: impl Into<String>,
    ) -> Result<Self, DomainError> {
        Ok(Self::Incremental {
            base_ref: required_text("base_ref", base_ref)?,
            head_ref: required_text("head_ref", head_ref)?,
        })
    }
}

/// Code repository indexing request shared by interfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexRequest {
    pub repository: CodeRepositorySelector,
    pub mode: CodeIndexMode,
    #[serde(default)]
    pub workspace_detection: CodeWorkspaceDetectionConfig,
    pub freshness_policy: FreshnessPolicy,
}

/// Retrieval query kind for code graph and lexical search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeQueryKind {
    Hybrid,
    Symbol,
    Definition,
    References,
    Callers,
    Callees,
    Imports,
    Sbom,
    Impact,
}

/// Code repository retrieval request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRetrievalRequest {
    pub query: String,
    pub repository: CodeRepositorySelector,
    pub code_query_kind: CodeQueryKind,
    pub limit: usize,
    pub freshness_policy: FreshnessPolicy,
    #[serde(default)]
    pub exclude_generated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_kind_filters: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_language_filters: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_path_substrings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_name_substrings: Vec<String>,
}

impl CodeRetrievalRequest {
    /// Validates query text and result limits before storage is consulted.
    pub fn new(
        query: impl Into<String>,
        repository: CodeRepositorySelector,
        code_query_kind: CodeQueryKind,
        limit: usize,
        freshness_policy: FreshnessPolicy,
    ) -> Result<Self, DomainError> {
        let limit = match limit {
            1..=50 => limit,
            0 => return Err(DomainError::invalid("limit", "must be greater than zero")),
            _ => return Err(DomainError::invalid("limit", "must be 50 or less")),
        };

        let qualifiers = parse_field_qualifiers(&required_text("query", query)?);

        Ok(Self {
            query: qualifiers.search_text,
            repository,
            code_query_kind,
            limit,
            freshness_policy,
            exclude_generated: false,
            query_kind_filters: qualifiers.kind_filters,
            query_language_filters: qualifiers.language_filters,
            query_path_substrings: qualifiers.path_substrings,
            query_name_substrings: qualifiers.name_substrings,
        })
    }
}

/// Feature-flag graph query over an indexed repository scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFeatureFlagRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub repository: CodeRepositorySelector,
    pub limit: usize,
    pub freshness_policy: FreshnessPolicy,
}

impl CodeFeatureFlagRequest {
    /// Validates optional filter text and bounds the number of returned flags.
    pub fn new(
        query: Option<String>,
        repository: CodeRepositorySelector,
        limit: usize,
        freshness_policy: FreshnessPolicy,
    ) -> Result<Self, DomainError> {
        let limit = match limit {
            1..=100 => limit,
            0 => return Err(DomainError::invalid("limit", "must be greater than zero")),
            _ => return Err(DomainError::invalid("limit", "must be 100 or less")),
        };
        let query = query
            .map(|value| required_text("query", value))
            .transpose()?;

        Ok(Self {
            query,
            repository,
            limit,
            freshness_policy,
        })
    }
}

/// Code impact analysis request over a Git diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeImpactRequest {
    pub repository: CodeRepositorySelector,
    pub base_ref: String,
    pub head_ref: String,
    pub limit: usize,
}

impl CodeImpactRequest {
    /// Validates diff refs and bounds the impact result count.
    pub fn new(
        repository: CodeRepositorySelector,
        base_ref: impl Into<String>,
        head_ref: impl Into<String>,
        limit: usize,
    ) -> Result<Self, DomainError> {
        let limit = match limit {
            1..=100 => limit,
            0 => return Err(DomainError::invalid("limit", "must be greater than zero")),
            _ => return Err(DomainError::invalid("limit", "must be 100 or less")),
        };

        Ok(Self {
            repository,
            base_ref: required_text("base_ref", base_ref)?,
            head_ref: required_text("head_ref", head_ref)?,
            limit,
        })
    }
}

/// Retrieval layer that contributed to a code hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeRetrievalLayer {
    Lexical,
    Symbol,
    Definition,
    Reference,
    CallGraph,
    ImportGraph,
    Sbom,
    Impact,
    TextFallback,
}

impl CodeRetrievalLayer {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Symbol => "symbol",
            Self::Definition => "definition",
            Self::Reference => "reference",
            Self::CallGraph => "call_graph",
            Self::ImportGraph => "import_graph",
            Self::Sbom => "sbom",
            Self::Impact => "impact",
            Self::TextFallback => "text_fallback",
        }
    }
}

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

/// File-level code index row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCodeFileRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub file_id: String,
    pub path: String,
    pub language_id: String,
    pub blob_hash: String,
    pub byte_len: usize,
    pub line_count: usize,
    pub parse_status: CodeParseStatus,
    #[serde(default)]
    pub is_generated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Previously indexed file hash used to skip unchanged incremental parses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFileFingerprint {
    pub path: String,
    pub blob_hash: String,
}

/// Symbol definition extracted from tree-sitter syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCodeSymbolRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub symbol_snapshot_id: String,
    pub canonical_symbol_id: String,
    pub file_id: String,
    pub path: String,
    pub language_id: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_comment: Option<String>,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_role: Option<SymbolRole>,
}

/// Reference extracted from tree-sitter syntax and optionally resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCodeReferenceRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub reference_id: String,
    pub file_id: String,
    pub path: String,
    pub name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_symbol_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
}

/// Import relationship extracted from code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeImportRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub import_id: String,
    pub file_id: String,
    pub path: String,
    pub module: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub line_range: RepositoryCodeRange,
}

/// Call relationship extracted from code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeCallRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub call_id: String,
    pub file_id: String,
    pub path: String,
    pub caller_symbol_snapshot_id: Option<String>,
    pub caller_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callee_symbol_snapshot_id: Option<String>,
    pub callee_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub line_range: RepositoryCodeRange,
}

/// Web framework route mapping extracted from source code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRouteRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub route_id: String,
    pub file_id: String,
    pub path: String,
    pub language_id: String,
    pub url: String,
    /// Lowercase HTTP verb, or `any` when a framework route accepts all methods.
    pub http_method: String,
    pub handler_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handler_symbol_snapshot_id: Option<String>,
    pub framework: String,
    pub line_range: RepositoryCodeRange,
}

/// Feature flag or runtime configuration relationship extracted from code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFeatureFlagRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub feature_flag_id: String,
    pub usage_id: String,
    pub file_id: String,
    pub path: String,
    pub language_id: String,
    pub name: String,
    pub source_kind: String,
    pub source_key: String,
    pub edge_kind: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
    pub excerpt: String,
}

/// Searchable code chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCodeChunkRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub chunk_id: String,
    pub file_id: String,
    pub path: String,
    pub language_id: String,
    pub content: String,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
    pub symbol_snapshot_id: Option<String>,
}

/// File-level diagnostic produced by indexing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFileDiagnostic {
    pub repository_id: String,
    pub source_scope: String,
    pub path: String,
    pub parse_status: CodeParseStatus,
    pub message: String,
}

/// Rename/delete lineage marker retained after incremental updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodePathTombstone {
    pub repository_id: String,
    pub source_scope: String,
    pub old_path: String,
    pub new_path: Option<String>,
    pub base_ref: String,
    pub head_ref: String,
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

/// Diff paths split by the effective repository selector.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeImpactPathGroups {
    pub in_scope_changed_paths: Vec<String>,
    pub out_of_scope_changed_paths: Vec<String>,
}

pub use super::code_staleness::StalenessHint;

/// Code retrieval hit with source location, layers, and freshness metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRetrievalHit {
    pub repository_id: String,
    pub scope_id: String,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub path: String,
    pub language_id: String,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
    pub symbol_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_symbol_id: Option<String>,
    pub file_id: Option<String>,
    pub retrieval_layers: Vec<CodeRetrievalLayer>,
    pub index_versions: Vec<String>,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staleness_hint: Option<StalenessHint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_resolution_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_target_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_confidence_basis_points: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_confidence_tier: Option<String>,
    pub score: f64,
    pub excerpt: String,
}

/// One code location where a feature flag is defined, read, or guards code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeFeatureFlagUsage {
    pub usage_id: String,
    pub path: String,
    pub language_id: String,
    pub file_id: String,
    pub byte_range: RepositoryCodeRange,
    pub line_range: RepositoryCodeRange,
    pub edge_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_symbol_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_symbol_name: Option<String>,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub excerpt: String,
}

/// Feature flag graph grouped by stable configuration source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeFeatureFlagGraph {
    pub feature_flag_id: String,
    pub name: String,
    pub source_kind: String,
    pub source_key: String,
    pub score: f64,
    pub usages: Vec<CodeFeatureFlagUsage>,
}

#[cfg(test)]
mod fact_version_tests {
    use super::{
        CODE_SNAPSHOT_FACT_VERSION, CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest,
        FreshnessPolicy, parse_field_qualifiers,
    };

    #[test]
    fn code_snapshot_fact_version_includes_generated_and_web_route_facts() {
        assert!(CODE_SNAPSHOT_FACT_VERSION.contains("generated-files-v1"));
        assert!(CODE_SNAPSHOT_FACT_VERSION.contains("web-routes-v1"));
    }

    #[test]
    fn field_qualifiers_strip_known_tags_and_keep_search_text() {
        let parsed = parse_field_qualifiers(
            "kind:function,method lang:rust path:storage name:query search_code",
        );

        assert_eq!(parsed.search_text, "search_code");
        assert_eq!(parsed.kind_filters, ["function", "method"]);
        assert_eq!(parsed.language_filters, ["rust"]);
        assert_eq!(parsed.path_substrings, ["storage"]);
        assert_eq!(parsed.name_substrings, ["query"]);
    }

    #[test]
    fn field_qualifiers_keep_unknown_tags_as_plain_text() {
        let parsed = parse_field_qualifiers("owner:runtime lang:rust refresh");

        assert_eq!(parsed.search_text, "owner:runtime refresh");
        assert_eq!(parsed.language_filters, ["rust"]);
    }

    #[test]
    fn field_qualifiers_keep_double_colon_paths_as_plain_text() {
        let parsed = parse_field_qualifiers("path:storage path::normalize_filter name::Worker");

        assert_eq!(parsed.search_text, "path::normalize_filter name::Worker");
        assert_eq!(parsed.path_substrings, ["storage"]);
        assert!(parsed.name_substrings.is_empty());
    }

    #[test]
    fn retrieval_request_carries_inline_filters_from_query_text() {
        let request = CodeRetrievalRequest::new(
            "language:Rust kind:Function path:storage name:query search_code",
            CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
                .expect("selector validates"),
            CodeQueryKind::Hybrid,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request validates");

        assert_eq!(request.query, "search_code");
        assert_eq!(request.query_language_filters, ["rust"]);
        assert_eq!(request.query_kind_filters, ["function"]);
        assert_eq!(request.query_path_substrings, ["storage"]);
        assert_eq!(request.query_name_substrings, ["query"]);
    }
}
