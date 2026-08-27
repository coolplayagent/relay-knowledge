//! Defines durable repository-index snapshots, batches, tasks, checkpoints, and progress.

use serde::{Deserialize, Serialize};

use super::{
    CodeFrameworkEdgeRecord, CodeFrameworkNodeRecord,
    dependencies::CodeDependencyRecord,
    error::DomainError,
    repository::{
        CodeCallRecord, CodeFeatureFlagRecord, CodeFileDiagnostic, CodeImportRecord, CodeIndexMode,
        CodePathTombstone, CodeRouteRecord, RepositoryCodeChunkRecord, RepositoryCodeFileRecord,
        RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
    },
    workspace::CodeMonorepoWorkspace,
};

mod incremental_clone;
mod reference_resolution;

pub(crate) use self::incremental_clone::{
    CodeIncrementalClonePhase, code_incremental_clone, code_incremental_clone_state,
};
pub(crate) use self::reference_resolution::{
    CodeReferenceResolution, CodeReferenceResolutionQueryIndexRepair, CodeReferenceResolutionStage,
    code_reference_resolution, code_reference_resolution_cursor_digest,
    code_reference_resolution_query_index_repair,
    code_reference_resolution_query_index_repair_state, code_reference_resolution_state,
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
    #[serde(default)]
    pub framework_nodes: Vec<CodeFrameworkNodeRecord>,
    #[serde(default)]
    pub framework_edges: Vec<CodeFrameworkEdgeRecord>,
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
    /// Paths that will be re-inserted during an incremental session.
    /// Used by `begin_session_once` to exclude them from the historical
    /// scope clone so stale rows are never copied into the new scope.
    /// Empty for full-replace sessions.
    #[serde(default)]
    pub changed_paths: Vec<String>,
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
    #[serde(default)]
    pub framework_nodes: Vec<CodeFrameworkNodeRecord>,
    #[serde(default)]
    pub framework_edges: Vec<CodeFrameworkEdgeRecord>,
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
            .saturating_add(self.framework_nodes.len())
            .saturating_add(self.framework_edges.len())
            .saturating_add(self.routes.len())
            .saturating_add(self.chunks.len())
            .saturating_add(self.diagnostics.len())
    }
}

/// Version of the stable deferred query-index finalization plan.
///
/// Reordering, adding, or removing a storage descriptor requires a version
/// bump plus an explicit recovery policy for checkpoints written by the old
/// plan.
pub(crate) const CODE_QUERY_INDEX_PLAN_VERSION: u32 = 3;

/// Number of stable units in the current deferred query-index plan.
pub(crate) const CODE_QUERY_INDEX_PLAN_UNIT_COUNT: usize = 17;

const LEGACY_CODE_QUERY_INDEX_PLAN_V1: u32 = 1;
const LEGACY_CODE_QUERY_INDEX_PLAN_V1_UNIT_COUNT: usize = 16;
const LEGACY_CODE_QUERY_INDEX_PLAN_V2: u32 = 2;
const LEGACY_CODE_QUERY_INDEX_PLAN_V2_UNIT_COUNT: usize = 17;

const CODE_QUERY_INDEX_SUBPHASE_PREFIX: &str = "finalizing:build_query_indexes";
const CODE_QUERY_INDEX_REPAIR_PREFIX: &str = "finalizing:query_index_repair";
const CODE_REFERENCE_SEARCH_REBUILD_PREFIX: &str = "finalizing:rebuild_reference_search";
const CODE_REFERENCE_SEARCH_REBUILD_VERSION: u32 = 2;

/// Durable stage within an unpublished reference-search rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodeReferenceSearchRebuildStage {
    Cleanup,
    Discover,
    Build,
}

impl CodeReferenceSearchRebuildStage {
    const fn code(self) -> &'static str {
        match self {
            Self::Cleanup => "cleanup",
            Self::Discover => "discover",
            Self::Build => "build",
        }
    }

    fn parse(code: &str) -> Option<Self> {
        match code {
            "cleanup" => Some(Self::Cleanup),
            "discover" => Some(Self::Discover),
            "build" => Some(Self::Build),
            _ => None,
        }
    }
}

