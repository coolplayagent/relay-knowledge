//! Stateful repository-index workflow stages.

mod projection;
pub(in crate::application::code_repository::indexing) mod publication;
mod recovery;
mod snapshot;

use std::sync::Arc;

use crate::{
    api::{ApiError, CodeRepositoryIndexResponse, RequestContext},
    application::service::RelayKnowledgeService,
    domain::{CodeIndexRequest, CodeRepositoryRegistration, CodeRepositoryStatus},
    storage::KnowledgeStore,
};

use super::{
    super::{errors::storage_api_error, repository::registration_from_status},
    state::requested_index_ref_for_response,
    task::{CodeIndexTaskLeaseContext, restore_rebound_worktree_task_lease},
};

pub(super) async fn run(
    service: &RelayKnowledgeService,
    request: CodeIndexRequest,
    context: RequestContext,
    task_lease: Option<CodeIndexTaskLeaseContext>,
) -> Result<CodeRepositoryIndexResponse, ApiError> {
    let store = service.store().await.map_err(storage_api_error)?;
    let task_lease = restore_rebound_worktree_task_lease(&store, &request.mode, task_lease).await?;
    let status = super::super::repository::required_code_repository(
        store.as_ref(),
        &request.repository.repository,
    )
    .await?;
    let workflow = IndexWorkflowContext {
        service,
        store,
        registration: registration_from_status(&status),
        status,
        requested_ref: requested_index_ref_for_response(&request),
        request,
        context,
        task_lease,
    };

    let recovery = match recovery::recover_and_reconcile(&workflow).await? {
        recovery::RecoveryOutcome::Published(response) => return Ok(*response),
        recovery::RecoveryOutcome::Continue(state) => *state,
    };
    let generated = snapshot::generate(&workflow, recovery).await?;
    let summary = match publication::publish(&workflow, generated).await? {
        publication::PublicationOutcome::Published(response) => return Ok(*response),
        publication::PublicationOutcome::Summary(summary) => *summary,
    };
    projection::refresh(&workflow, summary).await
}

pub(super) struct IndexWorkflowContext<'a> {
    service: &'a RelayKnowledgeService,
    store: Arc<dyn KnowledgeStore>,
    status: CodeRepositoryStatus,
    registration: CodeRepositoryRegistration,
    request: CodeIndexRequest,
    context: RequestContext,
    requested_ref: String,
    task_lease: Option<CodeIndexTaskLeaseContext>,
}
