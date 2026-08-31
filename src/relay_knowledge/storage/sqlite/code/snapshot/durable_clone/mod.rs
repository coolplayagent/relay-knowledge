//! Copies a large incremental base into an unpublished target in fenced pages.

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{
    clock::system_now_millis,
    domain::{
        CodeIncrementalClonePhase, CodeIndexResourceBudget, CodeIndexSnapshot,
        code_snapshot_scope_is_fact_versioned, code_snapshot_scope_matches_identity,
    },
    storage::StorageError,
};

use super::{
    admission,
    scope_tables::{CODE_SCOPE_TABLES, REFERENCE_SEARCH_SCOPE_TABLES},
};
use crate::storage::sqlite::code::lifecycle::publication_fence::PublicationFenceGuard;

mod base;
mod delta;
mod initialization;
mod progress;
mod search_bulk;
mod search_page;
mod table_page;
mod validation;

use initialization::{initialize_progress, require_init_budget};
use validation::validate_progress;

pub(super) use delta::{batched_delta_completion, finish_batched_delta, start_batched_delta};

const MAX_SOURCE_ROWS_PER_PAGE: usize = 32_768;
const MAX_PAGE_BYTES: usize = CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH;
const PAGE_FIXED_MUTATION_ROWS: usize = 4;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "e2e_tests.rs"]
mod e2e_tests;

#[derive(Clone, Debug)]
pub(super) struct CloneIdentity {
    repository_id: String,
    source_scope: String,
    base_resolved_commit_sha: String,
    resolved_commit_sha: String,
    tree_hash: String,
    path_filters_json: String,
    language_filters_json: String,
    delta_digest: String,
    affected_paths: BTreeSet<String>,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    resource_budget: CodeIndexResourceBudget,
}