/// Parsed canonical progress token for a staged reference-search rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CodeReferenceSearchRebuild {
    pub(crate) protocol_version: u32,
    pub(crate) stage: CodeReferenceSearchRebuildStage,
    pub(crate) completed_page_ordinal: usize,
}

impl CodeReferenceSearchRebuild {
    /// Restores the exact protocol version parsed from durable progress.
    ///
    /// Query-index repair must not silently upgrade a version-1 reference
    /// cursor before the reference-search driver has reconciled its matching
    /// version-1 progress row.
    pub(crate) fn checkpoint_state(self) -> Option<String> {
        (matches!(
            self.protocol_version,
            1 | CODE_REFERENCE_SEARCH_REBUILD_VERSION
        ) && !(self.protocol_version == 1
            && self.stage == CodeReferenceSearchRebuildStage::Discover))
            .then(|| {
                format!(
                    "{CODE_REFERENCE_SEARCH_REBUILD_PREFIX}:v{}:{}:{}",
                    self.protocol_version,
                    self.stage.code(),
                    self.completed_page_ordinal
                )
            })
    }
}

/// Durable query-index repair cursor that preserves an exact in-progress
/// reference-search page boundary across a versioned query-index plan upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CodeReferenceSearchQueryIndexRepair {
    pub(crate) plan_version: u32,
    pub(crate) completed_unit: usize,
    pub(crate) reference_search: CodeReferenceSearchRebuild,
}

impl CodeReferenceSearchQueryIndexRepair {
    pub(crate) const fn requires_legacy_retired_prefix(self) -> bool {
        self.plan_version == LEGACY_CODE_QUERY_INDEX_PLAN_V2
    }

    /// Advances without changing either the query-index plan policy or the
    /// nested reference-search protocol carried by the durable cursor.
    pub(crate) fn next_state(self, completed_unit: usize) -> Option<String> {
        code_reference_search_query_index_repair_state_for_version(
            self.plan_version,
            completed_unit,
            self.reference_search,
        )
    }
}

pub(crate) fn code_reference_search_rebuild_state(
    stage: CodeReferenceSearchRebuildStage,
    completed_page_ordinal: usize,
) -> String {
    format!(
        "{CODE_REFERENCE_SEARCH_REBUILD_PREFIX}:v{CODE_REFERENCE_SEARCH_REBUILD_VERSION}:{}:{completed_page_ordinal}",
        stage.code()
    )
}

/// Parses canonical current and legacy staged reference-search tokens while
/// preserving their protocol version for recovery.
pub(crate) fn code_reference_search_rebuild(state: &str) -> Option<CodeReferenceSearchRebuild> {
    let suffix = state.strip_prefix(&format!("{CODE_REFERENCE_SEARCH_REBUILD_PREFIX}:v"))?;
    let mut parts = suffix.split(':');
    let version = parts.next()?.parse::<u32>().ok()?;
    let stage = CodeReferenceSearchRebuildStage::parse(parts.next()?)?;
    let completed_page_ordinal = parts.next()?.parse::<usize>().ok()?;
    if !matches!(version, 1 | CODE_REFERENCE_SEARCH_REBUILD_VERSION)
        || (version == 1 && stage == CodeReferenceSearchRebuildStage::Discover)
        || parts.next().is_some()
    {
        return None;
    }
    let canonical = format!(
        "{CODE_REFERENCE_SEARCH_REBUILD_PREFIX}:v{version}:{}:{completed_page_ordinal}",
        stage.code()
    );
    (canonical == state).then_some(CodeReferenceSearchRebuild {
        protocol_version: version,
        stage,
        completed_page_ordinal,
    })
}

pub(crate) fn code_reference_search_query_index_repair_state(
    unit: usize,
    reference_search: CodeReferenceSearchRebuild,
) -> Option<String> {
    code_reference_search_query_index_repair_state_for_version(
        CODE_QUERY_INDEX_PLAN_VERSION,
        unit,
        reference_search,
    )
}

