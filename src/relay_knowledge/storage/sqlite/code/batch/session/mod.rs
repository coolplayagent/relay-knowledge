//! Coordinates checkpointed session startup, finalization, and atomic scope publication.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::super::{cleanup::delete_scope_index, lifecycle::commit_scope};
use super::{checkpoint, finalize};
use crate::{
    domain::{
        CodeIndexCheckpoint, CodeIndexSession, code_query_index_repair, code_query_index_subphase,
        code_reference_resolution, code_reference_resolution_query_index_repair,
        code_reference_search_query_index_repair, code_reference_search_rebuild,
    },
    storage::StorageError,
};

mod finalization;

#[cfg(test)]
use finalization::finalization_phase_pending;
pub(in crate::storage::sqlite::code) use finalization::{
    CodeIndexFinalizationAdvance, advance_session, advance_session_with_fence,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "checkpoint_batch_tests.rs"]
mod checkpoint_batch_tests;

#[cfg(test)]
#[path = "publication_barrier_tests.rs"]
mod publication_barrier_tests;

#[cfg(test)]
#[path = "phase_resume_tests.rs"]
mod phase_resume_tests;

#[cfg(test)]
#[path = "query_index_policy_tests.rs"]
mod query_index_policy_tests;

#[cfg(test)]
#[path = "reference_resolution_page_tests.rs"]
mod reference_resolution_page_tests;

pub(in super::super) fn begin_session(
    connection: &mut Connection,
    session: CodeIndexSession,
) -> Result<CodeIndexCheckpoint, StorageError> {
    begin_session_with_policy(connection, session, CheckpointExpectation::Unchecked, None)
}

pub(in super::super) fn begin_session_with_fence(
    connection: &mut Connection,
    session: CodeIndexSession,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    begin_session_with_policy(connection, session, CheckpointExpectation::Unchecked, fence)
}

pub(in super::super) fn begin_session_at_checkpoint(
    connection: &mut Connection,
    session: CodeIndexSession,
    expected_checkpoint: Option<CodeIndexCheckpoint>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    begin_session_with_policy(
        connection,
        session,
        CheckpointExpectation::Exact(Box::new(expected_checkpoint)),
        None,
    )
}

pub(in super::super) fn begin_session_at_checkpoint_with_fence(
    connection: &mut Connection,
    session: CodeIndexSession,
    expected_checkpoint: Option<CodeIndexCheckpoint>,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    begin_session_with_policy(
        connection,
        session,
        CheckpointExpectation::Exact(Box::new(expected_checkpoint)),
        fence,
    )
}

pub(in crate::storage::sqlite::code) fn materialize_partitioned_completed_checkpoint(
    connection: &mut Connection,
    expected: CodeIndexCheckpoint,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    if expected.state != finalize::phases::PARTITIONED_PUBLISH {
        return Err(StorageError::Invariant(format!(
            "partitioned checkpoint '{}' is not awaiting catalog publication",
            expected.source_scope
        )));
    }
    if let Some(fence) = fence {
        fence.validate_repository(&expected.repository_id)?;
    }
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        materialize_partitioned_completed_checkpoint_once(connection, &expected, fence)
    })
}

pub(in crate::storage::sqlite::code) fn reopen_completed_checkpoint_for_partitioned_repair(
    connection: &mut Connection,
    expected: CodeIndexCheckpoint,
    fence: &super::super::lifecycle::publication_fence::PublicationFenceGuard,
) -> Result<CodeIndexCheckpoint, StorageError> {
    if expected.state != "completed" {
        return Err(StorageError::Invariant(format!(
            "partitioned checkpoint '{}' is not a completed repair candidate",
            expected.source_scope
        )));
    }
    fence.validate_repository(&expected.repository_id)?;
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        let transaction = connection.transaction()?;
        let actual = checkpoint::load_optional(&transaction, &expected.source_scope)?;
        if actual.as_ref() != Some(&expected) {
            return Err(StorageError::Invariant(format!(
                "partitioned checkpoint for scope '{}' changed before repair reopening",
                expected.source_scope
            )));
        }
        fence.validate_target_scope(&transaction, &expected.source_scope)?;
        fence.validate(&transaction)?;
        fence.validate_partitioned_staged_scope(
            &transaction,
            &expected.repository_id,
            &expected.source_scope,
        )?;
        let retain_incremental_receipt = expected
            .incremental_summary
            .as_ref()
            .is_some_and(|receipt| receipt.task_id == fence.task_id());
        let changed = transaction.execute(
            "UPDATE code_repository_index_checkpoints
             SET state = ?2,
                 incremental_summary_json = CASE
                     WHEN ?3 THEN incremental_summary_json ELSE NULL
                 END
             WHERE source_scope = ?1 AND state = 'completed'",
            params![
                expected.source_scope,
                finalize::phases::PARTITIONED_PUBLISH,
                retain_incremental_receipt,
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::Invariant(format!(
                "partitioned checkpoint for scope '{}' was not reopened exactly once",
                expected.source_scope
            )));
        }
        fence.validate_target_scope(&transaction, &expected.source_scope)?;
        fence.validate(&transaction)?;
        transaction.commit()?;
        checkpoint::load(connection, &expected.source_scope)
    })
}

