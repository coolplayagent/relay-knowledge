//! Releases the SQLite writer between durable code-index finalization quanta.

use std::future::Future;

use super::{batch, lifecycle};
use crate::{
    domain::{CodeIndexPublicationFence, CodeIndexSession, CodeIndexSummary},
    storage::{
        CodeIndexFinalizationStep, CodeIndexPublicationStore, StorageError, StorageFuture,
        code_index_finalization_max_steps, sqlite::SqliteGraphStore,
    },
};

pub(super) fn advance_session_with_fence(
    store: &SqliteGraphStore,
    session: CodeIndexSession,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, CodeIndexFinalizationStep> {
    let authority_path = store.publication_authority_path.clone();
    store.run(move |connection| {
        let guard = lifecycle::publication_fence::prepare_guard(
            connection,
            fence,
            authority_path.as_deref(),
        )?;
        let advance = batch::advance_session_with_fence(connection, session, Some(&guard))?;
        Ok(finalization_step(advance))
    })
}

pub(super) fn finalize_session(
    store: &SqliteGraphStore,
    session: CodeIndexSession,
) -> StorageFuture<'_, CodeIndexSummary> {
    Box::pin(async move {
        let source_scope = session.source_scope.clone();
        let max_advances = finalization_step_bound(store, &source_scope).await?;
        drive_session_finalization(
            &source_scope,
            max_advances,
            || {
                let step_session = session.clone();
                async move {
                    store
                        .run(move |connection| batch::advance_session(connection, step_session))
                        .await
                        .map(finalization_step)
                }
            },
            CompletionMaintenance::BestEffort,
            || {
                let step_store = store;
                async move { run_best_effort_maintenance(step_store).await }
            },
        )
        .await
    })
}

pub(super) fn finalize_session_with_fence(
    store: &SqliteGraphStore,
    session: CodeIndexSession,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, CodeIndexSummary> {
    Box::pin(async move {
        let source_scope = session.source_scope.clone();
        let max_advances = finalization_step_bound(store, &source_scope).await?;
        drive_session_finalization(
            &source_scope,
            max_advances,
            || advance_session_with_fence(store, session.clone(), fence.clone()),
            CompletionMaintenance::CallerOwned,
            || async {},
        )
        .await
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompletionMaintenance {
    BestEffort,
    CallerOwned,
}

async fn drive_session_finalization<Advance, AdvanceFuture, Maintenance, MaintenanceFuture>(
    source_scope: &str,
    max_advances: usize,
    mut advance: Advance,
    completion_maintenance: CompletionMaintenance,
    mut run_maintenance: Maintenance,
) -> Result<CodeIndexSummary, StorageError>
where
    Advance: FnMut() -> AdvanceFuture,
    AdvanceFuture: Future<Output = Result<CodeIndexFinalizationStep, StorageError>>,
    Maintenance: FnMut() -> MaintenanceFuture,
    MaintenanceFuture: Future<Output = ()>,
{
    let mut previous_state = None;
    for _ in 0..max_advances {
        match advance().await? {
            CodeIndexFinalizationStep::Pending { checkpoint_state } => {
                require_progress(&mut previous_state, checkpoint_state)?;
            }
            CodeIndexFinalizationStep::Ready(summary) => {
                if completion_maintenance == CompletionMaintenance::BestEffort {
                    run_maintenance().await;
                }
                return Ok(*summary);
            }
        }
    }
    Err(bound_exhausted(source_scope))
}

async fn finalization_step_bound(
    store: &SqliteGraphStore,
    source_scope: &str,
) -> Result<usize, StorageError> {
    let checkpoint = store
        .code_index_checkpoint(source_scope.to_owned())
        .await?
        .ok_or_else(|| {
            StorageError::Invariant(format!(
                "code index checkpoint for scope '{source_scope}' is unavailable"
            ))
        })?;
    code_index_finalization_max_steps(checkpoint.committed_reference_count)
}

fn finalization_step(advance: batch::CodeIndexFinalizationAdvance) -> CodeIndexFinalizationStep {
    match advance {
        batch::CodeIndexFinalizationAdvance::Pending { checkpoint_state } => {
            CodeIndexFinalizationStep::Pending { checkpoint_state }
        }
        batch::CodeIndexFinalizationAdvance::Ready(summary) => {
            CodeIndexFinalizationStep::Ready(summary)
        }
    }
}

pub(super) async fn run_best_effort_maintenance(store: &SqliteGraphStore) {
    let maintenance = store.maintenance.clone();
    if let Err(error) = store
        .run(move |connection| {
            super::super::connection_runtime::maintenance::run_post_index_maintenance(
                connection,
                &maintenance,
            );
            Ok(())
        })
        .await
    {
        tracing::warn!(
            error = %error,
            "code index finalized but post-index SQLite maintenance did not run"
        );
    }
}

fn require_progress(
    previous_state: &mut Option<String>,
    checkpoint_state: String,
) -> Result<(), StorageError> {
    if previous_state.as_deref() == Some(checkpoint_state.as_str()) {
        return Err(StorageError::Invariant(format!(
            "code index finalization did not advance beyond checkpoint state '{checkpoint_state}'"
        )));
    }
    *previous_state = Some(checkpoint_state);
    Ok(())
}

fn bound_exhausted(source_scope: &str) -> StorageError {
    StorageError::Invariant(format!(
        "code index finalization for scope '{source_scope}' exceeded its durable step bound"
    ))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
