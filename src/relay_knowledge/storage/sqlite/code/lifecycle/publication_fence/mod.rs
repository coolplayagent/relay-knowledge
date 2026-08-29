//! Enforces attempt-scoped publication ownership at SQLite commit boundaries.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{
    clock::system_now_millis,
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget,
        code_snapshot_scope_is_fact_versioned, code_snapshot_scope_matches_identity,
        code_snapshot_scope_workspace_semantic,
    },
    storage::StorageError,
};

const AUTHORITY_SCHEMA: &str = "code_publication_authority";

#[derive(Debug, Clone)]
pub(in crate::storage) struct PublicationFenceGuard {
    fence: CodeIndexPublicationFence,
    authority_schema: &'static str,
}

pub(in crate::storage) fn prepare_guard(
    connection: &Connection,
    fence: CodeIndexPublicationFence,
    authority_path: Option<&Path>,
) -> Result<PublicationFenceGuard, StorageError> {
    if fence.repository_id.trim().is_empty()
        || fence.task_id.trim().is_empty()
        || fence.lease_owner.trim().is_empty()
        || fence.attempt_count == 0
        || fence.generation == 0
    {
        return Err(StorageError::InvalidInput(
            "code index publication fence is incomplete".to_owned(),
        ));
    }
    let authority_schema = if let Some(authority_path) = authority_path {
        attach_authority(connection, authority_path)?;
        AUTHORITY_SCHEMA
    } else {
        "main"
    };

    Ok(PublicationFenceGuard {
        fence,
        authority_schema,
    })
}

impl PublicationFenceGuard {
    pub(in crate::storage) fn task_id(&self) -> &str {
        &self.fence.task_id
    }

    /// Returns whether the fenced task tables live in the same SQLite WAL as
    /// the data transaction. Partitioned shards attach an external control
    /// authority and must leave receipt publication to the control database.
    pub(in crate::storage) fn authority_is_local(&self) -> bool {
        self.authority_schema == "main"
    }

    /// Selects the direct snapshot protocol for a fenced worktree task.
    ///
    /// Worktree snapshots must preserve their synthetic overlay identity and
    /// cannot fall back to the clean full-index pipeline. The eventual data
    /// transaction still performs the exact pending-to-content scope rebind
    /// and live lease validation before commit.
    pub(in crate::storage) fn is_worktree_overlay_task(
        &self,
        connection: &Connection,
    ) -> Result<bool, StorageError> {
        let sql = format!(
            "SELECT mode_json
             FROM {}.code_repository_index_tasks
             WHERE task_id = ?1 AND repository_id = ?2
               AND attempt_count = ?3 AND publication_generation = ?4",
            self.authority_schema
        );
        let encoded = connection
            .query_row(
                &sql,
                params![
                    self.fence.task_id,
                    self.fence.repository_id,
                    self.fence.attempt_count,
                    self.fence.generation,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| self.inactive_error())?;
        let mode = serde_json::from_str::<CodeIndexMode>(&encoded).map_err(|error| {
            StorageError::Invariant(format!(
                "code index publication fence for task '{}' has an invalid durable mode: {error}",
                self.fence.task_id
            ))
        })?;
        Ok(mode == CodeIndexMode::WorktreeOverlay)
    }

    pub(in crate::storage) fn validate_repository(
        &self,
        repository_id: &str,
    ) -> Result<(), StorageError> {
        if self.fence.repository_id != repository_id {
            return Err(StorageError::InvalidInput(format!(
                "code index publication fence for repository '{}' cannot publish repository '{}'",
                self.fence.repository_id, repository_id
            )));
        }
        Ok(())
    }

    pub(in crate::storage) fn validate_scope_repository(
        &self,
        transaction: &Transaction<'_>,
        source_scope: &str,
    ) -> Result<(), StorageError> {
        let repository_id = transaction
            .query_row(
                "SELECT repository_id FROM code_repository_scopes WHERE source_scope = ?1",
                params![source_scope],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                StorageError::InvalidInput(format!(
                    "code index publication scope '{source_scope}' is unavailable"
                ))
            })?;
        self.validate_repository(&repository_id)
    }

