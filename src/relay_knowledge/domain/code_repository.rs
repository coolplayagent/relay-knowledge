use serde::{Deserialize, Serialize};

use super::{CodeParseStatus, DomainError, FreshnessPolicy, error::required_text};

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

    format!("git_snapshot:{:016x}", stable_hash64(&input))
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

        Ok(Self {
            query: required_text("query", query)?,
            repository,
            code_query_kind,
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

/// Parsed index changes ready to commit into storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexSnapshot {
    pub repository_id: String,
    pub source_scope: String,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub full_replace: bool,
    pub changed_path_count: usize,
    pub skipped_unchanged_count: usize,
    pub deleted_paths: Vec<String>,
    pub tombstones: Vec<CodePathTombstone>,
    pub files: Vec<RepositoryCodeFileRecord>,
    pub symbols: Vec<RepositoryCodeSymbolRecord>,
    pub references: Vec<RepositoryCodeReferenceRecord>,
    pub imports: Vec<CodeImportRecord>,
    pub calls: Vec<CodeCallRecord>,
    pub chunks: Vec<RepositoryCodeChunkRecord>,
    pub diagnostics: Vec<CodeFileDiagnostic>,
}

/// Coarse phase timing and counts reported by repository indexing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexProgressSummary {
    pub git_file_count: usize,
    pub blob_read_count: usize,
    pub parsed_file_count: usize,
    pub sqlite_write_count: usize,
    pub skipped_file_count: usize,
    pub degraded_file_count: usize,
}

/// Result of applying a code index snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexSummary {
    pub repository_id: String,
    pub source_scope: String,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub indexed_file_count: usize,
    pub changed_path_count: usize,
    pub skipped_unchanged_count: usize,
    pub deleted_path_count: usize,
    pub symbol_count: usize,
    pub reference_count: usize,
    pub chunk_count: usize,
    pub degraded_file_count: usize,
    pub progress: CodeIndexProgressSummary,
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
    pub reference_count: usize,
    pub chunk_count: usize,
    pub degraded_file_count: usize,
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

fn normalize_filter_list(
    field: &'static str,
    values: Vec<String>,
) -> Result<Vec<String>, DomainError> {
    let mut normalized = Vec::new();
    for value in values {
        let value = required_text(field, value)?;
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }

    Ok(normalized)
}

fn checked_u32(field: &'static str, value: usize) -> Result<u32, DomainError> {
    u32::try_from(value).map_err(|_| DomainError::invalid(field, "must fit in u32"))
}

fn append_hash_list(input: &mut Vec<u8>, values: &[String]) {
    input.extend_from_slice(&(values.len() as u64).to_le_bytes());
    for value in values {
        append_hash_part(input, value);
    }
}

fn append_hash_part(input: &mut Vec<u8>, value: &str) {
    input.extend_from_slice(&(value.len() as u64).to_le_bytes());
    input.extend_from_slice(value.as_bytes());
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_trims_and_deduplicates_filters() {
        let selector = CodeRepositorySelector::new(
            " repo ",
            " HEAD ",
            vec!["src".to_owned(), " src ".to_owned()],
            vec!["rust".to_owned(), "rust".to_owned()],
        )
        .expect("selector should validate");

        assert_eq!(selector.repository, "repo");
        assert_eq!(selector.ref_selector, "HEAD");
        assert_eq!(selector.path_filters, ["src"]);
        assert_eq!(selector.language_filters, ["rust"]);
    }

    #[test]
    fn snapshot_scope_id_tracks_tree_and_filters() {
        let scope = code_snapshot_scope_id(
            "repo-1",
            "tree-a",
            &["src".to_owned()],
            &["rust".to_owned()],
        );
        let same = code_snapshot_scope_id(
            "repo-1",
            "tree-a",
            &["src".to_owned()],
            &["rust".to_owned()],
        );
        let different_tree = code_snapshot_scope_id(
            "repo-1",
            "tree-b",
            &["src".to_owned()],
            &["rust".to_owned()],
        );

        assert_eq!(scope, same);
        assert_ne!(scope, different_tree);
        assert!(scope.starts_with("git_snapshot:"));
    }

    #[test]
    fn retrieval_request_rejects_unbounded_limits() {
        let selector = CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
            .expect("selector should validate");
        let error = CodeRetrievalRequest::new(
            "symbol",
            selector,
            CodeQueryKind::Hybrid,
            51,
            FreshnessPolicy::AllowStale,
        )
        .expect_err("large limit should fail");

        assert_eq!(error.field, "limit");
    }

    #[test]
    fn code_ranges_must_be_ordered() {
        let error = RepositoryCodeRange::new("line_range", 3, 2).expect_err("range should fail");

        assert_eq!(error.field, "line_range");
    }
}
