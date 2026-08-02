//! Coordinates checkpointed session startup, finalization, and atomic scope publication.

use rusqlite::{Connection, Transaction, params};

use super::super::{
    cleanup::{count_code_rows, delete_scope_index},
    report, status, workspace,
};
use super::{checkpoint, finalize};
use crate::{
    domain::{CodeIndexCheckpoint, CodeIndexProgressSummary, CodeIndexSession, CodeIndexSummary},
    storage::StorageError,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub(in super::super) fn begin_session(
    connection: &mut Connection,
    session: CodeIndexSession,
) -> Result<CodeIndexCheckpoint, StorageError> {
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        begin_session_once(connection, &session)
    })
}

pub(in super::super) fn finalize_session(
    connection: &mut Connection,
    session: CodeIndexSession,
) -> Result<CodeIndexSummary, StorageError> {
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        finalize_session_once(connection, &session)
    })
}

fn begin_session_once(
    connection: &mut Connection,
    session: &CodeIndexSession,
) -> Result<CodeIndexCheckpoint, StorageError> {
    if !session.full_replace {
        return Err(StorageError::InvalidInput(
            "checkpointed code indexing currently requires a full-replace session".to_owned(),
        ));
    }

    let transaction = connection.transaction()?;
    delete_scope_index(&transaction, &session.source_scope)?;
    transaction.execute(
        "DELETE FROM code_repository_index_checkpoints WHERE source_scope = ?1",
        params![session.source_scope],
    )?;
    transaction.execute(
        "
        UPDATE code_repositories
        SET state = 'indexing', stale = 1, degraded_reason = NULL
        WHERE repository_id = ?1
        ",
        params![session.repository_id],
    )?;
    checkpoint::insert(&transaction, session, "indexing", None)?;
    transaction.commit()?;

    checkpoint::load(connection, &session.source_scope)
}

fn finalize_session_once(
    connection: &mut Connection,
    session: &CodeIndexSession,
) -> Result<CodeIndexSummary, StorageError> {
    run_finalize_phase(
        connection,
        &session.source_scope,
        finalize::phases::RESOLVE_REFERENCES,
        |transaction| finalize::phases::resolve_references(transaction, &session.source_scope),
    )?;
    let mut symbol_cache = finalize::phases::FinalizeSymbolCache::default();
    run_finalize_phase(
        connection,
        &session.source_scope,
        finalize::phases::RESOLVE_IMPORTS,
        |transaction| {
            finalize::phases::resolve_imports(transaction, &session.source_scope, &mut symbol_cache)
        },
    )?;
    run_finalize_phase(
        connection,
        &session.source_scope,
        finalize::phases::RESOLVE_CALL_TARGETS,
        |transaction| finalize::phases::resolve_call_targets(transaction, &session.source_scope),
    )?;
    run_finalize_phase(
        connection,
        &session.source_scope,
        finalize::phases::REFRESH_DEPENDENCIES,
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
        |transaction| {
            finalize::phases::rebuild_reference_search(transaction, &session.source_scope)
        },
    )?;
    run_finalize_phase(
        connection,
        &session.source_scope,
        finalize::phases::REBUILD_CALLS,
        |transaction| {
            finalize::phases::rebuild_calls(
                transaction,
                &session.source_scope,
                &session.repository_id,
                &mut symbol_cache,
            )
        },
    )?;
    checkpoint::mark_state(
        connection,
        &session.source_scope,
        finalize::phases::PUBLISH_SCOPE,
    )?;
    checkpoint::mark_state(
        connection,
        &session.source_scope,
        finalize::phases::RESOLVE_WORKSPACE_IMPORTS,
    )?;
    let transaction = connection.transaction()?;
    publish_repository_scope(&transaction, session)?;
    workspace::resolve_workspace_imports(
        &transaction,
        &session.workspaces,
        &session.repository_id,
        &session.source_scope,
    )?;
    checkpoint::mark_completed(&transaction, &session.source_scope)?;
    transaction.commit()?;

    build_summary(connection, session)
}

fn run_finalize_phase(
    connection: &mut Connection,
    source_scope: &str,
    state: &str,
    operation: impl FnOnce(&Transaction<'_>) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    checkpoint::mark_state(connection, source_scope, state)?;
    let transaction = connection.transaction()?;
    operation(&transaction)?;
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