    pub(in crate::storage) fn validate_target_scope(
        &self,
        transaction: &Transaction<'_>,
        source_scope: &str,
    ) -> Result<(), StorageError> {
        if self.target_scope_matches(transaction, source_scope)? {
            return Ok(());
        }
        if self.rebind_pending_worktree_target(transaction, source_scope)? {
            return Ok(());
        }
        Err(StorageError::InvalidInput(format!(
            "code index publication fence for task '{}' cannot publish scope '{}'",
            self.fence.task_id, source_scope
        )))
    }

    fn target_scope_matches(
        &self,
        transaction: &Transaction<'_>,
        source_scope: &str,
    ) -> Result<bool, StorageError> {
        let sql = format!(
            "
            SELECT 1
            FROM {}.code_repository_index_tasks
            WHERE task_id = ?1
              AND repository_id = ?2
              AND source_scope = ?3
              AND publication_generation = ?4
            ",
            self.authority_schema
        );
        Ok(transaction
            .query_row(
                &sql,
                params![
                    self.fence.task_id,
                    self.fence.repository_id,
                    source_scope,
                    self.fence.generation,
                ],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Rebinds a pending worktree task to the content-addressed overlay scope.
    ///
    /// The update is part of the caller's data transaction. Its predicate
    /// repeats the live lease, attempt, and generation fence so an expired
    /// worker cannot change retention pins or publish facts after takeover.
    fn rebind_pending_worktree_target(
        &self,
        transaction: &Transaction<'_>,
        source_scope: &str,
    ) -> Result<bool, StorageError> {
        let Some(target) = load_verified_worktree_scope(transaction, source_scope)? else {
            return Ok(false);
        };
        self.rebind_verified_worktree_target(transaction, source_scope, &target)
    }

    fn rebind_verified_worktree_target(
        &self,
        transaction: &Transaction<'_>,
        source_scope: &str,
        target: &WorktreeScopeIdentity,
    ) -> Result<bool, StorageError> {
        let sql = format!(
            "
            SELECT source_scope, resolved_commit_sha, tree_hash,
                   path_filters_json, language_filters_json, mode_json
            FROM {}.code_repository_index_tasks
            WHERE task_id = ?1 AND repository_id = ?2
            ",
            self.authority_schema
        );
        let pending = transaction
            .query_row(
                &sql,
                params![self.fence.task_id, self.fence.repository_id],
                |row| {
                    Ok(PendingWorktreeTarget {
                        source_scope: row.get(0)?,
                        resolved_commit_sha: row.get(1)?,
                        tree_hash: row.get(2)?,
                        path_filters_json: row.get(3)?,
                        language_filters_json: row.get(4)?,
                        mode_json: row.get(5)?,
                    })
                },
            )
            .optional()?;
        let Some(pending) = pending else {
            return Ok(false);
        };
        if !pending.matches_real_scope(&self.fence.repository_id, target)? {
            return Ok(false);
        }
        super::super::tasks::enforce_rebound_target(
            transaction,
            self.authority_schema,
            &self.fence.repository_id,
            &self.fence.task_id,
            source_scope,
        )?;

        self.lock_authority(transaction)?;
        let execution_now_ms = now_millis()?;
        let sql = format!(
            "
            UPDATE {}.code_repository_index_tasks
            SET source_scope = ?6,
                resolved_commit_sha = ?7,
                tree_hash = ?8,
                updated_at_ms = ?9
            WHERE task_id = ?1
              AND repository_id = ?2
              AND source_scope = ?10
              AND state = 'running'
              AND lease_owner = ?3
              AND attempt_count = ?4
              AND publication_generation = ?5
              AND lease_expires_at_ms > ?9
              AND NOT EXISTS (
                  SELECT 1
                  FROM {}.code_repository_scopes scope
                  WHERE scope.source_scope = ?6
                    AND (scope.repository_id <> ?2 OR scope.retiring <> 0)
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM {}.code_repository_scope_gc_jobs job
                  WHERE job.source_scope = ?6
              )
              AND EXISTS (
                  SELECT 1
                  FROM {}.code_repository_publication_fences authority
                  WHERE authority.repository_id = ?2
                    AND authority.generation = ?5
                    AND authority.task_id = ?1
                    AND authority.attempt_count = ?4
                    AND authority.lease_owner = ?3
              )
            ",
            self.authority_schema,
            self.authority_schema,
            self.authority_schema,
            self.authority_schema,
        );
        let changed = transaction.execute(
            &sql,
            params![
                self.fence.task_id,
                self.fence.repository_id,
                self.fence.lease_owner,
                self.fence.attempt_count,
                self.fence.generation,
                source_scope,
                target.resolved_commit_sha,
                target.tree_hash,
                execution_now_ms,
                pending.source_scope,
            ],
        )?;
        Ok(changed == 1)
    }

    /// Validates the live attempt and locks the authoritative fence row until
    /// the surrounding data transaction commits or rolls back.
    pub(in crate::storage) fn validate(
        &self,
        transaction: &Transaction<'_>,
    ) -> Result<(), StorageError> {
        self.validate_with_clock(transaction, now_millis)
    }

    pub(in crate::storage) fn resource_budget(
        &self,
        connection: &Connection,
    ) -> Result<CodeIndexResourceBudget, StorageError> {
        let sql = format!(
            "SELECT resource_budget_json
             FROM {}.code_repository_index_tasks
             WHERE task_id = ?1 AND repository_id = ?2
               AND attempt_count = ?3 AND publication_generation = ?4",
            self.authority_schema
        );
        let encoded = connection
            .query_row(
                &sql,
                params![
                    self.fence.task_id,
                    self.fence.repository_id,
                    self.fence.attempt_count,
                    self.fence.generation,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| self.inactive_error())?;
        let budget = serde_json::from_str::<CodeIndexResourceBudget>(&encoded).map_err(|error| {
            StorageError::Invariant(format!(
                "code index publication fence for task '{}' has an invalid durable resource budget: {error}",
                self.fence.task_id
            ))
        })?;
        CodeIndexResourceBudget::new(
            budget.max_files_per_batch,
            budget.max_bytes_per_batch,
            budget.max_rows_per_batch,
        )
        .map_err(|error| {
            StorageError::Invariant(format!(
                "code index publication fence for task '{}' has an invalid durable resource budget: {error}",
                self.fence.task_id
            ))
        })
    }

    /// Requires an inactive partitioned route still owned by this exact task.
    ///
    /// Callers invoke this only after `validate` has locked the attached
    /// control authority, so catalog publication cannot race a shard-side
    /// checkpoint transition out of a historical completed state.
    pub(in crate::storage) fn validate_partitioned_staged_scope(
        &self,
        transaction: &Transaction<'_>,
        repository_id: &str,
        source_scope: &str,
    ) -> Result<(), StorageError> {
        self.validate_repository(repository_id)?;
        let sql = format!(
            "SELECT 1 FROM {}.storage_repository_shard_scopes
             WHERE repository_id = ?1 AND source_scope = ?2
               AND state = 'staged' AND staged_task_id = ?3",
            self.authority_schema
        );
        if transaction
            .query_row(
                &sql,
                params![repository_id, source_scope, self.fence.task_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some()
        {
            return Ok(());
        }
        Err(StorageError::Invariant(format!(
            "partitioned scope '{source_scope}' is not staged by fenced task '{}'",
            self.fence.task_id
        )))
    }

    fn validate_with_clock(
        &self,
        transaction: &Transaction<'_>,
        clock: impl FnOnce() -> Result<u64, StorageError>,
    ) -> Result<(), StorageError> {
        self.lock_authority(transaction)?;
        let execution_now_ms = clock()?;
        let sql = format!(
            "
            SELECT EXISTS (
                SELECT 1
                FROM {}.code_repository_index_tasks task
                WHERE task.task_id = ?3
                  AND task.repository_id = ?1
                  AND task.state = 'running'
                  AND task.lease_owner = ?5
                  AND task.attempt_count = ?4
                  AND task.publication_generation = ?2
                  AND task.lease_expires_at_ms > ?6
            )
            ",
            self.authority_schema
        );
        let active = transaction.query_row(
            &sql,
            params![
                self.fence.repository_id,
                self.fence.generation,
                self.fence.task_id,
                self.fence.attempt_count,
                self.fence.lease_owner,
                execution_now_ms,
            ],
            |row| row.get::<_, bool>(0),
        )?;
        if active {
            return Ok(());
        }

        Err(self.inactive_error())
    }

    /// Obtains the SQLite authority-writer lock before any execution-time
    /// sample can authorize a lease-sensitive mutation.
    fn lock_authority(&self, transaction: &Transaction<'_>) -> Result<(), StorageError> {
        let sql = format!(
            "UPDATE {}.code_repository_publication_fences
             SET updated_at_ms = updated_at_ms
             WHERE repository_id = ?1
               AND generation = ?2
               AND task_id = ?3
               AND attempt_count = ?4
               AND lease_owner = ?5",
            self.authority_schema
        );
        let locked = transaction.execute(
            &sql,
            params![
                self.fence.repository_id,
                self.fence.generation,
                self.fence.task_id,
                self.fence.attempt_count,
                self.fence.lease_owner,
            ],
        )?;
        if locked == 1 {
            return Ok(());
        }
        Err(self.inactive_error())
    }

    fn inactive_error(&self) -> StorageError {
        StorageError::InvalidInput(format!(
            "code index publication fence for task '{}' attempt {} is no longer active",
            self.fence.task_id, self.fence.attempt_count
        ))
    }
}

/// Snapshot identity copied into the control-plane handoff before a
/// partitioned shard enters its publication transaction.
///
/// Keeping only immutable identity fields makes the handoff bounded even when
/// a snapshot carries a large fact payload.
#[derive(Debug, Clone)]
pub(in crate::storage) struct PartitionedPublicationTarget {
    repository_id: String,
    source_scope: String,
    base_resolved_commit_sha: Option<String>,
    resolved_commit_sha: String,
    tree_hash: String,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
}

impl From<&crate::domain::CodeIndexSnapshot> for PartitionedPublicationTarget {
    fn from(snapshot: &crate::domain::CodeIndexSnapshot) -> Self {
        Self {
            repository_id: snapshot.repository_id.clone(),
            source_scope: snapshot.source_scope.clone(),
            base_resolved_commit_sha: snapshot.base_resolved_commit_sha.clone(),
            resolved_commit_sha: snapshot.resolved_commit_sha.clone(),
            tree_hash: snapshot.tree_hash.clone(),
            path_filters: snapshot.path_filters.clone(),
            language_filters: snapshot.language_filters.clone(),
        }
    }
}

/// Durably prepares a partitioned publication target in the control database.
///
/// SQLite cannot make an attached multi-database WAL transaction power-loss
/// atomic. Worktree rebinding therefore happens first in this control-only
/// `BEGIN IMMEDIATE` transaction. The task row is the durable, bounded handoff
/// record: a crash before or after the later shard commit leaves an idempotent
/// real target for startup lease recovery and normal task retry.
pub(in crate::storage) fn prepare_partitioned_target(
    connection: &mut Connection,
    target: &PartitionedPublicationTarget,
    fence: CodeIndexPublicationFence,
) -> Result<(), StorageError> {
    let guard = prepare_guard(connection, fence, None)?;
    guard.validate_repository(&target.repository_id)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !guard.target_scope_matches(&transaction, &target.source_scope)? {
        let identity = verified_worktree_scope_from_target(target)?.ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "partitioned publication target '{}' does not match task '{}'",
                target.source_scope, guard.fence.task_id
            ))
        })?;
        if !guard.rebind_verified_worktree_target(&transaction, &target.source_scope, &identity)? {
            return Err(StorageError::InvalidInput(format!(
                "code index publication fence for task '{}' cannot prepare scope '{}'",
                guard.fence.task_id, target.source_scope
            )));
        }
    }
    guard.validate(&transaction)?;
    transaction.commit()?;
    Ok(())
}

struct PendingWorktreeTarget {
    source_scope: String,
    resolved_commit_sha: String,
    tree_hash: String,
    path_filters_json: String,
    language_filters_json: String,
    mode_json: String,
}

impl PendingWorktreeTarget {
    fn matches_real_scope(
        &self,
        repository_id: &str,
        target: &WorktreeScopeIdentity,
    ) -> Result<bool, StorageError> {
        let mode = serde_json::from_str::<CodeIndexMode>(&self.mode_json)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
        let Some(base_commit) = self.resolved_commit_sha.strip_prefix("worktree:pending:") else {
            return Ok(false);
        };
        let path_filters = parse_filters(&self.path_filters_json)?;
        let language_filters = parse_filters(&self.language_filters_json)?;
        let pending_scope_matches = code_snapshot_scope_matches_identity(
            repository_id,
            &self.tree_hash,
            &path_filters,
            &language_filters,
            &self.source_scope,
        );
        let pending_semantic = code_snapshot_scope_workspace_semantic(
            repository_id,
            &self.tree_hash,
            &path_filters,
            &language_filters,
            &self.source_scope,
        );

        let target_matches_base = target.base_commit.as_deref().map_or_else(
            || filesystem_snapshot_identity(base_commit),
            |target_base| target_base == base_commit,
        );

        Ok(mode == CodeIndexMode::WorktreeOverlay
            && !base_commit.is_empty()
            && self.tree_hash == self.resolved_commit_sha
            && pending_scope_matches
            && pending_semantic == Some(target.workspace_semantic)
            && target.repository_id == repository_id
            && target_matches_base
            && target.path_filters == path_filters
            && target.language_filters == language_filters)
    }
}

struct WorktreeScopeIdentity {
    repository_id: String,
    base_commit: Option<String>,
    resolved_commit_sha: String,
    tree_hash: String,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    workspace_semantic: Option<u8>,
}

fn verified_worktree_scope_from_target(
    target: &PartitionedPublicationTarget,
) -> Result<Option<WorktreeScopeIdentity>, StorageError> {
    if !code_snapshot_scope_is_fact_versioned(&target.source_scope) {
        return Ok(None);
    }
    let scope_matches = code_snapshot_scope_matches_identity(
        &target.repository_id,
        &target.tree_hash,
        &target.path_filters,
        &target.language_filters,
        &target.source_scope,
    );
    if !scope_matches {
        return Ok(None);
    }
    let Some(workspace_semantic) = code_snapshot_scope_workspace_semantic(
        &target.repository_id,
        &target.tree_hash,
        &target.path_filters,
        &target.language_filters,
        &target.source_scope,
    ) else {
        return Ok(None);
    };

    let base_commit = if let Some(identity) = target
        .resolved_commit_sha
        .strip_prefix("worktree:")
        .filter(|identity| !identity.starts_with("pending:"))
    {
        let Some((base_commit, overlay_hash)) = identity.split_once(':') else {
            return Ok(None);
        };
        if base_commit.is_empty()
            || overlay_hash.len() != 16
            || !overlay_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || target.tree_hash != format!("worktree:{overlay_hash}")
            || target.base_resolved_commit_sha.as_deref() != Some(base_commit)
        {
            return Ok(None);
        }
        Some(base_commit.to_owned())
    } else {
        let Some(base_commit) = target.base_resolved_commit_sha.as_deref() else {
            return Ok(None);
        };
        if target.resolved_commit_sha != target.tree_hash
            || !filesystem_snapshot_identity(&target.resolved_commit_sha)
            || !filesystem_snapshot_identity(base_commit)
        {
            return Ok(None);
        }
        Some(base_commit.to_owned())
    };

    Ok(Some(WorktreeScopeIdentity {
        repository_id: target.repository_id.clone(),
        base_commit,
        resolved_commit_sha: target.resolved_commit_sha.clone(),
        tree_hash: target.tree_hash.clone(),
        path_filters: target.path_filters.clone(),
        language_filters: target.language_filters.clone(),
        workspace_semantic,
    }))
}

fn load_verified_worktree_scope(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Option<WorktreeScopeIdentity>, StorageError> {
    if !code_snapshot_scope_is_fact_versioned(source_scope) {
        return Ok(None);
    }
    let scope = transaction
        .query_row(
            "
            SELECT repository_id, resolved_commit_sha, tree_hash,
                   path_filters_json, language_filters_json
            FROM code_repository_scopes
            WHERE source_scope = ?1 AND retiring = 0
            ",
            params![source_scope],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((repository_id, resolved_commit_sha, tree_hash, path_json, language_json)) = scope
    else {
        return Ok(None);
    };
    let (base_commit, valid_tree_identity) = resolved_commit_sha
        .strip_prefix("worktree:")
        .and_then(|identity| identity.split_once(':'))
        .map_or_else(
            || {
                (
                    None,
                    resolved_commit_sha == tree_hash
                        && filesystem_snapshot_identity(&resolved_commit_sha),
                )
            },
            |(base_commit, overlay_hash)| {
                (
                    Some(base_commit.to_owned()),
                    !base_commit.is_empty()
                        && !overlay_hash.is_empty()
                        && tree_hash == format!("worktree:{overlay_hash}"),
                )
            },
        );
    let path_filters = parse_filters(&path_json)?;
    let language_filters = parse_filters(&language_json)?;
    let scope_matches = code_snapshot_scope_matches_identity(
        &repository_id,
        &tree_hash,
        &path_filters,
        &language_filters,
        source_scope,
    );
    if !valid_tree_identity || !scope_matches {
        return Ok(None);
    }
    let Some(workspace_semantic) = code_snapshot_scope_workspace_semantic(
        &repository_id,
        &tree_hash,
        &path_filters,
        &language_filters,
        source_scope,
    ) else {
        return Ok(None);
    };

    Ok(Some(WorktreeScopeIdentity {
        repository_id,
        base_commit,
        resolved_commit_sha,
        tree_hash,
        path_filters,
        language_filters,
        workspace_semantic,
    }))
}

fn filesystem_snapshot_identity(identity: &str) -> bool {
    identity
        .strip_prefix("filesystem:")
        .is_some_and(|hash| hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn parse_filters(value: &str) -> Result<Vec<String>, StorageError> {
    serde_json::from_str(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn attach_authority(connection: &Connection, authority_path: &Path) -> Result<(), StorageError> {
    let attached = connection
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name = ?1",
            params![AUTHORITY_SCHEMA],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(attached) = attached {
        if Path::new(&attached) == authority_path {
            return Ok(());
        }
        return Err(StorageError::InvalidInput(format!(
            "SQLite publication authority is already attached from '{}'",
            attached
        )));
    }
    connection.execute(
        &format!("ATTACH DATABASE ?1 AS {AUTHORITY_SCHEMA}"),
        params![authority_path.to_string_lossy().as_ref()],
    )?;
    Ok(())
}

fn now_millis() -> Result<u64, StorageError> {
    system_now_millis().map_err(|error| StorageError::Invariant(error.to_string()))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "worktree_rebind_tests.rs"]
mod worktree_rebind_tests;
