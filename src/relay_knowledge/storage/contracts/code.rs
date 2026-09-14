//! Storage contracts for code repository indexes.

use crate::domain::CodeIndexSummary;

use super::{FrameworkGraphStore, StorageError};

mod catalog;
mod projection;
mod publication;
mod query;
mod repository_set;
mod retention;
mod source;
mod task;

pub use catalog::RepositoryCatalogStore;
pub use projection::SoftwareProjectionStore;
pub use publication::CodeIndexPublicationStore;
pub use query::CodeQueryReadStore;
pub use repository_set::CodeRepositorySetStore;
pub use retention::CodeScopeRetentionStore;
pub use source::CodeIndexSourceStore;
pub use task::CodeIndexTaskStore;

/// Default error text for stores that do not support code task lease recovery.
pub const CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE: &str =
    "code index task lease recovery is unavailable";

/// Default error text for stores that do not support code task lease renewal.
pub const CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE: &str =
    "code index task lease renewal is unavailable";

/// Stable coarse states in the durable code-index finalization plan.
pub const CODE_INDEX_FINALIZATION_COARSE_PHASE_COUNT: usize = 11;

/// Hard bound for missing index units, coarse phases, and terminal observation.
pub const CODE_INDEX_FINALIZATION_MAX_STEPS: usize = crate::domain::CODE_QUERY_INDEX_PLAN_UNIT_COUNT
    + CODE_INDEX_FINALIZATION_COARSE_PHASE_COUNT
    + 2;

/// Derives the hard finalization quantum bound including worst-case
/// byte-limited reference resolution plus reference-search cleanup, group
/// discovery, and build pages.
pub fn code_index_finalization_max_steps(
    committed_reference_count: usize,
) -> Result<usize, StorageError> {
    committed_reference_count
        .checked_mul(4)
        .and_then(|pages| pages.checked_add(CODE_INDEX_FINALIZATION_MAX_STEPS + 6))
        .ok_or_else(|| {
            StorageError::CapacityExceeded(
                "reference-resolution and search finalization step bound exceeds platform capacity"
                    .to_owned(),
            )
        })
}

/// Result of advancing one durable code-index finalization writer quantum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeIndexFinalizationStep {
    Pending { checkpoint_state: String },
    Ready(Box<CodeIndexSummary>),
}

/// Diff-derived inputs used to seed code impact expansion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeImpactChanges {
    pub paths: Vec<String>,
    pub deleted_symbol_names: Vec<String>,
}

/// Bounded repository-set overlay keys needed to decorate retrieval candidates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeRepositorySetEdgeSelector {
    pub origin_files: Vec<(String, String)>,
    pub target_records: Vec<(String, String, String)>,
}

/// New background code index task to persist or deduplicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskSeed {
    pub repository_id: String,
    pub alias: String,
    pub ref_selector: String,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub source_scope: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub mode: crate::domain::CodeIndexMode,
    pub input_fingerprint: String,
    pub resource_budget: crate::domain::CodeIndexResourceBudget,
    pub payload_json: String,
    pub now_ms: u64,
}

/// Lease acquisition request for one background code index task.
///
/// `now_ms` is a caller observation. Storage samples authoritative execution
/// time only after obtaining its writer lock and rejects future observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskClaimRequest {
    pub task_id: Option<String>,
    pub lease_owner: String,
    pub lease_duration_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Strict renewal request for one still-live fenced code-index attempt.
///
/// Expiry is irrevocable. `now_ms` is only the caller's observation; storage
/// samples authoritative time after acquiring its writer lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskLeaseRenewal {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub publication_generation: u64,
    pub lease_duration_ms: u64,
    /// Caller-observed time used only to reject future/rollback observations.
    /// Storage samples authoritative time after obtaining its writer lock.
    pub now_ms: u64,
}

/// Active code-index task lease used by service startup recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskLeaseRecord {
    pub task_id: String,
    pub lease_owner: String,
    pub lease_expires_at_ms: Option<u64>,
    pub attempt_count: u32,
    pub publication_generation: u64,
}

