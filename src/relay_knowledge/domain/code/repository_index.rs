use serde::{Deserialize, Serialize};

use super::{
    code_dependency::CodeDependencyRecord,
    code_repository::{
        CodeCallRecord, CodeFeatureFlagRecord, CodeFileDiagnostic, CodeImportRecord, CodeIndexMode,
        CodePathTombstone, CodeRouteRecord, RepositoryCodeChunkRecord, RepositoryCodeFileRecord,
        RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
    },
    code_workspace::CodeMonorepoWorkspace,
    error::DomainError,
};

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
    pub routes: Vec<CodeRouteRecord>,
    pub chunks: Vec<RepositoryCodeChunkRecord>,
    #[serde(default)]
    pub workspaces: Vec<CodeMonorepoWorkspace>,
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
    #[serde(default)]
    pub workspaces: Vec<CodeMonorepoWorkspace>,
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
    pub routes: Vec<CodeRouteRecord>,
    pub chunks: Vec<RepositoryCodeChunkRecord>,
    pub diagnostics: Vec<CodeFileDiagnostic>,
}

impl CodeIndexBatch {
    pub fn row_count(&self) -> usize {
        self.files
            .len()
            .saturating_add(self.symbols.len())
            .saturating_add(self.references.len())
            .saturating_add(self.imports.len())
            .saturating_add(self.dependencies.len())
            .saturating_add(self.feature_flags.len())
            .saturating_add(self.routes.len())
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

/// Aggregated durable queue state for background code-index tasks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexTaskQueueStatus {
    pub queued_task_count: usize,
    pub running_task_count: usize,
    pub retrying_task_count: usize,
    pub dead_letter_task_count: usize,
    pub running_lease_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
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
    #[serde(default)]
    pub handwritten_symbol_count: usize,
    #[serde(default)]
    pub generated_symbol_count: usize,
    pub reference_count: usize,
    pub chunk_count: usize,
    pub degraded_file_count: usize,
    pub progress: CodeIndexProgressSummary,
}
