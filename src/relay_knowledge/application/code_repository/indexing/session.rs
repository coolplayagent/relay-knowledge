//! Checkpointed begin, bounded parse/write loop, and finalize session lifecycle.

use crate::{
    api::ApiError,
    application::service::RelayKnowledgeService,
    code::{CodeIndexPlan, CodeIndexPlanRecovery},
    storage::KnowledgeStore,
};

use super::{
    super::{blocking::run_blocking_code, errors::storage_api_error},
    durable_incremental::checkpoint_skips_parser,
    task::{
        CodeIndexTaskLeaseContext, await_with_code_index_task_lease,
        finalize_code_index_session_with_task_lease, refresh_code_index_task_lease,
    },
};

const PARSED_BATCH_QUEUE_CAPACITY: usize = 2;

impl RelayKnowledgeService {
    /// Runs the checkpointed session lifecycle: begin, batch loop, finalize.
    pub(super) async fn apply_code_index_from_plan(
        &self,
        store: &std::sync::Arc<dyn KnowledgeStore>,
        plan: CodeIndexPlan,
        task_lease: Option<CodeIndexTaskLeaseContext>,
    ) -> Result<crate::domain::CodeIndexSummary, ApiError> {
        let session = plan.session();
        let preflight_checkpoint = store
            .code_index_checkpoint(session.source_scope.clone())
            .await
            .map_err(storage_api_error)?;
        let (plan, content_equivalent_restart) = match preflight_checkpoint.as_ref() {
            Some(checkpoint) => {
                let checkpoint = checkpoint.clone();
                match run_blocking_code(move || plan.recover_from_checkpoint(&checkpoint)).await? {
                    CodeIndexPlanRecovery::Resume(plan) => (plan, false),
                    CodeIndexPlanRecovery::ContentEquivalentRestart(plan) => (plan, true),
                }
            }
            None => (plan, false),
        };
        let checkpoint = match task_lease.as_ref() {
            Some(lease) => {
                store
                    .begin_code_index_session_at_checkpoint_with_fence(
                        session.clone(),
                        preflight_checkpoint.clone(),
                        lease.publication_fence.clone(),
                    )
                    .await
            }
            None => {
                store
                    .begin_code_index_session_at_checkpoint(
                        session.clone(),
                        preflight_checkpoint.clone(),
                    )
                    .await
            }
        }
        .map_err(storage_api_error)?;
        let plan = match preflight_checkpoint {
            Some(preflight) => {
                if content_equivalent_restart {
                    let checkpoint = checkpoint.clone();
                    run_blocking_code(move || {
                        plan.resume_from_content_equivalent_restart_checkpoint(&checkpoint)
                    })
                    .await?
                } else if checkpoint != preflight {
                    return Err(ApiError::internal(format!(
                        "code index checkpoint '{}' changed after resume preflight",
                        checkpoint.source_scope
                    )));
                } else {
                    plan
                }
            }
            None => {
                let checkpoint = checkpoint.clone();
                run_blocking_code(move || plan.resume_from_checkpoint(&checkpoint)).await?
            }
        };
        refresh_code_index_task_lease(store, task_lease.as_ref()).await?;
        if !checkpoint_skips_parser(&checkpoint.state) {
            let (batch_sender, mut batch_receiver) =
                tokio::sync::mpsc::channel(PARSED_BATCH_QUEUE_CAPACITY);
            let parser = tokio::spawn(run_blocking_code(move || {
                let mut plan = plan;
                loop {
                    let (next_plan, batch) = plan.parse_next_batch()?;
                    plan = next_plan;
                    let Some(batch) = batch else {
                        return Ok(());
                    };
                    if batch_sender.blocking_send(batch).is_err() {
                        return Ok(());
                    }
                }
            }));
            let writer_result =
                await_with_code_index_task_lease(store, task_lease.as_ref(), async {
                    while let Some(batch) = batch_receiver.recv().await {
                        refresh_code_index_task_lease(store, task_lease.as_ref()).await?;
                        match task_lease.as_ref() {
                            Some(lease) => {
                                store
                                    .apply_code_index_batch_with_fence(
                                        batch,
                                        lease.publication_fence.clone(),
                                    )
                                    .await
                            }
                            None => store.apply_code_index_batch(batch).await,
                        }
                        .map_err(storage_api_error)?;
                        refresh_code_index_task_lease(store, task_lease.as_ref()).await?;
                    }
                    Ok::<(), ApiError>(())
                })
                .await;
            drop(batch_receiver);
            let parser_result = parser
                .await
                .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
            writer_result?;
            parser_result?;
        }

        let summary = match task_lease.as_ref() {
            Some(lease) => {
                finalize_code_index_session_with_task_lease(store, lease, session).await?
            }
            None => store
                .finalize_code_index_session(session)
                .await
                .map_err(storage_api_error)?,
        };

        Ok(summary)
    }
}
