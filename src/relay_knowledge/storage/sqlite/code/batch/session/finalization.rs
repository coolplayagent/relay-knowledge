//! Advances checkpointed code-index finalization one durable writer quantum at a time.

use rusqlite::{Connection, Transaction, params};

use super::{checkpoint, finalize};
use crate::{
    domain::{
        CodeIndexProgressSummary, CodeIndexSession, CodeIndexSummary, code_query_index_subphase,
        code_reference_search_rebuild_state,
    },
    storage::StorageError,
};

use super::super::super::{
    lifecycle::publication_fence::PublicationFenceGuard, report, status, workspace,
};

use super::reference_resolution;

mod phase;
mod publication;
mod query_index;

use phase::FinalizationCheckpointPhase;
use publication::{complete_unfenced_publication, publish_repository_scope};
use query_index::{
    advance_query_index_phase, advance_query_index_repair,
    advance_reference_search_query_index_repair, repair_query_indexes_after_coarse_checkpoint,
    repair_query_indexes_during_reference_search,
};

#[derive(Debug)]
pub(in crate::storage::sqlite::code) enum CodeIndexFinalizationAdvance {
    Pending { checkpoint_state: String },
    Ready(Box<CodeIndexSummary>),
}

pub(in crate::storage::sqlite::code) fn advance_session(
    connection: &mut Connection,
    session: CodeIndexSession,
) -> Result<CodeIndexFinalizationAdvance, StorageError> {
    advance_session_with_fence(connection, session, None)
}

pub(in crate::storage::sqlite::code) fn advance_session_with_fence(
    connection: &mut Connection,
    session: CodeIndexSession,
    fence: Option<&PublicationFenceGuard>,
) -> Result<CodeIndexFinalizationAdvance, StorageError> {
    if let Some(fence) = fence {
        fence.validate_repository(&session.repository_id)?;
    }
    super::super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        advance_session_once(connection, &session, fence)
    })
}

fn advance_session_once(
    connection: &mut Connection,
    session: &CodeIndexSession,
    fence: Option<&PublicationFenceGuard>,
) -> Result<CodeIndexFinalizationAdvance, StorageError> {
    let transaction = connection.transaction()?;
    if fence.is_none() {
        super::super::super::tasks::enforce_unfenced_target(
            &transaction,
            &session.repository_id,
            &session.source_scope,
        )?;
    }
    let persisted =
        super::load_checkpoint_resume_record(&transaction, session)?.ok_or_else(|| {
            StorageError::Invariant(format!(
                "code index checkpoint for scope '{}' is unavailable",
                session.source_scope
            ))
        })?;
    if !persisted.identity_matches {
        return Err(super::checkpoint_identity_error(session));
    }
    super::validate_checkpoint_resume_record(&persisted, session)?;
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &session.source_scope)?;
        fence.validate(&transaction)?;
    }
    require_incremental_receipt_owner(&transaction, &persisted, session, fence)?;
    if persisted.committed_file_count != persisted.total_path_count {
        return Err(super::checkpoint_invariant_error(
            session,
            "finalization requires a complete committed file prefix",
        ));
    }
    let context = FinalizationTransactionContext {
        transaction: &transaction,
        session,
        committed_reference_count: persisted.committed_reference_count,
        fence,
    };
    let advance = advance_transaction(&context, persisted.state.as_str())?;
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &session.source_scope)?;
        fence.validate(&transaction)?;
    }
    transaction.commit()?;

    match advance {
        TransactionAdvance::Pending(checkpoint_state) => {
            Ok(CodeIndexFinalizationAdvance::Pending { checkpoint_state })
        }
        TransactionAdvance::Ready => build_summary(connection, session)
            .map(Box::new)
            .map(CodeIndexFinalizationAdvance::Ready),
    }
}

