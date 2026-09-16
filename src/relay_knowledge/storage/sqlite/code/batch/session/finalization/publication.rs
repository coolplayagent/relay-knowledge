//! Scope publication handler kept separate from phase dispatch and repair.

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    domain::CodeIndexSession,
    storage::{StorageError, sqlite::code::cleanup::count_code_rows},
};

use super::super::{checkpoint, finalize};
use super::{TransactionAdvance, finalization_phase_pending};
use crate::storage::sqlite::code::{
    lifecycle::publication_fence::PublicationFenceGuard, workspace,
};

pub(super) fn complete_unfenced_publication(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
) -> Result<TransactionAdvance, StorageError> {
    if finalization_phase_pending(checkpoint_state, finalize::phases::PUBLISH_SCOPE)? {
        publish_repository_scope(transaction, session, false)?;
    }
    if finalization_phase_pending(
        checkpoint_state,
        finalize::phases::RESOLVE_WORKSPACE_IMPORTS,
    )? {
        workspace::resolve_workspace_imports(
            transaction,
            &session.workspaces,
            &session.repository_id,
            &session.source_scope,
        )?;
    }
    checkpoint::compare_and_mark_completed(transaction, &session.source_scope, checkpoint_state)?;
    Ok(TransactionAdvance::Ready)
}

pub(super) fn publish_repository_scope(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    defer_until_software_projection: bool,
) -> Result<(), StorageError> {
    for tombstone in &session.tombstones {
        transaction.execute(
            "INSERT OR REPLACE INTO code_repository_path_tombstones
                (repository_id, source_scope, old_path, new_path, base_ref, head_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                tombstone.repository_id,
                tombstone.source_scope,
                tombstone.old_path,
                tombstone.new_path,
                tombstone.base_ref,
                tombstone.head_ref,
            ],
        )?;
    }
    let file_count = count_code_rows(transaction, "code_repository_files", &session.source_scope)?;
    let symbol_count = count_code_rows(
        transaction,
        "code_repository_symbols",
        &session.source_scope,
    )?;
    let reference_count = count_code_rows(
        transaction,
        "code_repository_references",
        &session.source_scope,
    )?;
    if session.full_replace {
        require_grouped_reference_search_manifest(
            transaction,
            &session.source_scope,
            reference_count,
        )?;
    }
    let chunk_count =
        count_code_rows(transaction, "code_repository_chunks", &session.source_scope)?;
    let degraded_file_count: usize = transaction.query_row(
        "SELECT COUNT(DISTINCT path) FROM code_repository_file_diagnostics WHERE source_scope = ?1",
        params![session.source_scope],
        |row| row.get(0),
    )?;
    let degraded_reason = (degraded_file_count > 0)
        .then(|| format!("{degraded_file_count} file(s) degraded during code indexing"));
    let path_filters_json = checkpoint::serialize_json(&session.path_filters)?;
    let language_filters_json = checkpoint::serialize_json(&session.language_filters)?;
    crate::storage::sqlite::code::publication::stage(
        transaction,
        &crate::storage::sqlite::code::publication::ScopePublication {
            repository_id: &session.repository_id,
            source_scope: &session.source_scope,
            resolved_commit_sha: &session.resolved_commit_sha,
            tree_hash: &session.tree_hash,
            path_filters_json: &path_filters_json,
            language_filters_json: &language_filters_json,
            indexed_file_count: file_count,
            symbol_count,
            reference_count,
            chunk_count,
            degraded_reason: degraded_reason.as_deref(),
        },
        defer_until_software_projection,
    )?;

    Ok(())
}

fn require_grouped_reference_search_manifest(
    transaction: &Transaction<'_>,
    source_scope: &str,
    expected_reference_count: usize,
) -> Result<(), StorageError> {
    let manifest = transaction
        .query_row(
            "SELECT projection_version, reference_count, group_count
             FROM code_repository_reference_search_manifests WHERE source_scope = ?1",
            params![source_scope],
            |row| {
                Ok((
                    row.get::<_, usize>(0)?,
                    row.get::<_, usize>(1)?,
                    row.get::<_, usize>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((projection_version, reference_count, group_count)) = manifest else {
        return Err(StorageError::Invariant(format!(
            "full code scope '{source_scope}' has no durable grouped reference-search manifest"
        )));
    };
    if projection_version != 2
        || reference_count != expected_reference_count
        || group_count > reference_count
    {
        return Err(StorageError::Invariant(format!(
            "full code scope '{source_scope}' has an invalid grouped reference-search manifest"
        )));
    }
    Ok(())
}

pub(in crate::storage::sqlite::code::batch::session) fn finalization_target_is_unpublished(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    fence: &PublicationFenceGuard,
) -> Result<bool, StorageError> {
    if !session.full_replace {
        return Ok(false);
    }
    fence.validate_repository(&session.repository_id)?;
    fence.validate_target_scope(transaction, &session.source_scope)?;
    fence.validate(transaction)?;
    if locally_queryable_finalization_target(transaction, session)? {
        return Ok(false);
    }
    if !fence.authority_is_local() {
        fence.validate_partitioned_staged_scope(
            transaction,
            &session.repository_id,
            &session.source_scope,
        )?;
    }
    Ok(true)
}

pub(super) fn locally_queryable_finalization_target(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
) -> Result<bool, StorageError> {
    transaction
        .query_row(
            "SELECT EXISTS (
             SELECT 1 FROM code_repositories repository
             WHERE repository.repository_id = ?1
               AND repository.last_indexed_scope_id = ?2
         ) OR EXISTS (
             SELECT 1 FROM code_repository_scopes scope
             WHERE scope.repository_id = ?1 AND scope.source_scope = ?2
               AND (scope.stale = 0 OR scope.retiring <> 0)
         ) OR EXISTS (
             SELECT 1 FROM code_repository_commit_scopes commit_scope
             WHERE commit_scope.repository_id = ?1 AND commit_scope.source_scope = ?2
         ) OR EXISTS (
             SELECT 1 FROM code_repository_scope_gc_jobs job
             WHERE job.repository_id = ?1 AND job.source_scope = ?2
         )",
            params![session.repository_id, session.source_scope],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code::batch::session) fn require_unpublished_finalization_target(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    fence: &PublicationFenceGuard,
) -> Result<(), StorageError> {
    require_unpublished_finalization_owner(transaction, session, fence)?;
    crate::storage::sqlite::code::schema::require_code_query_indexes_for_fact_publication(
        transaction,
    )
}

pub(in crate::storage::sqlite::code::batch::session) fn require_unpublished_finalization_owner(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    fence: &PublicationFenceGuard,
) -> Result<(), StorageError> {
    if !finalization_target_is_unpublished(transaction, session, fence)? {
        return Err(StorageError::Invariant(format!(
            "durable finalization pages cannot mutate queryable scope '{}'",
            session.source_scope
        )));
    }
    Ok(())
}
