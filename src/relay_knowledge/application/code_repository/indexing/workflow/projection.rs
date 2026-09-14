//! Workspace and derived business/software projection stage.

use crate::{
    api::{ApiError, ApiMetadata, CodeRepositoryIndexResponse, CodeRepositoryScopeMetadata},
    domain::{CodeIndexSummary, CodeRepositoryStatus},
};

use super::{
    super::{
        business_projection::refresh_business_projection, task::await_with_code_index_task_lease,
    },
    IndexWorkflowContext,
};
use crate::application::code_repository::errors::storage_api_error;

pub(super) async fn refresh(
    workflow: &IndexWorkflowContext<'_>,
    summary: CodeIndexSummary,
) -> Result<CodeRepositoryIndexResponse, ApiError> {
    if !workflow.request.workspace_detection.enabled {
        await_with_code_index_task_lease(&workflow.store, workflow.task_lease.as_ref(), async {
            match workflow.task_lease.as_ref() {
                Some(lease) => {
                    workflow
                        .store
                        .clear_code_workspace_state_with_fence(
                            summary.repository_id.clone(),
                            summary.source_scope.clone(),
                            lease.publication_fence.clone(),
                        )
                        .await
                }
                None => {
                    workflow
                        .store
                        .clear_code_workspace_state(
                            summary.repository_id.clone(),
                            summary.source_scope.clone(),
                        )
                        .await
                }
            }
            .map_err(storage_api_error)
        })
        .await?;
    }
    refresh_business_projection(
        &workflow.store,
        workflow.registration.clone(),
        &summary,
        workflow.task_lease.as_ref(),
    )
    .await?;
    let software_projection =
        await_with_code_index_task_lease(&workflow.store, workflow.task_lease.as_ref(), async {
            match workflow.task_lease.as_ref() {
                Some(lease) => {
                    workflow
                        .store
                        .refresh_software_global_projection_with_fence(
                            summary.source_scope.clone(),
                            lease.publication_fence.clone(),
                        )
                        .await
                }
                None => {
                    workflow
                        .store
                        .refresh_software_global_projection(summary.source_scope.clone())
                        .await
                }
            }
            .map_err(storage_api_error)
        })
        .await?;
    let status = workflow
        .store
        .code_repository_status(summary.repository_id.clone())
        .await
        .map_err(storage_api_error)?
        .ok_or_else(|| ApiError::storage_unavailable("code repository status is missing"))?;
    let graph_version = workflow
        .store
        .current_graph_version()
        .await
        .map_err(storage_api_error)?;
    let degraded_reason = status
        .degraded_reason
        .clone()
        .or(software_projection.status.last_error.clone());
    let status = CodeRepositoryStatus {
        degraded_reason,
        ..status
    };
    let _ = workflow
        .service
        .refresh_watched_code_repository(&status)
        .await;

    Ok(CodeRepositoryIndexResponse {
        metadata: ApiMetadata::graph_only(&workflow.context, graph_version),
        scope: CodeRepositoryScopeMetadata::from_status(
            &status,
            &workflow.request.repository,
            workflow.requested_ref.clone(),
        ),
        summary,
        status,
    })
}