fn require_incremental_receipt_owner(
    transaction: &Transaction<'_>,
    checkpoint: &super::CheckpointResumeRecord,
    session: &CodeIndexSession,
    fence: Option<&PublicationFenceGuard>,
) -> Result<(), StorageError> {
    let Some(receipt) = checkpoint.incremental_summary.as_ref() else {
        return Ok(());
    };
    let Some(fence) = fence else {
        return Err(StorageError::Invariant(format!(
            "durable incremental finalization for scope '{}' requires its publication fence",
            session.source_scope
        )));
    };
    if receipt.task_id != fence.task_id() {
        if matches!(
            checkpoint.state.as_str(),
            "completed" | finalize::phases::PARTITIONED_PUBLISH
        ) {
            if session.base_resolved_commit_sha.is_some() {
                return Err(StorageError::Invariant(format!(
                    "terminal durable incremental receipt for scope '{}' can transfer only to a generic repair session",
                    session.source_scope
                )));
            }
            let encoded = super::super::super::checkpoint_receipt::encode(receipt)?;
            let changed = transaction.execute(
                "UPDATE code_repository_index_checkpoints
                 SET incremental_summary_json = NULL
                 WHERE source_scope = ?1 AND state = ?2
                   AND incremental_summary_json = ?3",
                params![session.source_scope, checkpoint.state, encoded],
            )?;
            if changed != 1 {
                return Err(StorageError::Invariant(format!(
                    "terminal durable incremental receipt for scope '{}' changed before ownership transfer",
                    session.source_scope
                )));
            }
            return Ok(());
        }
        return Err(StorageError::Invariant(format!(
            "durable incremental receipt for scope '{}' does not match its finalization owner",
            session.source_scope
        )));
    }
    if session.base_resolved_commit_sha.as_deref()
        != Some(receipt.base_resolved_commit_sha.as_str())
        || session.resource_budget != checkpoint.resource_budget
    {
        return Err(StorageError::Invariant(format!(
            "durable incremental receipt for scope '{}' does not match its finalization owner",
            session.source_scope
        )));
    }
    Ok(())
}

pub(super) enum TransactionAdvance {
    Pending(String),
    Ready,
}

struct FinalizationTransactionContext<'transaction, 'connection> {
    transaction: &'transaction Transaction<'connection>,
    session: &'transaction CodeIndexSession,
    committed_reference_count: usize,
    fence: Option<&'transaction PublicationFenceGuard>,
}