fn materialize_partitioned_completed_checkpoint_once(
    connection: &mut Connection,
    expected: &CodeIndexCheckpoint,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    let transaction = connection.transaction()?;
    let actual = checkpoint::load_optional(&transaction, &expected.source_scope)?;
    if actual.as_ref() != Some(expected) {
        return Err(StorageError::Invariant(format!(
            "partitioned checkpoint for scope '{}' changed before completed-state materialization",
            expected.source_scope
        )));
    }
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &expected.source_scope)?;
        fence.validate(&transaction)?;
    }
    let changed = transaction.execute(
        "UPDATE code_repository_index_checkpoints
         SET state = 'completed'
         WHERE source_scope = ?1 AND state = ?2",
        params![expected.source_scope, finalize::phases::PARTITIONED_PUBLISH],
    )?;
    if changed != 1 {
        return Err(StorageError::Invariant(format!(
            "partitioned checkpoint for scope '{}' was not materialized exactly once",
            expected.source_scope
        )));
    }
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &expected.source_scope)?;
        fence.validate(&transaction)?;
    }
    transaction.commit()?;

    checkpoint::load(connection, &expected.source_scope)
}

enum CheckpointExpectation {
    Unchecked,
    Exact(Box<Option<CodeIndexCheckpoint>>),
}

fn begin_session_with_policy(
    connection: &mut Connection,
    session: CodeIndexSession,
    checkpoint_expectation: CheckpointExpectation,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    if let Some(fence) = fence {
        fence.validate_repository(&session.repository_id)?;
    }
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        begin_session_once(connection, &session, &checkpoint_expectation, fence)
    })
}

fn begin_session_once(
    connection: &mut Connection,
    session: &CodeIndexSession,
    checkpoint_expectation: &CheckpointExpectation,
    fence: Option<&super::super::lifecycle::publication_fence::PublicationFenceGuard>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    if !session.full_replace {
        return Err(StorageError::InvalidInput(
            "checkpointed code indexing currently requires a full-replace session".to_owned(),
        ));
    }

    let transaction = connection.transaction()?;
    super::super::tasks::retention_gc::reject_retiring_scope(&transaction, &session.source_scope)?;
    validate_checkpoint_expectation(&transaction, session, checkpoint_expectation)?;
    let resume = checkpoint_resume(&transaction, session)?;
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &session.source_scope)?;
        fence.validate(&transaction)?;
        if matches!(
            resume,
            CheckpointResume::Restart | CheckpointResume::Batches
        ) {
            super::super::publication::reject_fenced_active_scope_rebuild(
                &transaction,
                &session.repository_id,
                &session.source_scope,
            )?;
        }
    } else {
        super::super::tasks::enforce_unfenced_target(
            &transaction,
            &session.repository_id,
            &session.source_scope,
        )?;
    }
    if resume == CheckpointResume::Restart {
        delete_scope_index(&transaction, &session.source_scope)?;
        transaction.execute(
            "DELETE FROM code_repository_index_batch_staging WHERE source_scope = ?1",
            params![session.source_scope],
        )?;
        transaction.execute(
            "DELETE FROM code_repository_index_checkpoints WHERE source_scope = ?1",
            params![session.source_scope],
        )?;
        super::super::schema::prepare_restart_query_indexes(&transaction)?;
    }
    if resume == CheckpointResume::Restart {
        commit_scope::preserve_existing_scope_commit(
            &transaction,
            &session.repository_id,
            &session.source_scope,
        )?;
    }
    if matches!(
        resume,
        CheckpointResume::Restart | CheckpointResume::Batches
    ) {
        transaction.execute(
            "
            UPDATE code_repositories
            SET state = 'indexing', stale = 1, degraded_reason = NULL
            WHERE repository_id = ?1
            ",
            params![session.repository_id],
        )?;
    }
    if resume == CheckpointResume::Restart {
        checkpoint::insert(&transaction, session, "indexing", None)?;
    }
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &session.source_scope)?;
        fence.validate(&transaction)?;
    }
    transaction.commit()?;

    checkpoint::load(connection, &session.source_scope)
}

