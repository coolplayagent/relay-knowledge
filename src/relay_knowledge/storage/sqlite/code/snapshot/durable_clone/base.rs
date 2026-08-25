//! Bounded admission and durable step proof for an immutable clone base.

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    domain::{
        CodeIndexResourceBudget, code_snapshot_scope_is_fact_versioned,
        code_snapshot_scope_matches_identity,
    },
    storage::StorageError,
};

use super::{CloneIdentity, clone_capacity_error, table_count};

const MAX_BASE_SCOPE_CANDIDATES: usize = 32;

#[derive(Debug)]
pub(super) struct StepProof {
    pub(super) max_steps: usize,
    pub(super) source_fact_row_upper_bound: usize,
    pub(super) max_rows_per_batch: usize,
    pub(super) max_bytes_per_batch: usize,
}

pub(super) fn resolve_bounded_scope(
    transaction: &Transaction<'_>,
    identity: &CloneIdentity,
) -> Result<String, StorageError> {
    let active = transaction
        .query_row(
            "SELECT scope.source_scope, scope.tree_hash,
                    scope.path_filters_json, scope.language_filters_json
             FROM code_repositories repository
             JOIN code_repository_scopes scope
               ON scope.source_scope = repository.last_indexed_scope_id
              AND scope.repository_id = repository.repository_id
             WHERE repository.repository_id = ?1
               AND scope.stale = 0 AND scope.retiring = 0
               AND NOT EXISTS (
                   SELECT 1 FROM code_repository_scope_gc_jobs job
                   WHERE job.repository_id = scope.repository_id
                     AND job.source_scope = scope.source_scope
               )
               AND (
                   scope.resolved_commit_sha = ?2
                   OR EXISTS (
                       SELECT 1 FROM code_repository_commit_scopes alias
                       WHERE alias.repository_id = scope.repository_id
                         AND alias.resolved_commit_sha = ?2
                         AND alias.source_scope = scope.source_scope
                   )
               )",
            params![identity.repository_id, identity.base_resolved_commit_sha],
            scope_identity_row,
        )
        .optional()?;
    if let Some(candidate) = active.filter(|candidate| identity_matches(identity, candidate)) {
        return require_distinct(identity, candidate.0);
    }

    let limit = i64::try_from(MAX_BASE_SCOPE_CANDIDATES + 1)
        .map_err(|_| clone_capacity_error(&identity.source_scope))?;
    let mut statement = transaction.prepare(
        "SELECT scope.source_scope, scope.tree_hash,
                scope.path_filters_json, scope.language_filters_json
         FROM code_repository_commit_scopes alias
         JOIN code_repository_scopes scope
           ON scope.source_scope = alias.source_scope
          AND scope.repository_id = alias.repository_id
         WHERE alias.repository_id = ?1 AND alias.resolved_commit_sha = ?2
           AND scope.stale = 0 AND scope.retiring = 0
           AND NOT EXISTS (
               SELECT 1 FROM code_repository_scope_gc_jobs job
               WHERE job.repository_id = scope.repository_id
                 AND job.source_scope = scope.source_scope
           )
         ORDER BY alias.source_scope DESC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(
        params![
            identity.repository_id,
            identity.base_resolved_commit_sha,
            limit
        ],
        scope_identity_row,
    )?;
    let candidates = rows.collect::<Result<Vec<_>, _>>()?;
    if candidates.len() > MAX_BASE_SCOPE_CANDIDATES {
        return Err(StorageError::CapacityExceeded(format!(
            "incremental clone base commit '{}' has more than {MAX_BASE_SCOPE_CANDIDATES} retained scope candidates",
            identity.base_resolved_commit_sha
        )));
    }
    candidates
        .into_iter()
        .find(|candidate| identity_matches(identity, candidate))
        .map(|candidate| require_distinct(identity, candidate.0))
        .transpose()?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "repository '{}' has no bounded fresh fact-versioned scope for base commit '{}'",
                identity.repository_id, identity.base_resolved_commit_sha
            ))
        })
}

fn scope_identity_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, String, Vec<String>, Vec<String>)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        super::super::super::status::parse_json_list(row.get(2)?)?,
        super::super::super::status::parse_json_list(row.get(3)?)?,
    ))
}

