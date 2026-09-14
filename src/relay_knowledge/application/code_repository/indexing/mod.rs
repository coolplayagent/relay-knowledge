//! Durable repository indexing workflows and worker entry points.

mod business_projection;
mod durable_incremental;
mod fast_path;
mod preview;
mod queue;
mod session;
mod start;
mod state;
mod task;
mod tasks;
mod worker;
mod workflow;

use crate::{
    api::{ApiError, CodeRepositoryIndexResponse, RequestContext},
    application::service::RelayKnowledgeService,
    domain::CodeIndexRequest,
};

use self::task::CodeIndexTaskLeaseContext;

pub(super) use task::recover_code_index_task_leases;

impl RelayKnowledgeService {
    /// Builds or updates the tree-sitter code index for a registered repository.
    pub async fn index_code_repository(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryIndexResponse, ApiError> {
        let started = self
            .start_code_repository_index(request, context.clone())
            .await?;
        if let Some(summary) = started.summary {
            return Ok(CodeRepositoryIndexResponse {
                metadata: started.metadata,
                scope: started.scope,
                summary,
                status: started.status,
            });
        }
        let task_id = started
            .task
            .as_ref()
            .map(|task| task.task_id.clone())
            .ok_or_else(|| {
                ApiError::storage_unavailable("durable repository index did not return a task")
            })?;
        self.run_code_index_task_once_with_response(Some(task_id.clone()), context)
            .await?
            .map(|(_, response)| response)
            .ok_or_else(|| {
                ApiError::qos_rejected(format!(
                    "durable repository index task '{task_id}' is already claimed or queued behind another repository writer; inspect repo status and let the managed worker drain it"
                ))
            })
    }

    async fn index_code_repository_inner(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
        task_lease: Option<CodeIndexTaskLeaseContext>,
    ) -> Result<CodeRepositoryIndexResponse, ApiError> {
        workflow::run(self, request, context, task_lease).await
    }
}

#[cfg(test)]
use self::{
    durable_incremental::{checkpoint_skips_parser, should_resume_staged_full},
    workflow::publication::incremental_snapshot_matches_lease,
};

#[cfg(test)]
#[path = "resume_tests.rs"]
mod resume_tests;