fn code_reference_search_query_index_repair_state_for_version(
    plan_version: u32,
    unit: usize,
    reference_search: CodeReferenceSearchRebuild,
) -> Option<String> {
    (matches!(
        plan_version,
        LEGACY_CODE_QUERY_INDEX_PLAN_V2 | CODE_QUERY_INDEX_PLAN_VERSION
    ) && unit < CODE_QUERY_INDEX_PLAN_UNIT_COUNT
        && matches!(
            reference_search.protocol_version,
            1 | CODE_REFERENCE_SEARCH_REBUILD_VERSION
        )
        && !(reference_search.protocol_version == 1
            && reference_search.stage == CodeReferenceSearchRebuildStage::Discover))
    .then(|| {
        format!(
            "{CODE_QUERY_INDEX_REPAIR_PREFIX}:v{plan_version}:{unit}:resume:reference_search:v{}:{}:{}",
            reference_search.protocol_version,
            reference_search.stage.code(), reference_search.completed_page_ordinal
        )
    })
}

pub(crate) fn code_reference_search_query_index_repair(
    state: &str,
) -> Option<CodeReferenceSearchQueryIndexRepair> {
    let suffix = state.strip_prefix(&format!("{CODE_QUERY_INDEX_REPAIR_PREFIX}:v"))?;
    let (version_and_unit, reference) = suffix.split_once(":resume:reference_search:v")?;
    let (version, unit) = version_and_unit.split_once(':')?;
    let version = version.parse::<u32>().ok()?;
    let unit = unit.parse::<usize>().ok()?;
    let mut reference = reference.split(':');
    let reference_version = reference.next()?.parse::<u32>().ok()?;
    let stage = CodeReferenceSearchRebuildStage::parse(reference.next()?)?;
    let completed_page_ordinal = reference.next()?.parse::<usize>().ok()?;
    if !matches!(
        version,
        CODE_QUERY_INDEX_PLAN_VERSION | LEGACY_CODE_QUERY_INDEX_PLAN_V2
    ) || unit >= CODE_QUERY_INDEX_PLAN_UNIT_COUNT
        || !matches!(reference_version, 1 | CODE_REFERENCE_SEARCH_REBUILD_VERSION)
        || (reference_version == 1 && stage == CodeReferenceSearchRebuildStage::Discover)
        || reference.next().is_some()
    {
        return None;
    }
    let reference_search = CodeReferenceSearchRebuild {
        protocol_version: reference_version,
        stage,
        completed_page_ordinal,
    };
    let canonical = format!(
        "{CODE_QUERY_INDEX_REPAIR_PREFIX}:v{version}:{unit}:resume:reference_search:v{reference_version}:{}:{}",
        reference_search.stage.code(),
        reference_search.completed_page_ordinal
    );
    (canonical == state).then_some(CodeReferenceSearchQueryIndexRepair {
        plan_version: version,
        completed_unit: unit,
        reference_search,
    })
}

/// Stable coarse checkpoint restored after a durable query-index repair.
///
/// These explicit codes are persisted. Reordering the finalization driver must
/// not change them; changing their meaning requires a repair-token version
/// bump and an explicit compatibility policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum CodeQueryIndexRepairResumePhase {
    BuildQueryIndexes = 0,
    ResolveReferences = 1,
    ResolveImports = 2,
    ResolveCallTargets = 3,
    RefreshDependencies = 4,
    RebuildReferenceSearch = 5,
    RebuildCalls = 6,
    PublishScope = 7,
    ResolveWorkspaceImports = 8,
    SoftwareProjection = 9,
    PartitionedPublish = 10,
}