fn validate_checkpoint_expectation(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    expectation: &CheckpointExpectation,
) -> Result<(), StorageError> {
    let CheckpointExpectation::Exact(expected) = expectation else {
        return Ok(());
    };
    let actual = checkpoint::load_optional(transaction, &session.source_scope)?;
    if actual.as_ref() == expected.as_ref().as_ref() {
        return Ok(());
    }

    Err(StorageError::Invariant(format!(
        "checkpoint for scope '{}' changed after read-only plan validation",
        session.source_scope
    )))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckpointResume {
    Restart,
    Batches,
    Finalization,
    Publication,
}

struct CheckpointResumeRecord {
    state: String,
    total_path_count: usize,
    parsed_file_count: usize,
    committed_file_count: usize,
    committed_reference_count: usize,
    batch_count: usize,
    last_path: Option<String>,
    resource_budget: crate::domain::CodeIndexResourceBudget,
    incremental_summary: Option<crate::domain::CodeIncrementalSummaryReceipt>,
    content_identity_matches: bool,
    identity_matches: bool,
}

fn checkpoint_resume(
    connection: &Connection,
    session: &CodeIndexSession,
) -> Result<CheckpointResume, StorageError> {
    let Some(persisted) = load_checkpoint_resume_record(connection, session)? else {
        return Ok(CheckpointResume::Restart);
    };
    let completed_commit_alias_restart = persisted.state == "completed"
        && persisted.content_identity_matches
        && !persisted.identity_matches;
    if !persisted.identity_matches && !completed_commit_alias_restart {
        return Err(checkpoint_identity_error(session));
    }
    validate_checkpoint_resume_record(&persisted, session)?;
    if completed_commit_alias_restart {
        return Ok(CheckpointResume::Restart);
    }
    if matches!(
        persisted.state.as_str(),
        finalize::phases::SOFTWARE_PROJECTION | finalize::phases::PARTITIONED_PUBLISH | "completed"
    ) {
        return Ok(CheckpointResume::Publication);
    }
    if persisted.state == "indexing" {
        return Ok(CheckpointResume::Batches);
    }
    if finalize::phases::position(&persisted.state).is_some()
        || code_query_index_repair(&persisted.state).is_some()
        || code_query_index_subphase(&persisted.state).is_some()
        || code_reference_resolution_query_index_repair(&persisted.state).is_some()
        || code_reference_resolution(&persisted.state).is_some()
        || code_reference_search_query_index_repair(&persisted.state).is_some()
        || code_reference_search_rebuild(&persisted.state).is_some()
    {
        return Ok(CheckpointResume::Finalization);
    }

    Err(checkpoint_invariant_error(
        session,
        "checkpoint state is not resumable",
    ))
}

fn load_checkpoint_resume_record(
    connection: &Connection,
    session: &CodeIndexSession,
) -> Result<Option<CheckpointResumeRecord>, StorageError> {
    let persisted = connection
        .query_row(
            "
            SELECT repository_id, state, resolved_commit_sha, tree_hash,
                   path_filters_json, language_filters_json, total_path_count,
                   parsed_file_count, committed_file_count, committed_reference_count,
                   batch_count, last_path,
                   resource_budget_json, incremental_summary_json
            FROM code_repository_index_checkpoints
            WHERE source_scope = ?1
            ",
            params![session.source_scope],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, usize>(6)?,
                    row.get::<_, usize>(7)?,
                    row.get::<_, usize>(8)?,
                    row.get::<_, usize>(9)?,
                    row.get::<_, usize>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, Option<String>>(13)?,
                ))
            },
        )
        .optional()?;
    let Some((
        repository_id,
        state,
        commit,
        tree,
        paths,
        languages,
        total,
        parsed,
        committed,
        committed_references,
        batches,
        last_path,
        resource_budget_json,
        incremental_summary_json,
    )) = persisted
    else {
        return Ok(None);
    };
    let resource_budget = serde_json::from_str(&resource_budget_json).map_err(|error| {
        StorageError::Invariant(format!(
            "code index checkpoint '{}' has an invalid resource budget: {error}",
            session.source_scope
        ))
    })?;
    let incremental_summary =
        super::super::checkpoint_receipt::decode(incremental_summary_json, 13, resource_budget)
            .map_err(|error| {
                StorageError::Invariant(format!(
                    "code index checkpoint '{}' has an invalid incremental receipt: {error}",
                    session.source_scope
                ))
            })?;
    let content_identity_matches = repository_id == session.repository_id
        && tree == session.tree_hash
        && paths == checkpoint::serialize_json(&session.path_filters)?
        && languages == checkpoint::serialize_json(&session.language_filters)?
        && total == session.total_path_count;
    let identity_matches = content_identity_matches && commit == session.resolved_commit_sha;
    Ok(Some(CheckpointResumeRecord {
        state,
        total_path_count: total,
        parsed_file_count: parsed,
        committed_file_count: committed,
        committed_reference_count: committed_references,
        batch_count: batches,
        last_path,
        resource_budget,
        incremental_summary,
        content_identity_matches,
        identity_matches,
    }))
}

