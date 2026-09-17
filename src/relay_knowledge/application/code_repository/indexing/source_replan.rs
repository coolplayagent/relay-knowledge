//! Drains only fenced, unpublished source-replan retirement before changing task identity.

use std::sync::Arc;

use crate::{api::ApiError, storage::KnowledgeStore};

use super::{
    super::errors::storage_api_error,
    task::{CodeIndexTaskLeaseContext, refresh_code_index_task_lease},
};

// Every step deletes at most the existing GC row quantum; source churn cannot create an
// unbounded per-attempt cleanup loop. The enclosing task retains its timeout/cancellation.
const MAX_SOURCE_REPLAN_CLEANUP_STEPS: usize = 100_000;

pub(super) async fn drain(
    store: &Arc<dyn KnowledgeStore>,
    lease: &CodeIndexTaskLeaseContext,
    resume_only: bool,
) -> Result<(), ApiError> {
    for step in 0..MAX_SOURCE_REPLAN_CLEANUP_STEPS {
        refresh_code_index_task_lease(store, Some(lease)).await?;
        if store
            .cleanup_source_replan_with_fence(
                lease.source_scope.clone(),
                lease.publication_fence.clone(),
                resume_only || step > 0,
            )
            .await
            .map_err(storage_api_error)?
        {
            return Ok(());
        }
    }
    Err(ApiError::storage_unavailable(
        "source-replan cleanup exceeded its bounded step budget",
    ))
}

#[cfg(test)]
#[path = "source_replan_tests.rs"]
mod tests;