impl CodeQueryIndexRepairResumePhase {
    pub(crate) const ALL: [Self; 11] = [
        Self::BuildQueryIndexes,
        Self::ResolveReferences,
        Self::ResolveImports,
        Self::ResolveCallTargets,
        Self::RefreshDependencies,
        Self::RebuildReferenceSearch,
        Self::RebuildCalls,
        Self::PublishScope,
        Self::ResolveWorkspaceImports,
        Self::SoftwareProjection,
        Self::PartitionedPublish,
    ];

    pub(crate) const fn checkpoint_state(self) -> &'static str {
        match self {
            Self::BuildQueryIndexes => "finalizing:build_query_indexes",
            Self::ResolveReferences => "finalizing:resolve_references",
            Self::ResolveImports => "finalizing:resolve_imports",
            Self::ResolveCallTargets => "finalizing:resolve_call_targets",
            Self::RefreshDependencies => "finalizing:refresh_dependencies",
            Self::RebuildReferenceSearch => "finalizing:rebuild_reference_search",
            Self::RebuildCalls => "finalizing:rebuild_calls",
            Self::PublishScope => "finalizing:publish_scope",
            Self::ResolveWorkspaceImports => "finalizing:resolve_workspace_imports",
            Self::SoftwareProjection => "finalizing:software_projection",
            Self::PartitionedPublish => "finalizing:partitioned_publish",
        }
    }

    pub(crate) fn from_checkpoint_state(state: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|phase| phase.checkpoint_state() == state)
    }

    const fn code(self) -> u8 {
        self as u8
    }

    fn from_code(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|phase| phase.code() == code)
    }
}

/// Parsed durable cursor for an interrupted query-index repair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CodeQueryIndexRepair {
    pub(crate) plan_version: u32,
    pub(crate) completed_unit: usize,
    pub(crate) resume_phase: CodeQueryIndexRepairResumePhase,
}

impl CodeQueryIndexRepair {
    pub(crate) const fn requires_legacy_retired_prefix(self) -> bool {
        self.plan_version == LEGACY_CODE_QUERY_INDEX_PLAN_V2
    }

    /// Advances a repair while retaining the parsed plan version and its
    /// retired-prefix policy across every durable writer quantum.
    pub(crate) fn next_state(self, completed_unit: usize) -> Option<String> {
        code_query_index_repair_state_for_version(
            self.plan_version,
            completed_unit,
            self.resume_phase,
        )
    }
}

/// Parsed durable cursor for one completed query-index plan unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CodeQueryIndexSubphase {
    pub(crate) plan_version: u32,
    pub(crate) completed_unit: usize,
}

impl CodeQueryIndexSubphase {
    pub(crate) const fn requires_legacy_retired_prefix(self) -> bool {
        matches!(
            self.plan_version,
            LEGACY_CODE_QUERY_INDEX_PLAN_V1 | LEGACY_CODE_QUERY_INDEX_PLAN_V2
        )
    }

    /// Advances a parsed cursor without reinterpreting a legacy completed
    /// prefix under the current retired-index policy.
    pub(crate) fn next_state(self, completed_unit: usize) -> Option<String> {
        code_query_index_subphase_state_for_version(self.plan_version, completed_unit)
    }
}

/// Formats the durable checkpoint token for one completed query-index unit.
pub(crate) fn code_query_index_subphase_state(unit: usize) -> Option<String> {
    code_query_index_subphase_state_for_version(CODE_QUERY_INDEX_PLAN_VERSION, unit)
}

fn code_query_index_subphase_state_for_version(plan_version: u32, unit: usize) -> Option<String> {
    let unit_count = match plan_version {
        CODE_QUERY_INDEX_PLAN_VERSION => CODE_QUERY_INDEX_PLAN_UNIT_COUNT,
        LEGACY_CODE_QUERY_INDEX_PLAN_V2 => LEGACY_CODE_QUERY_INDEX_PLAN_V2_UNIT_COUNT,
        LEGACY_CODE_QUERY_INDEX_PLAN_V1 => LEGACY_CODE_QUERY_INDEX_PLAN_V1_UNIT_COUNT,
        _ => return None,
    };
    (unit < unit_count)
        .then(|| format!("{CODE_QUERY_INDEX_SUBPHASE_PREFIX}:v{plan_version}:{unit}"))
}