#[derive(Clone, Debug)]
pub(super) struct CloneSession {
    pub(super) identity: CloneIdentity,
    pub(super) max_steps: usize,
    pub(super) initialized: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CloneCompletion {
    pub(super) task_id: String,
    pub(super) checkpoint_state: String,
    pub(super) cloned_file_count: usize,
    pub(super) cloned_symbol_count: usize,
    pub(super) cloned_reference_count: usize,
    pub(super) cloned_chunk_count: usize,
    pub(super) base_source_fact_row_upper_bound: usize,
    pub(super) completed_page_ordinal: usize,
    pub(super) terminal_cleanup_rows: usize,
    pub(super) terminal_cleanup_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CloneAdvance {
    Pending { completed_steps: usize },
    CloneComplete,
}

struct CloneBaseHeader<'a> {
    source_scope: &'a str,
    manifest_reference_count: usize,
    manifest_group_count: usize,
    source_fact_row_upper_bound: usize,
}

impl CloneIdentity {
    pub(super) fn from_snapshot(
        snapshot: &CodeIndexSnapshot,
        delta_digest: String,
        resource_budget: CodeIndexResourceBudget,
    ) -> Result<Self, StorageError> {
        let base_resolved_commit_sha = snapshot
            .base_resolved_commit_sha
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                StorageError::InvalidInput(
                    "durable incremental clone requires a non-empty base commit".to_owned(),
                )
            })?;
        let affected_paths = snapshot
            .files
            .iter()
            .map(|file| file.path.clone())
            .chain(snapshot.deleted_paths.iter().cloned())
            .collect::<BTreeSet<_>>();
        Ok(Self {
            repository_id: snapshot.repository_id.clone(),
            source_scope: snapshot.source_scope.clone(),
            base_resolved_commit_sha: base_resolved_commit_sha.to_owned(),
            resolved_commit_sha: snapshot.resolved_commit_sha.clone(),
            tree_hash: snapshot.tree_hash.clone(),
            path_filters_json: serde_json::to_string(&snapshot.path_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            language_filters_json: serde_json::to_string(&snapshot.language_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            delta_digest,
            affected_paths,
            path_filters: snapshot.path_filters.clone(),
            language_filters: snapshot.language_filters.clone(),
            resource_budget,
        })
    }
}

pub(super) fn begin_or_resume(
    connection: &mut rusqlite::Connection,
    snapshot: &CodeIndexSnapshot,
    guard: &PublicationFenceGuard,
) -> Result<CloneSession, StorageError> {
    if snapshot.full_replace {
        return Err(StorageError::InvalidInput(
            "durable incremental clone cannot stage a full snapshot".to_owned(),
        ));
    }
    guard.validate_repository(&snapshot.repository_id)?;
    let budget = guard.resource_budget(connection)?;
    require_delta_path_budget(snapshot, budget)?;
    let identity = CloneIdentity::from_snapshot(
        snapshot,
        admission::snapshot_delta_digest(snapshot)?,
        budget,
    )?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    guard.validate(&transaction)?;
    validate_partitioned_target(&transaction, guard, &identity)?;
    if guard.resource_budget(&transaction)? != budget {
        return Err(StorageError::Invariant(format!(
            "incremental clone task '{}' changed its resource budget",
            guard.task_id()
        )));
    }
    admission::require_no_workspace_projection(&transaction, snapshot)?;
    let base_scope = base::resolve_bounded_scope(&transaction, &identity)?;
    let base_step_proof = base::step_proof(&transaction, &base_scope)?;
    let base_manifest = base::manifest_header(&transaction, &base_scope)?;
    let base_header = CloneBaseHeader {
        source_scope: &base_scope,
        manifest_reference_count: base_manifest.0,
        manifest_group_count: base_manifest.1,
        source_fact_row_upper_bound: base_step_proof.source_fact_row_upper_bound,
    };
    if budget.max_rows_per_batch < base_step_proof.max_rows_per_batch
        || budget.max_bytes_per_batch < base_step_proof.max_bytes_per_batch
        || base_step_proof.max_bytes_per_batch > MAX_PAGE_BYTES
    {
        return Err(StorageError::CapacityExceeded(format!(
            "incremental clone task '{}' has a row or byte quantum smaller than its immutable base",
            guard.task_id()
        )));
    }
    let persisted = progress::load(&transaction, &snapshot.source_scope)?;
    let initialized = persisted.is_none();
    match persisted {
        Some(progress) => {
            validate_progress(
                &transaction,
                &progress,
                &identity,
                &base_scope,
                guard,
                budget,
            )?;
            progress
        }
        None => {
            require_init_budget(&identity, &base_scope, guard.task_id(), budget)?;
            initialize_progress(
                &transaction,
                snapshot,
                &identity,
                &base_header,
                guard,
                budget,
            )?
        }
    };
    validate_affected_paths(&transaction, &identity, budget)?;
    let max_steps = base_step_proof.max_steps;
    // A new worktree target exists only after initialization stages its empty scope. Rebinding in
    // this transaction prevents both an unowned staged target and a rebound task without progress.
    guard.validate_target_scope(&transaction, &identity.source_scope)?;
    guard.validate(&transaction)?;
    validate_partitioned_target(&transaction, guard, &identity)?;
    transaction.commit()?;

    Ok(CloneSession {
        identity,
        max_steps,
        initialized,
    })
}

pub(super) fn advance(
    connection: &mut rusqlite::Connection,
    identity: &CloneIdentity,
    guard: &PublicationFenceGuard,
    max_steps: usize,
) -> Result<CloneAdvance, StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    guard.validate_target_scope(&transaction, &identity.source_scope)?;
    guard.validate(&transaction)?;
    validate_partitioned_target(&transaction, guard, identity)?;
    let budget = guard.resource_budget(&transaction)?;
    let current = progress::load(&transaction, &identity.source_scope)?.ok_or_else(|| {
        StorageError::Invariant(format!(
            "incremental clone progress for scope '{}' disappeared",
            identity.source_scope
        ))
    })?;
    validate_progress(
        &transaction,
        &current,
        identity,
        &current.base_scope,
        guard,
        budget,
    )?;
    if current.completed_page_ordinal > max_steps
        || (current.phase != progress::PHASE_CLONE_COMPLETE
            && current.completed_page_ordinal == max_steps)
    {
        return Err(StorageError::Invariant(format!(
            "incremental clone for scope '{}' exhausted its durable step proof before the next write",
            identity.source_scope
        )));
    }
    let outcome = match current.typed_phase()? {
        CodeIncrementalClonePhase::Tables => {
            table_page::advance(&transaction, &current, identity, now_millis()?)?;
            CloneAdvance::Pending {
                completed_steps: current.completed_page_ordinal.saturating_add(1),
            }
        }
        CodeIncrementalClonePhase::Search => {
            search_page::advance(&transaction, &current, identity, now_millis()?)?;
            CloneAdvance::Pending {
                completed_steps: current.completed_page_ordinal.saturating_add(1),
            }
        }
        CodeIncrementalClonePhase::CloneComplete => CloneAdvance::CloneComplete,
    };
    guard.validate_target_scope(&transaction, &identity.source_scope)?;
    guard.validate(&transaction)?;
    validate_partitioned_target(&transaction, guard, identity)?;
    transaction.commit()?;
    Ok(outcome)
}

