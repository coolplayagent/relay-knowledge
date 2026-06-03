use serde::{Deserialize, Serialize};

use super::{
    CodeDependencyRecord, CodeParseStatus, CodeParseStatusCounts, DomainError, FreshnessPolicy,
    error::required_text,
};

const CODE_SNAPSHOT_FACT_VERSION: &str =
    "code-facts-js-ts-import-edges-v1-sbom-dependencies-v2-python-type-refs-v1-scope-compat-v1";

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

/// Parsed index changes ready to commit into storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexSnapshot {
    pub repository_id: String,
    pub source_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_resolved_commit_sha: Option<String>,
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
    pub dependencies: Vec<CodeDependencyRecord>,
    pub feature_flags: Vec<CodeFeatureFlagRecord>,
    pub chunks: Vec<RepositoryCodeChunkRecord>,
    pub diagnostics: Vec<CodeFileDiagnostic>,
}

/// Resource budget used to partition repository indexing into bounded batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexResourceBudget {
    pub max_files_per_batch: usize,
    pub max_bytes_per_batch: usize,
    pub max_rows_per_batch: usize,
}

impl CodeIndexResourceBudget {
    pub const DEFAULT_MAX_FILES_PER_BATCH: usize = 512;
    pub const DEFAULT_MAX_BYTES_PER_BATCH: usize = 16 * 1024 * 1024;
    pub const DEFAULT_MAX_ROWS_PER_BATCH: usize = 150_000;

    /// Creates a non-zero resource budget for batch parsing and SQLite writes.
    pub fn new(
        max_files_per_batch: usize,
        max_bytes_per_batch: usize,
        max_rows_per_batch: usize,
    ) -> Result<Self, DomainError> {
        if max_files_per_batch == 0 {
            return Err(DomainError::invalid(
                "max_files_per_batch",
                "must be greater than zero",
            ));
        }
        if max_bytes_per_batch == 0 {
            return Err(DomainError::invalid(
                "max_bytes_per_batch",
                "must be greater than zero",
            ));
        }
        if max_rows_per_batch == 0 {
            return Err(DomainError::invalid(
                "max_rows_per_batch",
                "must be greater than zero",
            ));
        }

        Ok(Self {
            max_files_per_batch,
            max_bytes_per_batch,
            max_rows_per_batch,
        })
    }
}

impl Default for CodeIndexResourceBudget {
    fn default() -> Self {
        Self {
            max_files_per_batch: Self::DEFAULT_MAX_FILES_PER_BATCH,
            max_bytes_per_batch: Self::DEFAULT_MAX_BYTES_PER_BATCH,
            max_rows_per_batch: Self::DEFAULT_MAX_ROWS_PER_BATCH,
        }
    }
}

/// Stable metadata for one resumable repository indexing session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexSession {
    pub repository_id: String,
    pub source_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_resolved_commit_sha: Option<String>,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub full_replace: bool,
    pub total_path_count: usize,
    pub changed_path_count: usize,
    pub skipped_unchanged_count: usize,
    pub deleted_paths: Vec<String>,
    pub tombstones: Vec<CodePathTombstone>,
    pub resource_budget: CodeIndexResourceBudget,
}

/// One bounded parse result committed under a checkpointed index session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexBatch {
    pub repository_id: String,
    pub source_scope: String,
    pub batch_index: usize,
    pub parsed_byte_count: usize,
    pub files: Vec<RepositoryCodeFileRecord>,
    pub symbols: Vec<RepositoryCodeSymbolRecord>,
    pub references: Vec<RepositoryCodeReferenceRecord>,
    pub imports: Vec<CodeImportRecord>,
    pub dependencies: Vec<CodeDependencyRecord>,
    pub feature_flags: Vec<CodeFeatureFlagRecord>,
    pub chunks: Vec<RepositoryCodeChunkRecord>,
    pub diagnostics: Vec<CodeFileDiagnostic>,
}

impl CodeIndexBatch {
    /// Counts mutable SQLite rows written by this batch.
    pub fn row_count(&self) -> usize {
        self.files
            .len()
            .saturating_add(self.symbols.len())
            .saturating_add(self.references.len())
            .saturating_add(self.imports.len())
            .saturating_add(self.dependencies.len())
            .saturating_add(self.feature_flags.len())
            .saturating_add(self.chunks.len())
            .saturating_add(self.diagnostics.len())
    }
}

/// Durable progress checkpoint for a repository indexing session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexCheckpoint {
    pub repository_id: String,
    pub source_scope: String,
    pub state: String,
    pub total_path_count: usize,
    pub parsed_file_count: usize,
    pub committed_file_count: usize,
    pub committed_symbol_count: usize,
    pub committed_reference_count: usize,
    pub committed_chunk_count: usize,
    pub batch_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_path: Option<String>,
    pub resource_budget: CodeIndexResourceBudget,
    pub updated_at_ms: u64,
}

/// Persistent lifecycle for background code repository index tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeIndexTaskState {
    Queued,
    Running,
    Succeeded,
    Retrying,
    Failed,
    DeadLetter,
    Cancelled,
}

impl CodeIndexTaskState {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Retrying => "retrying",
            Self::Failed => "failed",
            Self::DeadLetter => "dead_letter",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses the stable storage and API representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "retrying" => Ok(Self::Retrying),
            "failed" => Ok(Self::Failed),
            "dead_letter" => Ok(Self::DeadLetter),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(DomainError::invalid(
                "code_index_task_state",
                "unknown code index task state",
            )),
        }
    }

    /// Returns whether the task can still consume executor capacity.
    pub const fn is_unfinished(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Retrying)
    }
}

/// Durable background task for one code repository index request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexTaskRecord {
    pub task_id: String,
    pub repository_id: String,
    pub alias: String,
    pub ref_selector: String,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub source_scope: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub mode: CodeIndexMode,
    pub state: CodeIndexTaskState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_expires_at_ms: Option<u64>,
    pub attempt_count: u32,
    pub next_retry_at_ms: u64,
    pub input_fingerprint: String,
    pub resource_budget: CodeIndexResourceBudget,
    pub payload_json: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_message: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Scope retention result after pruning old repository snapshots.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeScopeRetentionSummary {
    pub repository_id: String,
    pub retained_scope_count: usize,
    pub prunable_scope_count: usize,
    pub pruned_scope_count: usize,
    pub retained_scopes: Vec<String>,
    pub prunable_scopes: Vec<String>,
    pub pruned_scopes: Vec<String>,
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
    pub batch_count: usize,
    pub checkpoint_file_count: usize,
    pub resource_budget: CodeIndexResourceBudget,
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
    pub parse_status_counts: CodeParseStatusCounts,
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