/// Parses canonical current-plan tokens and compatible version-1/version-2 tokens.
///
/// Version 2 appended unit 16. Version 3 preserves all 17 ordinal identities
/// while retiring unit 1's creation action. The returned plan version must
/// reach prefix validation so no older completed ordinal is reinterpreted.
pub(crate) fn code_query_index_subphase(state: &str) -> Option<CodeQueryIndexSubphase> {
    let suffix = state.strip_prefix(&format!("{CODE_QUERY_INDEX_SUBPHASE_PREFIX}:v"))?;
    let (version, unit) = suffix.split_once(':')?;
    let version = version.parse::<u32>().ok()?;
    let unit = unit.parse::<usize>().ok()?;
    let unit_count = match version {
        CODE_QUERY_INDEX_PLAN_VERSION => CODE_QUERY_INDEX_PLAN_UNIT_COUNT,
        LEGACY_CODE_QUERY_INDEX_PLAN_V2 => LEGACY_CODE_QUERY_INDEX_PLAN_V2_UNIT_COUNT,
        LEGACY_CODE_QUERY_INDEX_PLAN_V1 => LEGACY_CODE_QUERY_INDEX_PLAN_V1_UNIT_COUNT,
        _ => return None,
    };
    if unit >= unit_count {
        return None;
    }
    let canonical = format!("{CODE_QUERY_INDEX_SUBPHASE_PREFIX}:v{version}:{unit}");
    (canonical == state).then_some(CodeQueryIndexSubphase {
        plan_version: version,
        completed_unit: unit,
    })
}

/// Formats a durable query-index repair token that preserves the exact coarse
/// phase already completed by the older writer.
pub(crate) fn code_query_index_repair_state(
    unit: usize,
    resume_phase: CodeQueryIndexRepairResumePhase,
) -> Option<String> {
    code_query_index_repair_state_for_version(CODE_QUERY_INDEX_PLAN_VERSION, unit, resume_phase)
}

fn code_query_index_repair_state_for_version(
    plan_version: u32,
    unit: usize,
    resume_phase: CodeQueryIndexRepairResumePhase,
) -> Option<String> {
    (matches!(
        plan_version,
        LEGACY_CODE_QUERY_INDEX_PLAN_V2 | CODE_QUERY_INDEX_PLAN_VERSION
    ) && unit < CODE_QUERY_INDEX_PLAN_UNIT_COUNT)
        .then(|| {
            format!(
                "{CODE_QUERY_INDEX_REPAIR_PREFIX}:v{plan_version}:{unit}:resume:{}",
                resume_phase.code()
            )
        })
}

/// Parses canonical version-2 and current-plan query-index repair tokens.
pub(crate) fn code_query_index_repair(state: &str) -> Option<CodeQueryIndexRepair> {
    let suffix = state.strip_prefix(&format!("{CODE_QUERY_INDEX_REPAIR_PREFIX}:v"))?;
    let (version_and_unit, resume_code) = suffix.split_once(":resume:")?;
    let (version, unit) = version_and_unit.split_once(':')?;
    let version = version.parse::<u32>().ok()?;
    let unit = unit.parse::<usize>().ok()?;
    let resume_code = resume_code.parse::<u8>().ok()?;
    if !matches!(
        version,
        CODE_QUERY_INDEX_PLAN_VERSION | LEGACY_CODE_QUERY_INDEX_PLAN_V2
    ) || unit >= CODE_QUERY_INDEX_PLAN_UNIT_COUNT
    {
        return None;
    }
    let resume_phase = CodeQueryIndexRepairResumePhase::from_code(resume_code)?;
    let canonical = format!(
        "{CODE_QUERY_INDEX_REPAIR_PREFIX}:v{version}:{unit}:resume:{}",
        resume_phase.code()
    );
    (canonical == state).then_some(CodeQueryIndexRepair {
        plan_version: version,
        completed_unit: unit,
        resume_phase,
    })
}

