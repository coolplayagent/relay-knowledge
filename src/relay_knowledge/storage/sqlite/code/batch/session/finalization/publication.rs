//! Scope publication handler kept separate from phase dispatch and repair.

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    domain::CodeIndexSession,
    storage::{StorageError, sqlite::code::cleanup::count_code_rows},
};

use super::super::{checkpoint, finalize};
use super::{TransactionAdvance, finalization_phase_pending};
use crate::storage::sqlite::code::workspace;

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
    let degraded_file_count = count_code_rows(
        transaction,
        "code_repository_file_diagnostics",
        &session.source_scope,
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
