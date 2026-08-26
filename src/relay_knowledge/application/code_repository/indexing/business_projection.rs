//! Business glossary projection inside the durable repository-index attempt.

use std::sync::Arc;

use crate::{
    api::ApiError,
    application::code_repository::{blocking::run_blocking_code, errors::storage_api_error},
    code::load_business_knowledge_projection,
    domain::{CodeIndexSummary, CodeRepositoryRegistration},
    storage::KnowledgeStore,
};

use super::task::{
    CodeIndexTaskLeaseContext, await_with_code_index_task_lease, refresh_code_index_task_lease,
};

pub(super) async fn refresh_business_projection(
    store: &Arc<dyn KnowledgeStore>,
    registration: CodeRepositoryRegistration,
    summary: &CodeIndexSummary,
    task_lease: Option<&CodeIndexTaskLeaseContext>,
) -> Result<(), ApiError> {
    refresh_code_index_task_lease(store, task_lease).await?;
    let source_scope = summary.source_scope.clone();
    let resolved_commit_sha = summary.resolved_commit_sha.clone();
    let projection = await_with_code_index_task_lease(
        store,
        task_lease,
        run_blocking_code(move || {
            load_business_knowledge_projection(&registration, &source_scope, &resolved_commit_sha)
        }),
    )
    .await?;
    await_with_code_index_task_lease(store, task_lease, async {
        match task_lease {
            Some(lease) => {
                store
                    .replace_business_knowledge_projection_with_fence(
                        projection,
                        lease.publication_fence.clone(),
                    )
                    .await
            }
            None => {
                store
                    .replace_business_knowledge_projection(projection)
                    .await
            }
        }
        .map_err(storage_api_error)
    })
    .await?;
    refresh_code_index_task_lease(store, task_lease).await
}