fn validate_checkpoint_resume_record(
    checkpoint: &CheckpointResumeRecord,
    session: &CodeIndexSession,
) -> Result<(), StorageError> {
    let state_is_known = checkpoint.state == "indexing"
        || checkpoint.state == "completed"
        || finalize::phases::position(&checkpoint.state).is_some()
        || code_query_index_repair(&checkpoint.state).is_some()
        || code_query_index_subphase(&checkpoint.state).is_some()
        || code_reference_resolution_query_index_repair(&checkpoint.state).is_some()
        || code_reference_resolution(&checkpoint.state).is_some()
        || code_reference_search_query_index_repair(&checkpoint.state).is_some()
        || code_reference_search_rebuild(&checkpoint.state).is_some();
    if !state_is_known {
        return Err(checkpoint_invariant_error(
            session,
            "checkpoint state is not recognized",
        ));
    }
    if checkpoint.resource_budget != session.resource_budget {
        return Err(checkpoint_invariant_error(
            session,
            "resource budget does not match the durable task",
        ));
    }
    if checkpoint.parsed_file_count != checkpoint.committed_file_count {
        return Err(checkpoint_invariant_error(
            session,
            "parsed and committed file counts differ",
        ));
    }
    let committed = checkpoint.committed_file_count;
    if committed > checkpoint.total_path_count {
        return Err(checkpoint_invariant_error(
            session,
            "committed file count exceeds total path count",
        ));
    }
    if checkpoint.state != "indexing" && committed != checkpoint.total_path_count {
        return Err(checkpoint_invariant_error(
            session,
            "finalizing or completed checkpoint has an incomplete file prefix",
        ));
    }
    if committed == 0 {
        if checkpoint.batch_count != 0 || checkpoint.last_path.is_some() {
            return Err(checkpoint_invariant_error(
                session,
                "empty committed prefix has batch or last-path progress",
            ));
        }
    } else if checkpoint.batch_count == 0
        || checkpoint.batch_count > committed
        || checkpoint
            .last_path
            .as_deref()
            .is_none_or(|path| path.trim().is_empty())
    {
        return Err(checkpoint_invariant_error(
            session,
            "committed prefix has invalid batch or last-path progress",
        ));
    }

    Ok(())
}

fn checkpoint_identity_error(session: &CodeIndexSession) -> StorageError {
    StorageError::Invariant(format!(
        "checkpoint identity for scope '{}' does not match the requested index session",
        session.source_scope
    ))
}

fn checkpoint_invariant_error(session: &CodeIndexSession, message: &str) -> StorageError {
    StorageError::Invariant(format!(
        "checkpoint invariant for scope '{}': {message}",
        session.source_scope
    ))
}
