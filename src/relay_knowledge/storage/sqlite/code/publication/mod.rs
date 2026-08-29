//! Stages code scopes and publishes them only after derived software facts exist.
//!
//! This code-owned boundary is called by software projection in the existing
//! software-to-code dependency direction.

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    clock::system_now_millis_or_zero as now_millis,
    storage::{CodeIndexPublicationTarget, StorageError},
};

use super::lifecycle::commit_scope;

pub(in crate::storage::sqlite) struct ScopePublication<'a> {
    pub(in crate::storage::sqlite) repository_id: &'a str,
    pub(in crate::storage::sqlite) source_scope: &'a str,
    pub(in crate::storage::sqlite) resolved_commit_sha: &'a str,
    pub(in crate::storage::sqlite) tree_hash: &'a str,
    pub(in crate::storage::sqlite) path_filters_json: &'a str,
    pub(in crate::storage::sqlite) language_filters_json: &'a str,
    pub(in crate::storage::sqlite) indexed_file_count: usize,
    pub(in crate::storage::sqlite) symbol_count: usize,
    pub(in crate::storage::sqlite) reference_count: usize,
    pub(in crate::storage::sqlite) chunk_count: usize,
    pub(in crate::storage::sqlite) degraded_reason: Option<&'a str>,
}

/// Persists a complete scope row. Fenced writers leave it stale and keep the
/// repository's previous active scope until software projection succeeds.
pub(in crate::storage::sqlite) fn stage(
    connection: &Connection,
    publication: &ScopePublication<'_>,
    defer_until_software_projection: bool,
) -> Result<(), StorageError> {
    commit_scope::preserve_existing_scope_commit(
        connection,
        publication.repository_id,
        publication.source_scope,
    )?;
    connection.execute(
        "
        INSERT INTO code_repository_scopes (
            source_scope, repository_id, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, indexed_file_count,
            symbol_count, reference_count, chunk_count, stale, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT(source_scope) DO UPDATE SET
            repository_id = excluded.repository_id,
            resolved_commit_sha = excluded.resolved_commit_sha,
            tree_hash = excluded.tree_hash,
            path_filters_json = excluded.path_filters_json,
            language_filters_json = excluded.language_filters_json,
            indexed_file_count = excluded.indexed_file_count,
            symbol_count = excluded.symbol_count,
            reference_count = excluded.reference_count,
            chunk_count = excluded.chunk_count,
            stale = excluded.stale,
            degraded_reason = excluded.degraded_reason
        ",
        params![
            publication.source_scope,
            publication.repository_id,
            publication.resolved_commit_sha,
            publication.tree_hash,
            publication.path_filters_json,
            publication.language_filters_json,
            publication.indexed_file_count,
            publication.symbol_count,
            publication.reference_count,
            publication.chunk_count,
            i64::from(defer_until_software_projection),
            publication.degraded_reason,
        ],
    )?;
    if defer_until_software_projection {
        connection.execute(
            "
            UPDATE code_repositories
            SET state = 'indexing', stale = 1
            WHERE repository_id = ?1
            ",
            params![publication.repository_id],
        )?;
        return Ok(());
    }

    publish_staged_scope(connection, publication.source_scope)
}

/// Reads the exact durable software-projection resume token.
pub(in crate::storage::sqlite) fn software_projection_checkpoint_state(
    connection: &Connection,
    source_scope: &str,
) -> Result<Option<String>, StorageError> {
    Ok(connection
        .query_row(
            "SELECT state FROM code_repository_index_checkpoints WHERE source_scope = ?1",
            params![source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?)
}

/// Atomically moves one software projection phase after its materialized rows commit.
pub(in crate::storage::sqlite) fn advance_software_projection_checkpoint(
    connection: &Connection,
    source_scope: &str,
    expected_state: &str,
    next_state: &str,
) -> Result<(), StorageError> {
    let updated_at_ms = crate::clock::system_now_millis()
        .map_err(|error| StorageError::Invariant(error.to_string()))?;
    let changed = connection.execute(
        "
        UPDATE code_repository_index_checkpoints
        SET state = ?3, updated_at_ms = ?4, error_message = NULL
        WHERE source_scope = ?1 AND state = ?2
        ",
        params![source_scope, expected_state, next_state, updated_at_ms],
    )?;
    if changed == 1 {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "software projection checkpoint for scope '{source_scope}' changed while advancing from '{expected_state}' to '{next_state}'"
    )))
}

/// Makes a staged scope, its software projection, and its checkpoint visible
/// as one publication decision. The caller revalidates the task fence in the
/// same transaction before commit.
pub(in crate::storage::sqlite) fn complete_after_software_projection(
    connection: &Connection,
    source_scope: &str,
    fence: &super::lifecycle::publication_fence::PublicationFenceGuard,
) -> Result<(), StorageError> {
    let checkpoint_state = connection
        .query_row(
            "SELECT state FROM code_repository_index_checkpoints WHERE source_scope = ?1",
            params![source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if checkpoint_state.as_deref().is_some_and(|state| {
        state != "completed" && crate::domain::code_software_projection_phase(state).is_none()
    }) {
        return Err(StorageError::InvalidInput(format!(
            "code scope '{source_scope}' cannot publish from checkpoint state '{}'",
            checkpoint_state.as_deref().unwrap_or_default()
        )));
    }
    let projection_staged = connection
        .query_row(
            "
            SELECT stale = 1
            FROM software_global_status
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false);
    if !projection_staged {
        return Err(StorageError::InvalidInput(format!(
            "code scope '{source_scope}' cannot publish before its fenced software projection is complete"
        )));
    }
    super::super::business::refresh_mapping_resolutions(connection, source_scope)?;
    super::super::business::mark_published(connection, source_scope)?;
    connection.execute(
        "UPDATE software_global_status SET stale = 0 WHERE source_scope = ?1",
        params![source_scope],
    )?;
    publish_staged_scope(connection, source_scope)?;
    let checkpoint_state = if fence.authority_is_local() {
        "completed"
    } else {
        super::batch::finalize::phases::PARTITIONED_PUBLISH
    };
    connection.execute(
        "
        UPDATE code_repository_index_checkpoints
        SET state = ?2, updated_at_ms = ?3, error_message = NULL
        WHERE source_scope = ?1
        ",
        params![source_scope, checkpoint_state, now_millis()],
    )?;
    if fence.authority_is_local() {
        record_receipt_from_active_fence(connection, source_scope)?;
    }

    Ok(())
}

pub(in crate::storage) fn record_receipt_from_active_fence(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    let changed = connection.execute(
        "
        INSERT OR REPLACE INTO code_repository_publication_receipts (
            task_id, repository_id, source_scope, publication_generation, published_at_ms
        )
        SELECT task.task_id, task.repository_id, task.source_scope,
               task.publication_generation, ?2
        FROM code_repository_index_tasks task
        JOIN code_repository_publication_fences fence
          ON fence.repository_id = task.repository_id
         AND fence.task_id = task.task_id
         AND fence.generation = task.publication_generation
         AND fence.attempt_count = task.attempt_count
         AND fence.lease_owner = task.lease_owner
        WHERE task.source_scope = ?1
          AND task.state = 'running'
        ",
        params![source_scope, now_millis()],
    )?;
    if changed != 1 {
        return Err(StorageError::InvalidInput(format!(
            "code scope '{source_scope}' cannot record a publication receipt without one active fenced task"
        )));
    }
    Ok(())
}

/// Adopts a published content scope for the commit named by the current task.
///
/// A source scope is content-addressed by tree, filters, fact version, and the
/// workspace-detection semantic. Two commits with the same content identity
/// therefore share code and software facts. Adoption changes only the bounded
/// commit aliases and publication metadata; it never rewrites those facts. A
/// retained, inactive scope may become active again without first rebuilding
/// its still-queryable facts.
/// The current live fence is validated before inspection and again after the
/// metadata writes so a detached attempt cannot move the active commit pointer.
pub(in crate::storage::sqlite) fn adopt_active_target(
    connection: &mut Connection,
    target: &CodeIndexPublicationTarget,
    guard: &super::lifecycle::publication_fence::PublicationFenceGuard,
) -> Result<bool, StorageError> {
    guard.validate_repository(&target.repository_id)?;
    if target.task_id != guard.task_id() {
        return Err(StorageError::InvalidInput(format!(
            "code index publication target task '{}' does not match active fence task '{}'",
            target.task_id,
            guard.task_id()
        )));
    }
    if target.resolved_commit_sha.trim().is_empty() {
        return Err(StorageError::InvalidInput(
            "code index publication target commit must not be empty".to_owned(),
        ));
    }
    let path_filters_json = serde_json::to_string(&target.path_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let language_filters_json = serde_json::to_string(&target.language_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let partitioned_authority = !guard.authority_is_local();
    let transaction = connection.transaction()?;
    guard.validate(&transaction)?;
    let published_commits = transaction
        .query_row(
            "
            SELECT scope.resolved_commit_sha, repository.last_indexed_commit,
                   checkpoint.resolved_commit_sha, checkpoint.resource_budget_json,
                   checkpoint.incremental_summary_json,
                   repository.last_indexed_scope_id,
                   scope.indexed_file_count, scope.symbol_count,
                   scope.reference_count, scope.chunk_count, scope.degraded_reason
            FROM code_repositories repository
            JOIN code_repository_scopes scope
              ON scope.source_scope = ?2
             AND scope.repository_id = repository.repository_id
            JOIN software_global_status software
              ON software.source_scope = scope.source_scope
             AND software.repository_id = scope.repository_id
            JOIN business_knowledge_status business
              ON business.source_scope = scope.source_scope
             AND business.repository_id = scope.repository_id
             AND business.resolved_commit_sha = scope.resolved_commit_sha
            LEFT JOIN code_repository_index_checkpoints checkpoint
              ON checkpoint.source_scope = scope.source_scope
            WHERE repository.repository_id = ?1
              AND repository.state = 'fresh' AND repository.stale = 0
              AND scope.tree_hash = ?3
              AND scope.path_filters_json = ?4 AND scope.language_filters_json = ?5
              AND trim(scope.resolved_commit_sha) <> ''
              AND scope.stale = 0 AND scope.retiring = 0
              AND software.stale = 0
              AND business.stale = 0
              AND EXISTS (
                  SELECT 1
                  FROM code_repository_scopes active_scope
                  JOIN software_global_status active_software
                    ON active_software.source_scope = active_scope.source_scope
                   AND active_software.repository_id = active_scope.repository_id
                  JOIN business_knowledge_status active_business
                    ON active_business.source_scope = active_scope.source_scope
                   AND active_business.repository_id = active_scope.repository_id
                   AND active_business.resolved_commit_sha = active_scope.resolved_commit_sha
                  WHERE active_scope.repository_id = repository.repository_id
                    AND active_scope.source_scope = repository.last_indexed_scope_id
                    AND active_scope.stale = 0 AND active_scope.retiring = 0
                    AND active_software.stale = 0
                    AND active_business.stale = 0
              )
              AND (
                  repository.last_indexed_commit IS NULL
                  OR EXISTS (
                      SELECT 1 FROM code_repository_commit_scopes active_alias
                      WHERE active_alias.repository_id = repository.repository_id
                        AND active_alias.resolved_commit_sha = repository.last_indexed_commit
                        AND active_alias.source_scope = repository.last_indexed_scope_id
                  )
                  OR repository.last_indexed_commit = (
                      SELECT active_scope.resolved_commit_sha
                      FROM code_repository_scopes active_scope
                      WHERE active_scope.repository_id = repository.repository_id
                        AND active_scope.source_scope = repository.last_indexed_scope_id
                  )
              )
              AND (
                  checkpoint.source_scope IS NULL
                  OR (
                      checkpoint.repository_id = scope.repository_id
                      AND (
                          checkpoint.state = 'completed'
                          OR (?6 = 1 AND checkpoint.state = ?7)
                      )
                      AND checkpoint.tree_hash = scope.tree_hash
                      AND checkpoint.path_filters_json = scope.path_filters_json
                      AND checkpoint.language_filters_json = scope.language_filters_json
                      AND trim(checkpoint.resolved_commit_sha) <> ''
                  )
              )
            ",
            params![
                target.repository_id,
                target.source_scope,
                target.tree_hash,
                path_filters_json,
                language_filters_json,
                i64::from(partitioned_authority),
                super::batch::finalize::phases::PARTITIONED_PUBLISH,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, usize>(6)?,
                    row.get::<_, usize>(7)?,
                    row.get::<_, usize>(8)?,
                    row.get::<_, usize>(9)?,
                    row.get::<_, Option<String>>(10)?,
                ))
            },
        )
        .optional()?;
    let Some((
        previous_scope_commit,
        previous_repository_commit,
        previous_checkpoint_commit,
        checkpoint_budget_json,
        incremental_summary_json,
        previous_active_scope,
        indexed_file_count,
        symbol_count,
        reference_count,
        chunk_count,
        degraded_reason,
    )) = published_commits
    else {
        guard.validate(&transaction)?;
        transaction.commit()?;
        return Ok(false);
    };

    guard.validate_target_scope(&transaction, &target.source_scope)?;
    commit_scope::preserve_existing_scope_commit(
        &transaction,
        &target.repository_id,
        &previous_active_scope,
    )?;
    commit_scope::record(
        &transaction,
        &target.repository_id,
        &previous_scope_commit,
        &target.source_scope,
    )?;
    if let Some(previous_repository_commit) = previous_repository_commit.as_deref() {
        commit_scope::record(
            &transaction,
            &target.repository_id,
            previous_repository_commit,
            &previous_active_scope,
        )?;
    }
    if let Some(previous_checkpoint_commit) = previous_checkpoint_commit.as_deref()
        && previous_checkpoint_commit != previous_scope_commit.as_str()
        && previous_repository_commit.as_deref() != Some(previous_checkpoint_commit)
    {
        commit_scope::record(
            &transaction,
            &target.repository_id,
            previous_checkpoint_commit,
            &target.source_scope,
        )?;
    }
    commit_scope::record(
        &transaction,
        &target.repository_id,
        &target.resolved_commit_sha,
        &target.source_scope,
    )?;
    let scope_changed = transaction.execute(
        "
        UPDATE code_repository_scopes
        SET resolved_commit_sha = ?3
        WHERE repository_id = ?1 AND source_scope = ?2
          AND tree_hash = ?4
          AND path_filters_json = ?5 AND language_filters_json = ?6
          AND stale = 0 AND retiring = 0
        ",
        params![
            target.repository_id,
            target.source_scope,
            target.resolved_commit_sha,
            target.tree_hash,
            path_filters_json,
            language_filters_json,
        ],
    )?;
    let business_changed = transaction.execute(
        "UPDATE business_knowledge_status
         SET resolved_commit_sha = ?3
         WHERE repository_id = ?1 AND source_scope = ?2
           AND resolved_commit_sha = ?4 AND stale = 0",
        params![
            target.repository_id,
            target.source_scope,
            target.resolved_commit_sha,
            previous_scope_commit,
        ],
    )?;
    let repository_changed = transaction.execute(
        "
        UPDATE code_repositories
        SET last_indexed_scope_id = ?2,
            last_indexed_commit = ?3,
            tree_hash = ?4,
            state = 'fresh',
            indexed_file_count = ?5,
            symbol_count = ?6,
            reference_count = ?7,
            chunk_count = ?8,
            stale = 0,
            degraded_reason = ?9
        WHERE repository_id = ?1 AND last_indexed_scope_id = ?10
          AND state = 'fresh' AND stale = 0
        ",
        params![
            target.repository_id,
            target.source_scope,
            target.resolved_commit_sha,
            target.tree_hash,
            indexed_file_count,
            symbol_count,
            reference_count,
            chunk_count,
            degraded_reason,
            previous_active_scope,
        ],
    )?;
    if scope_changed != 1 || business_changed != 1 || repository_changed != 1 {
        return Err(StorageError::InvalidInput(format!(
            "active code scope '{}' changed while its commit alias was being adopted",
            target.source_scope
        )));
    }
    let retain_incremental_summary = match (checkpoint_budget_json, incremental_summary_json) {
        (Some(budget_json), Some(receipt_json)) => {
            let budget = serde_json::from_str(&budget_json).map_err(|error| {
                StorageError::Invariant(format!(
                    "active code scope '{}' has an invalid checkpoint budget during adoption: {error}",
                    target.source_scope
                ))
            })?;
            super::checkpoint_receipt::decode(Some(receipt_json), 0, budget)
                .map_err(|error| {
                    StorageError::Invariant(format!(
                        "active code scope '{}' has an invalid incremental receipt during adoption: {error}",
                        target.source_scope
                    ))
                })?
                .is_some_and(|receipt| receipt.task_id == guard.task_id())
        }
        (None, None) | (Some(_), None) => false,
        (None, Some(_)) => {
            return Err(StorageError::Invariant(format!(
                "active code scope '{}' has an incremental receipt without a checkpoint budget",
                target.source_scope
            )));
        }
    };
    // A partitioned shard deliberately retains its cross-database handoff
    // state after catalog activation. Adoption updates only the commit alias;
    // the partitioned store projects that raw state as completed for readers.
    let checkpoint_changed = transaction.execute(
        "
        UPDATE code_repository_index_checkpoints
        SET resolved_commit_sha = ?2, updated_at_ms = ?3,
            incremental_summary_json = CASE
                WHEN ?10 THEN incremental_summary_json ELSE NULL
            END
        WHERE source_scope = ?1 AND repository_id = ?4
          AND (
              state = 'completed'
              OR (?8 = 1 AND state = ?9)
          )
          AND tree_hash = ?5
          AND path_filters_json = ?6 AND language_filters_json = ?7
        ",
        params![
            target.source_scope,
            target.resolved_commit_sha,
            now_millis(),
            target.repository_id,
            target.tree_hash,
            path_filters_json,
            language_filters_json,
            i64::from(partitioned_authority),
            super::batch::finalize::phases::PARTITIONED_PUBLISH,
            retain_incremental_summary,
        ],
    )?;
    if checkpoint_changed != usize::from(previous_checkpoint_commit.is_some()) {
        return Err(StorageError::InvalidInput(format!(
            "active code scope '{}' checkpoint changed while its commit alias was being adopted",
            target.source_scope
        )));
    }
    if guard.authority_is_local() {
        record_receipt_from_active_fence(&transaction, &target.source_scope)?;
    }
    guard.validate_target_scope(&transaction, &target.source_scope)?;
    guard.validate(&transaction)?;
    transaction.commit()?;
    Ok(true)
}

fn publish_staged_scope(connection: &Connection, source_scope: &str) -> Result<(), StorageError> {
    require_current_grouped_reference_manifest(connection, source_scope)?;
    let scope = connection
        .query_row(
            "
            SELECT repository_id, resolved_commit_sha, tree_hash, indexed_file_count,
                   symbol_count, reference_count, chunk_count, degraded_reason
            FROM code_repository_scopes
            WHERE source_scope = ?1
              AND retiring = 0
            ",
            params![source_scope],
            |row| {
                Ok(PersistedScope {
                    repository_id: row.get(0)?,
                    resolved_commit_sha: row.get(1)?,
                    tree_hash: row.get(2)?,
                    indexed_file_count: row.get(3)?,
                    symbol_count: row.get(4)?,
                    reference_count: row.get(5)?,
                    chunk_count: row.get(6)?,
                    degraded_reason: row.get(7)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "staged code scope '{source_scope}' is missing during publication"
            ))
        })?;
    commit_scope::record(
        connection,
        &scope.repository_id,
        &scope.resolved_commit_sha,
        source_scope,
    )?;
    connection.execute(
        "UPDATE code_repository_scopes SET stale = 0 WHERE source_scope = ?1",
        params![source_scope],
    )?;
    connection.execute(
        "
        UPDATE code_repositories
        SET last_indexed_scope_id = ?2,
            last_indexed_commit = ?3,
            tree_hash = ?4,
            state = 'fresh',
            indexed_file_count = ?5,
            symbol_count = ?6,
            reference_count = ?7,
            chunk_count = ?8,
            stale = 0,
            degraded_reason = ?9
        WHERE repository_id = ?1
        ",
        params![
            scope.repository_id,
            source_scope,
            scope.resolved_commit_sha,
            scope.tree_hash,
            scope.indexed_file_count,
            scope.symbol_count,
            scope.reference_count,
            scope.chunk_count,
            scope.degraded_reason,
        ],
    )?;

    Ok(())
}

fn require_current_grouped_reference_manifest(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    let current = connection.query_row(
        "SELECT EXISTS (
             SELECT 1
             FROM code_repository_scopes scope
             JOIN code_repository_reference_search_manifests manifest
               ON manifest.source_scope = scope.source_scope
             WHERE scope.source_scope = ?1
               AND manifest.projection_version = 2
               AND manifest.reference_count = scope.reference_count
               AND manifest.group_count <= manifest.reference_count
               AND NOT EXISTS (
                   SELECT 1 FROM code_repository_reference_search_progress progress
                   WHERE progress.source_scope = scope.source_scope
               )
         )",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    if current {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "code scope '{source_scope}' cannot publish without its complete grouped reference-search v2 manifest"
    )))
}

pub(in crate::storage::sqlite) fn reject_fenced_active_scope_rebuild(
    connection: &Connection,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    let active = connection
        .query_row(
            "
            SELECT 1
            FROM code_repositories repository
            JOIN code_repository_scopes scope
              ON scope.source_scope = repository.last_indexed_scope_id
             AND scope.repository_id = repository.repository_id
            WHERE repository.repository_id = ?1
              AND repository.last_indexed_scope_id = ?2
              AND scope.stale = 0 AND scope.retiring = 0
            ",
            params![repository_id, source_scope],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if active {
        return Err(StorageError::InvalidInput(format!(
            "fenced code index cannot rewrite already-active scope '{source_scope}'; reconcile the durable task publication first"
        )));
    }
    Ok(())
}

struct PersistedScope {
    repository_id: String,
    resolved_commit_sha: String,
    tree_hash: String,
    indexed_file_count: usize,
    symbol_count: usize,
    reference_count: usize,
    chunk_count: usize,
    degraded_reason: Option<String>,
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
