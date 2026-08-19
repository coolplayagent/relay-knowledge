//! Coordinates checkpointed session startup, finalization, and atomic scope publication.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::super::{
    cleanup::{count_code_rows, delete_scope_index},
    lifecycle::commit_scope,
    report,
    snapshot::clone_active_scope_for_incremental,
    status, workspace,
};
use super::{checkpoint, finalize};
use crate::{
    domain::{CodeIndexCheckpoint, CodeIndexProgressSummary, CodeIndexSession, CodeIndexSummary},
    storage::StorageError,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "checkpoint_batch_tests.rs"]
mod checkpoint_batch_tests;

pub(in super::super) fn begin_session(
    connection: &mut Connection,
    session: CodeIndexSession,
) -> Result<CodeIndexCheckpoint, StorageError> {
    begin_session_with_fence(connection, session, None)
}

pub(in super::super) fn begin_session_with_fence(
    connection: &mut Connection,
    session: CodeIndexSession,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    if let Some(fence) = fence {
        fence.validate_repository(&session.repository_id)?;
    }
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        begin_session_once(connection, &session, fence)
    })
}

pub(in super::super) fn finalize_session(
    connection: &mut Connection,
    session: CodeIndexSession,
) -> Result<CodeIndexSummary, StorageError> {
    finalize_session_with_fence(connection, session, None)
}

pub(in super::super) fn finalize_session_with_fence(
    connection: &mut Connection,
    session: CodeIndexSession,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexSummary, StorageError> {
    if let Some(fence) = fence {
        fence.validate_repository(&session.repository_id)?;
    }
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        finalize_session_once(connection, &session, fence)
    })
}

fn begin_session_once(
    connection: &mut Connection,
    session: &CodeIndexSession,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    let transaction = connection.transaction()?;
    super::super::tasks::retention_gc::reject_retiring_scope(&transaction, &session.source_scope)?;
    if fence.is_none() {
        super::super::tasks::enforce_unfenced_target(
            &transaction,
            &session.repository_id,
            &session.source_scope,
        )?;
    }
    let resumable = transaction
        .query_row(
            "SELECT committed_file_count FROM code_repository_index_checkpoints WHERE source_scope = ?1 AND state = 'indexing'",
            params![session.source_scope],
            |row| row.get::<_, u64>(0),
        )
        .optional()?
        .is_some_and(|committed_file_count| committed_file_count > 0);
    if session.total_path_count <= session.resource_budget.max_files_per_batch {
        super::super::schema::ensure_code_query_indexes(&transaction)?;
    }
    if !resumable {
        if session.full_replace {
            delete_scope_index(&transaction, &session.source_scope)?;
        } else {
            let mut excluded_paths = session.changed_paths.clone();
            for deleted_path in &session.deleted_paths {
                if !excluded_paths.contains(deleted_path) {
                    excluded_paths.push(deleted_path.clone());
                }
            }
            excluded_paths.sort_unstable();
            excluded_paths.dedup();
            clone_active_scope_for_incremental(
                &transaction,
                &session.repository_id,
                &session.source_scope,
                &session.path_filters,
                &session.language_filters,
                session.base_resolved_commit_sha.as_deref(),
                &excluded_paths,
            )?;
        }
        transaction.execute(
            "DELETE FROM code_repository_index_batch_staging WHERE source_scope = ?1",
            params![session.source_scope],
        )?;
        transaction.execute(
            "DELETE FROM code_repository_index_checkpoints WHERE source_scope = ?1",
            params![session.source_scope],
        )?;
    }
    commit_scope::preserve_existing_scope_commit(
        &transaction,
        &session.repository_id,
        &session.source_scope,
    )?;
    transaction.execute(
        "
        UPDATE code_repositories
        SET state = 'indexing', stale = 1, degraded_reason = NULL
        WHERE repository_id = ?1
        ",
        params![session.repository_id],
    )?;
    if !resumable {
        checkpoint::insert(&transaction, session, "indexing", None)?;
    }
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &session.source_scope)?;
        fence.validate(&transaction)?;
    }
    transaction.commit()?;

    checkpoint::load(connection, &session.source_scope)
}