/// Bounded incremental-work metrics retained across post-delta finalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIncrementalSummaryReceipt {
    pub task_id: String,
    pub base_resolved_commit_sha: String,
    pub changed_path_count: usize,
    pub skipped_unchanged_count: usize,
    pub deleted_path_count: usize,
    pub affected_path_count: usize,
    pub blob_read_count: usize,
    pub parsed_file_count: usize,
    pub sqlite_write_count: usize,
    pub degraded_file_count: usize,
    pub batch_count: usize,
}

impl CodeIncrementalSummaryReceipt {
    pub(crate) fn validate(&self) -> Result<(), DomainError> {
        let affected_surface = self
            .parsed_file_count
            .checked_add(self.deleted_path_count)
            .ok_or_else(|| {
                DomainError::invalid(
                    "incremental_summary",
                    "affected path surface exceeds platform capacity",
                )
            })?;
        let minimum_sqlite_writes = self
            .parsed_file_count
            .checked_add(self.degraded_file_count)
            .ok_or_else(|| {
                DomainError::invalid(
                    "incremental_summary",
                    "minimum SQLite write count exceeds platform capacity",
                )
            })?;
        if self.task_id.trim().is_empty()
            || self.task_id.len() > 1_024
            || self.base_resolved_commit_sha.trim().is_empty()
            || self.base_resolved_commit_sha.len() > 1_024
            || self.blob_read_count != self.parsed_file_count
            || self.parsed_file_count > self.affected_path_count
            || self.deleted_path_count > self.affected_path_count
            || self.affected_path_count > affected_surface
            || self.degraded_file_count > self.parsed_file_count
            || self.sqlite_write_count < minimum_sqlite_writes
            || self.batch_count != 1
        {
            return Err(DomainError::invalid(
                "incremental_summary",
                "durable incremental metrics are inconsistent",
            ));
        }
        Ok(())
    }
}

/// Durable progress checkpoint for a repository indexing session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexCheckpoint {
    pub repository_id: String,
    pub source_scope: String,
    #[serde(default)]
    pub resolved_commit_sha: String,
    #[serde(default)]
    pub tree_hash: String,
    #[serde(default)]
    pub path_filters: Vec<String>,
    #[serde(default)]
    pub language_filters: Vec<String>,
    pub state: String,
    pub total_path_count: usize,
    pub parsed_file_count: usize,
    pub committed_file_count: usize,
    pub committed_symbol_count: usize,
    pub committed_reference_count: usize,
    pub committed_chunk_count: usize,
    #[serde(default)]
    pub committed_fact_row_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incremental_summary: Option<CodeIncrementalSummaryReceipt>,
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
    /// Monotonic repository-local generation assigned when this attempt was claimed.
    #[serde(default)]
    pub publication_generation: u64,
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

/// Attempt-scoped token required to publish code-index state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexPublicationFence {
    pub repository_id: String,
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub generation: u64,
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
    #[serde(default)]
    pub scope_listing_truncated: bool,
    #[serde(default)]
    pub retiring_job_count: usize,
    #[serde(default)]
    pub maintenance_pending: bool,
    pub retained_scopes: Vec<String>,
    pub prunable_scopes: Vec<String>,
    pub pruned_scopes: Vec<String>,
    #[serde(default)]
    pub retiring_jobs: Vec<CodeScopeRetirementJobStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_retention_job: Option<CodeRepositoryRetentionJobStatus>,
}

/// Observable progress for one durable whole-repository index retention job.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryRetentionJobStatus {
    pub repository_id: String,
    pub initial_scope: String,
    pub cutoff_ms: u64,
    #[serde(default)]
    pub cutoff_publication_generation: u64,
    pub phase: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Observable progress for one durable, restart-safe scope retirement job.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeScopeRetirementJobStatus {
    pub repository_id: String,
    pub source_scope: String,
    pub phase: String,
    pub deleted_rows: usize,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_resolved_commit_sha: Option<String>,
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