pub(super) fn validate_clone_complete(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
    identity: &CloneIdentity,
    guard: &PublicationFenceGuard,
    budget: CodeIndexResourceBudget,
) -> Result<CloneCompletion, StorageError> {
    let progress = progress::load(transaction, &snapshot.source_scope)?.ok_or_else(|| {
        StorageError::Invariant(format!(
            "incremental clone progress for scope '{}' disappeared before delta apply",
            snapshot.source_scope
        ))
    })?;
    validate_progress(
        transaction,
        &progress,
        identity,
        &progress.base_scope,
        guard,
        budget,
    )?;
    if progress.phase != progress::PHASE_CLONE_COMPLETE {
        return Err(StorageError::Invariant(format!(
            "incremental clone scope '{}' cannot apply its delta from phase '{}'",
            snapshot.source_scope, progress.phase
        )));
    }
    let checkpoint_state = progress::checkpoint_state(&progress)?;
    delta::clone_completion(progress, checkpoint_state, &identity.affected_paths)
}

pub(super) fn remove_after_delta(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    let current = progress::load(transaction, source_scope)?.ok_or_else(|| {
        StorageError::Invariant(format!(
            "incremental clone progress for scope '{source_scope}' disappeared during delta apply"
        ))
    })?;
    if current.phase != progress::PHASE_CLONE_COMPLETE {
        return Err(StorageError::Invariant(format!(
            "incremental clone scope '{source_scope}' cannot publish delta from phase '{}'",
            current.phase
        )));
    }
    let checkpoint_state = transaction.query_row(
        "SELECT state FROM code_repository_index_checkpoints WHERE source_scope = ?1",
        [source_scope],
        |row| row.get::<_, String>(0),
    )?;
    if checkpoint_state != super::durable_handoff::FINALIZATION_HANDOFF_STATE {
        return Err(StorageError::Invariant(format!(
            "incremental clone scope '{source_scope}' cannot remove its owner before the finalization handoff"
        )));
    }
    let changed = transaction.execute(
        "DELETE FROM code_repository_incremental_clone_progress
         WHERE source_scope = ?1 AND phase = 'clone_complete'
           AND completed_page_ordinal = ?2",
        params![source_scope, current.completed_page_ordinal],
    )?;
    if changed == 1 {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "incremental clone owner for scope '{source_scope}' changed during delta publication"
    )))
}

fn require_delta_path_budget(
    snapshot: &CodeIndexSnapshot,
    budget: CodeIndexResourceBudget,
) -> Result<(), StorageError> {
    let affected = snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .chain(snapshot.deleted_paths.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    if affected.len() > budget.max_files_per_batch {
        return Err(clone_capacity_error(&snapshot.source_scope));
    }
    Ok(())
}

fn validate_affected_paths(
    transaction: &Transaction<'_>,
    identity: &CloneIdentity,
    budget: CodeIndexResourceBudget,
) -> Result<(), StorageError> {
    let limit = i64::try_from(budget.max_files_per_batch.saturating_add(1))
        .map_err(|_| clone_capacity_error(&identity.source_scope))?;
    let mut statement = transaction.prepare(
        "SELECT path
         FROM code_repository_incremental_clone_affected_paths
         WHERE source_scope = ?1
         ORDER BY path
         LIMIT ?2",
    )?;
    let persisted = statement
        .query_map(params![identity.source_scope, limit], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<BTreeSet<_>, _>>()?;
    if persisted == identity.affected_paths {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "incremental clone affected-path owner for scope '{}' changed",
        identity.source_scope
    )))
}