fn finalize_session_once(
    connection: &mut Connection,
    session: &CodeIndexSession,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexSummary, StorageError> {
    run_finalize_phase(
        connection,
        &session.source_scope,
        finalize::phases::BUILD_QUERY_INDEXES,
        fence,
        |transaction| super::super::schema::ensure_code_query_indexes(transaction),
    )?;

    let affected_paths = if !session.full_replace && !session.changed_paths.is_empty() {
        let transaction = connection.transaction()?;
        let paths = finalize::affected_paths::compute(
            &transaction,
            &session.source_scope,
            &session.changed_paths,
            &session.deleted_paths,
        )?;
        transaction.commit()?;
        paths
    } else {
        finalize::affected_paths::AffectedPaths::full_scope()
    };

    if affected_paths.is_full_scope() {
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::RESOLVE_REFERENCES,
            fence,
            |transaction| finalize::phases::resolve_references(transaction, &session.source_scope),
        )?;
        let mut symbol_cache = finalize::phases::FinalizeSymbolCache::default();
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::RESOLVE_IMPORTS,
            fence,
            |transaction| {
                finalize::phases::resolve_imports(
                    transaction,
                    &session.source_scope,
                    &mut symbol_cache,
                )
            },
        )?;
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::RESOLVE_CALL_TARGETS,
            fence,
            |transaction| {
                finalize::phases::resolve_call_targets(transaction, &session.source_scope)
            },
        )?;
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::REFRESH_DEPENDENCIES,
            fence,
            |transaction| {
                finalize::phases::refresh_dependencies(
                    transaction,
                    &session.source_scope,
                    &session.language_filters,
                )
            },
        )?;
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::REBUILD_REFERENCE_SEARCH,
            fence,
            |transaction| {
                finalize::phases::rebuild_reference_search(transaction, &session.source_scope)
            },
        )?;
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::REBUILD_CALLS,
            fence,
            |transaction| {
                finalize::phases::rebuild_calls(
                    transaction,
                    &session.source_scope,
                    &session.repository_id,
                    &mut symbol_cache,
                )
            },
        )?;
    } else {
        let path_refs = affected_paths.path_refs();
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::RESOLVE_REFERENCES,
            fence,
            |transaction| {
                finalize::phases::resolve_references_for_paths(
                    transaction,
                    &session.source_scope,
                    &path_refs,
                )
            },
        )?;
        let mut symbol_cache = finalize::phases::FinalizeSymbolCache::default();
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::RESOLVE_IMPORTS,
            fence,
            |transaction| {
                finalize::phases::resolve_imports_for_paths(
                    transaction,
                    &session.source_scope,
                    &path_refs,
                    &mut symbol_cache,
                )
            },
        )?;
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::RESOLVE_CALL_TARGETS,
            fence,
            |transaction| {
                finalize::phases::resolve_call_targets_for_paths(
                    transaction,
                    &session.source_scope,
                    &path_refs,
                )
            },
        )?;
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::REFRESH_DEPENDENCIES,
            fence,
            |transaction| {
                finalize::phases::refresh_dependencies(
                    transaction,
                    &session.source_scope,
                    &session.language_filters,
                )
            },
        )?;
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::REBUILD_REFERENCE_SEARCH,
            fence,
            |transaction| {
                finalize::phases::rebuild_reference_search_for_paths(
                    transaction,
                    &session.source_scope,
                    &path_refs,
                )
            },
        )?;
        run_finalize_phase(
            connection,
            &session.source_scope,
            finalize::phases::REBUILD_CALLS,
            fence,
            |transaction| {
                finalize::phases::rebuild_calls_for_paths(
                    transaction,
                    &session.source_scope,
                    &session.repository_id,
                    &path_refs,
                    &mut symbol_cache,
                )
            },
        )?;
    }
    let transaction = connection.transaction()?;
    checkpoint::mark_state_in_transaction(
        &transaction,
        &session.source_scope,
        finalize::phases::PUBLISH_SCOPE,
    )?;
    publish_repository_scope(&transaction, session)?;
    checkpoint::mark_state_in_transaction(
        &transaction,
        &session.source_scope,
        finalize::phases::RESOLVE_WORKSPACE_IMPORTS,
    )?;
    workspace::resolve_workspace_imports(
        &transaction,
        &session.workspaces,
        &session.repository_id,
        &session.source_scope,
    )?;
    checkpoint::mark_completed(&transaction, &session.source_scope)?;
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &session.source_scope)?;
        fence.validate(&transaction)?;
    }
    transaction.commit()?;

    build_summary(connection, session)
}