fn advance_transaction(
    context: &FinalizationTransactionContext<'_, '_>,
    checkpoint_state: &str,
) -> Result<TransactionAdvance, StorageError> {
    let transaction = context.transaction;
    let session = context.session;
    let fence = context.fence;
    let committed_reference_count = context.committed_reference_count;
    let phase = FinalizationCheckpointPhase::decode(checkpoint_state);

    match phase {
        FinalizationCheckpointPhase::ReferenceResolutionQueryIndexRepair(repair) => {
            return reference_resolution::advance_query_index_repair(
                transaction,
                session,
                checkpoint_state,
                repair,
                fence,
            );
        }
        FinalizationCheckpointPhase::ReferenceSearchQueryIndexRepair(repair) => {
            return advance_reference_search_query_index_repair(
                transaction,
                session,
                checkpoint_state,
                repair,
                fence,
            );
        }
        FinalizationCheckpointPhase::QueryIndexRepair(repair) => {
            return advance_query_index_repair(transaction, session, checkpoint_state, repair);
        }
        FinalizationCheckpointPhase::Indexing => {
            return advance_query_index_phase(transaction, session, checkpoint_state);
        }
        FinalizationCheckpointPhase::ReferenceResolution(resolution) => {
            return reference_resolution::advance_page(
                transaction,
                session,
                checkpoint_state,
                resolution,
                committed_reference_count,
                fence,
            );
        }
        FinalizationCheckpointPhase::ReferenceSearch(reference_search) => {
            let fence = fence.ok_or_else(|| {
                StorageError::Invariant(
                    "durable reference-search progress requires a publication fence".to_owned(),
                )
            })?;
            require_unpublished_finalization_owner(transaction, session, fence)?;
            if let Some(repair) = repair_query_indexes_during_reference_search(
                transaction,
                session,
                checkpoint_state,
                reference_search,
            )? {
                require_unpublished_finalization_owner(transaction, session, fence)?;
                return Ok(repair);
            }
            super::super::super::schema::require_code_query_indexes_for_fact_publication(
                transaction,
            )?;
            let advance = finalize::search_documents::advance_reference_search_progress(
                transaction,
                &session.source_scope,
                reference_search,
            )?;
            let result =
                mark_reference_search_advance(transaction, session, checkpoint_state, advance)?;
            require_unpublished_finalization_target(transaction, session, fence)?;
            return Ok(result);
        }
        FinalizationCheckpointPhase::Coarse {
            resume_phase,
            ready_for_outer_publication,
        } => {
            if let Some(repair) = repair_query_indexes_after_coarse_checkpoint(
                transaction,
                session,
                checkpoint_state,
                resume_phase,
            )? {
                return Ok(repair);
            }
            if ready_for_outer_publication {
                return Ok(TransactionAdvance::Ready);
            }
        }
        FinalizationCheckpointPhase::Completed => {
            super::super::super::schema::validate_existing_query_indexes(transaction)?;
            return Ok(TransactionAdvance::Ready);
        }
        FinalizationCheckpointPhase::ReadyForOuterPublication => {
            return Ok(TransactionAdvance::Ready);
        }
        FinalizationCheckpointPhase::Unknown => {}
    }
    if finalization_phase_pending(checkpoint_state, finalize::phases::RESOLVE_REFERENCES)? {
        if let Some(fence) = fence
            && session.full_replace
        {
            require_unpublished_finalization_owner(transaction, session, fence)?;
            return reference_resolution::initialize(
                transaction,
                session,
                checkpoint_state,
                committed_reference_count,
                fence,
            );
        }
        finalize::phases::resolve_references(transaction, &session.source_scope)?;
        return mark_phase_pending(
            transaction,
            session,
            checkpoint_state,
            finalize::phases::RESOLVE_REFERENCES,
        );
    }
    let mut symbol_cache = finalize::phases::FinalizeSymbolCache::default();
    if finalization_phase_pending(checkpoint_state, finalize::phases::RESOLVE_IMPORTS)? {
        finalize::phases::resolve_imports(transaction, &session.source_scope, &mut symbol_cache)?;
        return mark_phase_pending(
            transaction,
            session,
            checkpoint_state,
            finalize::phases::RESOLVE_IMPORTS,
        );
    }
    if finalization_phase_pending(checkpoint_state, finalize::phases::RESOLVE_CALL_TARGETS)? {
        finalize::phases::resolve_call_targets(transaction, &session.source_scope)?;
        return mark_phase_pending(
            transaction,
            session,
            checkpoint_state,
            finalize::phases::RESOLVE_CALL_TARGETS,
        );
    }
    if finalization_phase_pending(checkpoint_state, finalize::phases::REFRESH_DEPENDENCIES)? {
        let refresh = finalize::phases::refresh_dependencies(
            transaction,
            &session.source_scope,
            &session.language_filters,
        )?;
        checkpoint::compare_and_mark_dependency_refresh(
            transaction,
            &session.source_scope,
            checkpoint_state,
            finalize::phases::REFRESH_DEPENDENCIES,
            refresh.deleted_fact_count,
            refresh.inserted_fact_count,
        )?;
        return Ok(TransactionAdvance::Pending(
            finalize::phases::REFRESH_DEPENDENCIES.to_owned(),
        ));
    }
    if finalization_phase_pending(checkpoint_state, finalize::phases::REBUILD_REFERENCE_SEARCH)? {
        if let Some(fence) = fence
            && finalization_target_is_unpublished(transaction, session, fence)?
        {
            require_unpublished_finalization_target(transaction, session, fence)?;
            let advance = finalize::search_documents::initialize_reference_search_progress(
                transaction,
                &session.source_scope,
                session.resource_budget,
                committed_reference_count,
            )?;
            let result =
                mark_reference_search_advance(transaction, session, checkpoint_state, advance)?;
            require_unpublished_finalization_target(transaction, session, fence)?;
            return Ok(result);
        }
        finalize::phases::rebuild_reference_search(
            transaction,
            &session.source_scope,
            session.resource_budget,
            committed_reference_count,
        )?;
        return mark_phase_pending(
            transaction,
            session,
            checkpoint_state,
            finalize::phases::REBUILD_REFERENCE_SEARCH,
        );
    }
    if finalization_phase_pending(checkpoint_state, finalize::phases::REBUILD_CALLS)? {
        finalize::phases::rebuild_calls(
            transaction,
            &session.source_scope,
            &session.repository_id,
            &mut symbol_cache,
        )?;
        return mark_phase_pending(
            transaction,
            session,
            checkpoint_state,
            finalize::phases::REBUILD_CALLS,
        );
    }
    if fence.is_none() {
        return complete_unfenced_publication(transaction, session, checkpoint_state);
    }
    if finalization_phase_pending(checkpoint_state, finalize::phases::PUBLISH_SCOPE)? {
        publish_repository_scope(transaction, session, true)?;
        return mark_phase_pending(
            transaction,
            session,
            checkpoint_state,
            finalize::phases::PUBLISH_SCOPE,
        );
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
        return mark_phase_pending(
            transaction,
            session,
            checkpoint_state,
            finalize::phases::RESOLVE_WORKSPACE_IMPORTS,
        );
    }
    mark_phase_pending(
        transaction,
        session,
        checkpoint_state,
        finalize::phases::SOFTWARE_PROJECTION,
    )
}