fn validate_base_scope(
    transaction: &Transaction<'_>,
    identity: &CloneIdentity,
    base_scope: &str,
) -> Result<(), StorageError> {
    let candidate = transaction
        .query_row(
            "SELECT scope.tree_hash, scope.path_filters_json, scope.language_filters_json
             FROM code_repository_scopes scope
             JOIN code_repository_index_checkpoints checkpoint
               ON checkpoint.source_scope = scope.source_scope
              AND checkpoint.repository_id = scope.repository_id
              AND checkpoint.tree_hash = scope.tree_hash
              AND checkpoint.path_filters_json = scope.path_filters_json
              AND checkpoint.language_filters_json = scope.language_filters_json
             WHERE scope.source_scope = ?1 AND scope.repository_id = ?2
               AND scope.stale = 0 AND scope.retiring = 0
               AND checkpoint.state IN ('completed', 'finalizing:partitioned_publish')
               AND NOT EXISTS (
                   SELECT 1 FROM code_repository_scope_gc_jobs job
                   WHERE job.repository_id = scope.repository_id
                     AND job.source_scope = scope.source_scope
               )
               AND (
                   scope.resolved_commit_sha = ?3
                   OR EXISTS (
                       SELECT 1 FROM code_repository_commit_scopes alias
                       WHERE alias.repository_id = scope.repository_id
                         AND alias.resolved_commit_sha = ?3
                         AND alias.source_scope = scope.source_scope
                   )
               )",
            params![
                base_scope,
                identity.repository_id,
                identity.base_resolved_commit_sha
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    super::super::status::parse_json_list(row.get(1)?)?,
                    super::super::status::parse_json_list(row.get(2)?)?,
                ))
            },
        )
        .optional()?;
    if let Some((tree_hash, path_filters, language_filters)) = candidate {
        let requested_paths = super::super::status::canonical_path_filters(&identity.path_filters);
        let requested_languages =
            super::super::status::canonical_filter_values(&identity.language_filters);
        if code_snapshot_scope_is_fact_versioned(base_scope)
            && code_snapshot_scope_matches_identity(
                &identity.repository_id,
                &tree_hash,
                &path_filters,
                &language_filters,
                base_scope,
            )
            && super::super::status::canonical_path_filters(&path_filters) == requested_paths
            && super::super::status::canonical_filter_values(&language_filters)
                == requested_languages
        {
            return Ok(());
        }
    }
    Err(StorageError::Invariant(format!(
        "incremental clone base scope '{base_scope}' is no longer a fresh immutable owner"
    )))
}

fn validate_staged_target(
    transaction: &Transaction<'_>,
    identity: &CloneIdentity,
) -> Result<(), StorageError> {
    let staged = transaction
        .query_row(
            "SELECT stale, repository_id = ?2, resolved_commit_sha = ?3, tree_hash = ?4,
                    path_filters_json = ?5, language_filters_json = ?6
             FROM code_repository_scopes
             WHERE source_scope = ?1 AND retiring = 0",
            params![
                identity.source_scope,
                identity.repository_id,
                identity.resolved_commit_sha,
                identity.tree_hash,
                identity.path_filters_json,
                identity.language_filters_json,
            ],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, bool>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((stale, repository, commit, tree, paths, languages)) = staged else {
        return Err(StorageError::Invariant(format!(
            "incremental clone target scope '{}' is not staged",
            identity.source_scope
        )));
    };
    if !stale || !repository || !commit || !tree || !paths || !languages {
        return Err(StorageError::Invariant(format!(
            "incremental clone target scope '{}' changed while staged",
            identity.source_scope
        )));
    }
    let active = transaction.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM code_repositories
             WHERE repository_id = ?1 AND last_indexed_scope_id = ?2
         )",
        params![identity.repository_id, identity.source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    if active {
        return Err(StorageError::Invariant(format!(
            "incremental clone target scope '{}' became active before its delta was applied",
            identity.source_scope
        )));
    }
    Ok(())
}

fn validate_partitioned_target(
    transaction: &Transaction<'_>,
    guard: &PublicationFenceGuard,
    identity: &CloneIdentity,
) -> Result<(), StorageError> {
    if guard.authority_is_local() {
        return Ok(());
    }
    guard.validate_partitioned_staged_scope(
        transaction,
        &identity.repository_id,
        &identity.source_scope,
    )
}

pub(super) fn all_clone_tables()
-> impl Iterator<Item = &'static super::scope_tables::CodeScopeTable> {
    CODE_SCOPE_TABLES
        .iter()
        .chain(REFERENCE_SEARCH_SCOPE_TABLES.iter().take(1))
}