fn identity_matches(
    identity: &CloneIdentity,
    candidate: &(String, String, Vec<String>, Vec<String>),
) -> bool {
    code_snapshot_scope_is_fact_versioned(&candidate.0)
        && code_snapshot_scope_matches_identity(
            &identity.repository_id,
            &candidate.1,
            &candidate.2,
            &candidate.3,
            &candidate.0,
        )
        && super::super::super::status::canonical_path_filters(&candidate.2)
            == super::super::super::status::canonical_path_filters(&identity.path_filters)
        && super::super::super::status::canonical_filter_values(&candidate.3)
            == super::super::super::status::canonical_filter_values(&identity.language_filters)
}

fn require_distinct(identity: &CloneIdentity, base_scope: String) -> Result<String, StorageError> {
    if base_scope == identity.source_scope {
        return Err(StorageError::Invariant(
            "durable incremental clone target cannot equal its base scope".to_owned(),
        ));
    }
    Ok(base_scope)
}

pub(super) fn manifest_header(
    transaction: &Transaction<'_>,
    base_scope: &str,
) -> Result<(usize, usize), StorageError> {
    let manifest = transaction
        .query_row(
            "SELECT manifest.reference_count, manifest.group_count
             FROM code_repository_reference_search_manifests manifest
             JOIN code_repository_index_checkpoints checkpoint
               ON checkpoint.source_scope = manifest.source_scope
             WHERE manifest.source_scope = ?1
               AND manifest.projection_version = 2
               AND manifest.reference_count = checkpoint.committed_reference_count
               AND manifest.group_count <= manifest.reference_count",
            [base_scope],
            |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?)),
        )
        .optional()?;
    manifest.ok_or_else(|| {
        StorageError::Invariant(format!(
            "incremental clone base scope '{base_scope}' lacks an exact grouped-reference manifest header"
        ))
    })
}

pub(super) fn step_proof(
    connection: &rusqlite::Connection,
    base_scope: &str,
) -> Result<StepProof, StorageError> {
    let checkpoint = connection
        .query_row(
            "SELECT state, committed_fact_row_count, resource_budget_json
             FROM code_repository_index_checkpoints
             WHERE source_scope = ?1",
            [base_scope],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, usize>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((state, committed_fact_row_count, encoded_budget)) = checkpoint else {
        return Err(StorageError::CapacityExceeded(format!(
            "incremental clone base scope '{base_scope}' has no durable fact-row proof"
        )));
    };
    if !matches!(
        state.as_str(),
        "completed" | "finalizing:partitioned_publish"
    ) {
        return Err(StorageError::InvalidInput(format!(
            "incremental clone base scope '{base_scope}' is not a completed proven session"
        )));
    }
    if committed_fact_row_count == 0 {
        return Err(StorageError::CapacityExceeded(format!(
            "incremental clone base scope '{base_scope}' predates the durable fact-row proof"
        )));
    }
    let budget = serde_json::from_str::<CodeIndexResourceBudget>(&encoded_budget).map_err(|error| {
        StorageError::Invariant(format!(
            "incremental clone base scope '{base_scope}' has an invalid resource budget: {error}"
        ))
    })?;
    let budget = CodeIndexResourceBudget::new(
        budget.max_files_per_batch,
        budget.max_bytes_per_batch,
        budget.max_rows_per_batch,
    )
    .map_err(|error| {
        StorageError::Invariant(format!(
            "incremental clone base scope '{base_scope}' has a noncanonical resource budget: {error}"
        ))
    })?;
    // Let F be the exact cumulative committed batch fact count and R its reference rows. Calls K
    // and reference groups G are each bounded by R. Non-reference search owners A are bounded by
    // F - R, while call and grouped-reference owners add K + G. Clone source rows are therefore
    // (F + K) + G + (A + K + G) <= 2F + K + 2G <= 5F. Fixed table transitions are added below.
    let source_fact_row_upper_bound = committed_fact_row_count;
    let max_steps = source_fact_row_upper_bound
        .checked_mul(5)
        .and_then(|rows| rows.checked_add(table_count() + 4))
        .ok_or_else(|| clone_capacity_error(base_scope))?;
    Ok(StepProof {
        max_steps,
        source_fact_row_upper_bound,
        max_rows_per_batch: budget.max_rows_per_batch,
        max_bytes_per_batch: budget.max_bytes_per_batch,
    })
}