fn mark_reference_search_advance(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
    advance: finalize::search_documents::ReferenceSearchAdvance,
) -> Result<TransactionAdvance, StorageError> {
    let next_state = match advance {
        finalize::search_documents::ReferenceSearchAdvance::Pending {
            stage,
            completed_page_ordinal,
        } => code_reference_search_rebuild_state(stage, completed_page_ordinal),
        finalize::search_documents::ReferenceSearchAdvance::Complete => {
            finalize::phases::REBUILD_REFERENCE_SEARCH.to_owned()
        }
    };
    checkpoint::compare_and_mark_state(
        transaction,
        &session.source_scope,
        checkpoint_state,
        &next_state,
    )?;
    Ok(TransactionAdvance::Pending(next_state))
}

pub(super) fn finalization_target_is_unpublished(
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
    let locally_queryable = transaction.query_row(
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
    )?;
    if locally_queryable {
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

pub(super) fn require_unpublished_finalization_target(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    fence: &PublicationFenceGuard,
) -> Result<(), StorageError> {
    require_unpublished_finalization_owner(transaction, session, fence)?;
    super::super::super::schema::require_code_query_indexes_for_fact_publication(transaction)
}

pub(super) fn require_unpublished_finalization_owner(
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

fn mark_phase_pending(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
    next_state: &str,
) -> Result<TransactionAdvance, StorageError> {
    checkpoint::compare_and_mark_state(
        transaction,
        &session.source_scope,
        checkpoint_state,
        next_state,
    )?;
    Ok(TransactionAdvance::Pending(next_state.to_owned()))
}

pub(super) fn finalization_phase_pending(
    checkpoint_state: &str,
    target_phase: &str,
) -> Result<bool, StorageError> {
    if checkpoint_state == "indexing" || code_query_index_subphase(checkpoint_state).is_some() {
        return Ok(true);
    }
    if checkpoint_state == "completed" {
        return Ok(false);
    }
    let completed_position = finalize::phases::position(checkpoint_state).ok_or_else(|| {
        StorageError::Invariant(format!(
            "unknown code index finalization checkpoint state '{checkpoint_state}'"
        ))
    })?;
    let target_position = finalize::phases::position(target_phase).ok_or_else(|| {
        StorageError::Invariant(format!(
            "unknown code index finalization target phase '{target_phase}'"
        ))
    })?;

    Ok(completed_position < target_position)
}

fn build_summary(
    connection: &mut Connection,
    session: &CodeIndexSession,
) -> Result<CodeIndexSummary, StorageError> {
    let status =
        status::repository_scope_status_by_source_scope(connection, &session.source_scope)?
            .ok_or_else(|| {
                StorageError::InvalidInput(
                    "code repository scope is missing after index".to_owned(),
                )
            })?;
    let checkpoint = checkpoint::load(connection, &session.source_scope)?;
    let sqlite_write_count = checkpoint::count_scope_rows(connection, &session.source_scope)?;
    let symbol_generation_counts =
        report::scope_symbol_generation_counts(connection, &session.source_scope)?;
    let degraded_file_count =
        checkpoint::count_scope_diagnostics(connection, status.last_indexed_scope_id.as_deref())?;
    let incremental = checkpoint.incremental_summary.as_ref();

    Ok(CodeIndexSummary {
        repository_id: session.repository_id.clone(),
        source_scope: session.source_scope.clone(),
        base_resolved_commit_sha: incremental
            .map(|receipt| receipt.base_resolved_commit_sha.clone())
            .or_else(|| session.base_resolved_commit_sha.clone()),
        resolved_commit_sha: session.resolved_commit_sha.clone(),
        tree_hash: session.tree_hash.clone(),
        indexed_file_count: status.indexed_file_count,
        changed_path_count: incremental
            .map(|receipt| receipt.changed_path_count)
            .unwrap_or(session.changed_path_count),
        skipped_unchanged_count: incremental
            .map(|receipt| receipt.skipped_unchanged_count)
            .unwrap_or(session.skipped_unchanged_count),
        deleted_path_count: incremental
            .map(|receipt| receipt.deleted_path_count)
            .unwrap_or(session.deleted_paths.len()),
        symbol_count: status.symbol_count,
        handwritten_symbol_count: symbol_generation_counts.handwritten,
        generated_symbol_count: symbol_generation_counts.generated,
        reference_count: status.reference_count,
        chunk_count: status.chunk_count,
        degraded_file_count: incremental
            .map(|receipt| receipt.degraded_file_count)
            .unwrap_or(degraded_file_count),
        progress: CodeIndexProgressSummary {
            git_file_count: incremental
                .map(|receipt| receipt.changed_path_count)
                .unwrap_or(session.total_path_count),
            blob_read_count: incremental
                .map(|receipt| receipt.blob_read_count)
                .unwrap_or(checkpoint.committed_file_count),
            parsed_file_count: incremental
                .map(|receipt| receipt.parsed_file_count)
                .unwrap_or(checkpoint.parsed_file_count),
            sqlite_write_count: incremental
                .map(|receipt| receipt.sqlite_write_count)
                .unwrap_or(sqlite_write_count),
            skipped_file_count: incremental
                .map(|receipt| receipt.skipped_unchanged_count)
                .unwrap_or(session.skipped_unchanged_count),
            degraded_file_count: incremental
                .map(|receipt| receipt.degraded_file_count)
                .unwrap_or(degraded_file_count),
            batch_count: incremental
                .map(|receipt| receipt.batch_count)
                .unwrap_or(checkpoint.batch_count),
            checkpoint_file_count: incremental
                .map(|receipt| receipt.parsed_file_count)
                .unwrap_or(checkpoint.committed_file_count),
            resource_budget: session.resource_budget,
        },
    })
}