/// Durable task target whose already-complete publication may be adopted by
/// a later fenced attempt without rebuilding code or software facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexPublicationTarget {
    pub task_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
}

/// Recovery request carrying the exact running leases observed as orphaned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskLeaseRecovery {
    pub leases: Vec<CodeIndexTaskLeaseRecord>,
    pub now_ms: u64,
    pub max_attempts: u32,
    pub error_kind: String,
    pub error_message: String,
}

/// Completion report guarded by task lease and attempt token.
///
/// Expiry is irrevocable. `now_ms` is only the caller's observation; storage
/// samples authoritative time after acquiring its writer lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskCompletion {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub publication_generation: u64,
    pub now_ms: u64,
}

/// Failure report for retry and dead-letter handling.
///
/// Expiry is irrevocable. `now_ms` is only the caller's observation; storage
/// samples authoritative time after acquiring its writer lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskFailure {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub publication_generation: u64,
    pub error_kind: String,
    pub error_message: String,
    pub retry_backoff_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Scope retention request after a repository index completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeScopeRetentionRequest {
    pub repository_id: String,
    pub active_scope: String,
    pub retain_recent_successful_scopes: usize,
    /// Whole-repository wall-clock cutoff used for legacy publications and checkpoints.
    pub repository_retention_cutoff_ms: Option<u64>,
    /// Publication generation current when whole-repository retention was scheduled.
    /// Newer generations remain protected even when timestamps share one millisecond.
    pub repository_retention_cutoff_generation: Option<u64>,
    /// Scope that was current when whole-repository retention was scheduled.
    pub repository_retention_initial_scope: Option<String>,
}

/// New repository set metadata to persist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetSeed {
    pub alias: String,
    pub description: Option<String>,
    pub default_ref_policy_json: String,
    pub now_ms: u64,
}

/// New or replaced repository-set member pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetMemberSeed {
    pub set_alias: String,
    pub repository_id: String,
    pub repository_alias: String,
    pub ref_selector: String,
    pub resolved_commit_sha: String,
    pub source_scope: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub priority: i32,
}

/// Repository-set overlay refresh task to persist or deduplicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetRefreshTaskSeed {
    pub set_id: String,
    pub set_alias: String,
    pub input_fingerprint: String,
    pub now_ms: u64,
}

/// Lease acquisition request for one repository-set overlay task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetRefreshTaskClaimRequest {
    pub task_id: Option<String>,
    pub lease_owner: String,
    pub lease_duration_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Attempt-scoped authority required to publish a repository-set overlay.
///
/// Storage implementations must validate every field against a live running
/// task in the same transaction that replaces the overlay rows. A worker that
/// loses its lease or is superseded by a later attempt therefore cannot publish
/// a stale DELETE/INSERT sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetRefreshPublication {
    pub task_id: String,
    pub set_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub member_replacements: Vec<CodeRepositorySetMemberSeed>,
}

/// Completion report guarded by task lease and attempt token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetRefreshTaskCompletion {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub now_ms: u64,
}

/// Failure report for retry and dead-letter handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetRefreshTaskFailure {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub error_kind: String,
    pub error_message: String,
    pub retry_backoff_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Compatibility facade that combines all persisted code repository
/// capabilities. New workflows depend on the narrow capability they consume.
pub trait CodeRepositoryStore:
    FrameworkGraphStore
    + RepositoryCatalogStore
    + CodeIndexTaskStore
    + CodeScopeRetentionStore
    + CodeIndexSourceStore
    + CodeIndexPublicationStore
    + CodeQueryReadStore
    + SoftwareProjectionStore
    + CodeRepositorySetStore
{
}

impl<T> CodeRepositoryStore for T where
    T: FrameworkGraphStore
        + RepositoryCatalogStore
        + CodeIndexTaskStore
        + CodeScopeRetentionStore
        + CodeIndexSourceStore
        + CodeIndexPublicationStore
        + CodeQueryReadStore
        + SoftwareProjectionStore
        + CodeRepositorySetStore
        + ?Sized
{
}

#[cfg(test)]
#[path = "code_tests.rs"]
mod tests;