pub(super) fn table_at(ordinal: usize) -> Option<&'static super::scope_tables::CodeScopeTable> {
    all_clone_tables().nth(ordinal)
}

pub(super) fn table_count() -> usize {
    CODE_SCOPE_TABLES.len() + 1
}

fn source_row_budget(
    progress: &progress::CloneProgress,
    identity: &CloneIdentity,
    row_multiplier: usize,
    byte_multiplier: usize,
) -> Result<(usize, usize), StorageError> {
    let row_budget = progress
        .page_row_limit
        .saturating_sub(PAGE_FIXED_MUTATION_ROWS)
        / row_multiplier;
    let byte_budget = progress
        .page_byte_limit
        .saturating_sub(page_control_bytes(progress, identity)?)
        / byte_multiplier;
    let row_budget = row_budget.min(MAX_SOURCE_ROWS_PER_PAGE);
    if row_budget == 0 || byte_budget == 0 {
        return Err(clone_capacity_error(&progress.source_scope));
    }
    Ok((row_budget, byte_budget))
}

fn durable_page_byte_limit(budget: CodeIndexResourceBudget) -> usize {
    budget.max_bytes_per_batch.min(MAX_PAGE_BYTES)
}

fn require_page_budget(
    progress: &progress::CloneProgress,
    identity: &CloneIdentity,
    source_rows: usize,
    source_bytes: usize,
    row_multiplier: usize,
    byte_multiplier: usize,
) -> Result<(), StorageError> {
    let rows = source_rows
        .checked_mul(row_multiplier)
        .and_then(|value| value.checked_add(PAGE_FIXED_MUTATION_ROWS))
        .ok_or_else(|| clone_capacity_error(&progress.source_scope))?;
    let control_bytes = page_control_bytes(progress, identity)?;
    let bytes = source_bytes
        .checked_mul(byte_multiplier)
        .and_then(|value| value.checked_add(control_bytes))
        .ok_or_else(|| clone_capacity_error(&progress.source_scope))?;
    if rows > progress.page_row_limit || bytes > progress.page_byte_limit {
        return Err(clone_capacity_error(&progress.source_scope));
    }
    Ok(())
}

fn page_control_bytes(
    progress: &progress::CloneProgress,
    identity: &CloneIdentity,
) -> Result<usize, StorageError> {
    let checkpoint_state = progress::checkpoint_state(progress)?;
    let resource_budget_json = serde_json::to_string(&identity.resource_budget)
        .map_err(|error| StorageError::Invariant(error.to_string()))?;
    let values = [
        // Both rows persist their owner identity.  Count these values once for the progress row
        // and once for the checkpoint row rather than treating the shared values as shared
        // storage.
        progress.source_scope.as_str(),
        progress.repository_id.as_str(),
        progress.base_scope.as_str(),
        progress.task_id.as_str(),
        progress.delta_digest.as_str(),
        progress.phase.as_str(),
        progress.cursor_key.as_deref().unwrap_or_default(),
        progress.cursor_tiebreaker.as_deref().unwrap_or_default(),
        progress.source_scope.as_str(),
        progress.repository_id.as_str(),
        identity.resolved_commit_sha.as_str(),
        identity.tree_hash.as_str(),
        identity.path_filters_json.as_str(),
        identity.language_filters_json.as_str(),
        checkpoint_state.as_str(),
        resource_budget_json.as_str(),
    ];
    values.iter().try_fold(
        admission::ROW_STORAGE_OVERHEAD_BYTES
            .saturating_mul(2)
            .saturating_add(36 * 8)
            .saturating_add(9),
        |bytes, value| {
            bytes
                .checked_add(value.len())
                .ok_or_else(|| clone_capacity_error(&progress.source_scope))
        },
    )
}

pub(super) fn clone_capacity_error(source_scope: &str) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "incremental clone page for scope '{source_scope}' exceeds its durable row or byte budget"
    ))
}

fn now_millis() -> Result<u64, StorageError> {
    system_now_millis().map_err(|error| StorageError::Invariant(error.to_string()))
}