fn run_finalize_phase(
    connection: &mut Connection,
    source_scope: &str,
    state: &str,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
    operation: impl FnOnce(&Transaction<'_>) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    checkpoint::mark_state_in_transaction(&transaction, source_scope, state)?;
    operation(&transaction)?;
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, source_scope)?;
        fence.validate(&transaction)?;
    }
    transaction.commit()?;

    Ok(())
}

fn publish_repository_scope(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
) -> Result<(), StorageError> {
    for tombstone in &session.tombstones {
        transaction.execute(
            "
            INSERT OR REPLACE INTO code_repository_path_tombstones
                (repository_id, source_scope, old_path, new_path, base_ref, head_ref)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ",
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
    let chunk_count =
        count_code_rows(transaction, "code_repository_chunks", &session.source_scope)?;
    let degraded_file_count = count_code_rows(
        transaction,
        "code_repository_file_diagnostics",
        &session.source_scope,
    )?;
    let degraded_reason = (degraded_file_count > 0)
        .then(|| format!("{degraded_file_count} file(s) degraded during code indexing"));
    transaction.execute(
        "
        INSERT INTO code_repository_scopes (
            source_scope, repository_id, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, indexed_file_count,
            symbol_count, reference_count, chunk_count, stale, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11)
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
            stale = 0,
            degraded_reason = excluded.degraded_reason
        ",
        params![
            session.source_scope,
            session.repository_id,
            session.resolved_commit_sha,
            session.tree_hash,
            checkpoint::serialize_json(&session.path_filters)?,
            checkpoint::serialize_json(&session.language_filters)?,
            file_count,
            symbol_count,
            reference_count,
            chunk_count,
            degraded_reason,
        ],
    )?;
    commit_scope::record(
        transaction,
        &session.repository_id,
        &session.resolved_commit_sha,
        &session.source_scope,
    )?;
    transaction.execute(
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
            session.repository_id,
            session.source_scope,
            session.resolved_commit_sha,
            session.tree_hash,
            file_count,
            symbol_count,
            reference_count,
            chunk_count,
            degraded_reason,
        ],
    )?;

    Ok(())
}

fn build_summary(
    connection: &mut Connection,
    session: &CodeIndexSession,
) -> Result<CodeIndexSummary, StorageError> {
    let status =
        status::repository_status(connection, &session.repository_id)?.ok_or_else(|| {
            StorageError::InvalidInput("code repository status is missing after index".to_owned())
        })?;
    let checkpoint = checkpoint::load(connection, &session.source_scope)?;
    let sqlite_write_count = checkpoint::count_scope_rows(connection, &session.source_scope)?;
    let symbol_generation_counts =
        report::scope_symbol_generation_counts(connection, &session.source_scope)?;
    let degraded_file_count =
        checkpoint::count_scope_diagnostics(connection, status.last_indexed_scope_id.as_deref())?;

    Ok(CodeIndexSummary {
        repository_id: session.repository_id.clone(),
        source_scope: session.source_scope.clone(),
        base_resolved_commit_sha: session.base_resolved_commit_sha.clone(),
        resolved_commit_sha: session.resolved_commit_sha.clone(),
        tree_hash: session.tree_hash.clone(),
        indexed_file_count: status.indexed_file_count,
        changed_path_count: session.changed_path_count,
        skipped_unchanged_count: session.skipped_unchanged_count,
        deleted_path_count: session.deleted_paths.len(),
        symbol_count: status.symbol_count,
        handwritten_symbol_count: symbol_generation_counts.handwritten,
        generated_symbol_count: symbol_generation_counts.generated,
        reference_count: status.reference_count,
        chunk_count: status.chunk_count,
        degraded_file_count,
        progress: CodeIndexProgressSummary {
            git_file_count: session.total_path_count,
            blob_read_count: checkpoint.committed_file_count,
            parsed_file_count: checkpoint.parsed_file_count,
            sqlite_write_count,
            skipped_file_count: session.skipped_unchanged_count,
            degraded_file_count,
            batch_count: checkpoint.batch_count,
            checkpoint_file_count: checkpoint.committed_file_count,
            resource_budget: session.resource_budget,
        },
    })
}
