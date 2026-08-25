mod checkpoint;
mod completion;
mod lease;
mod queue;
mod record_mapping;
mod repository_retention;
mod reset;
mod retention;
pub(in crate::storage::sqlite::code) mod retention_gc;
mod retention_publications;
mod scope_capacity;
mod status;
mod worktree;

pub(super) use checkpoint::{checkpoint, latest_checkpoint_for_repository};
pub(super) use completion::{complete_task, fail_task, publication_receipt};
#[cfg(test)]
use completion::{complete_task_at, fail_task_at};
pub(super) use lease::{
    claim_task, recover_expired_task_leases, recover_task_leases_by_task, renew_task_lease,
    running_task_leases,
};
#[cfg(test)]
use lease::{claim_task_at, recover_expired_task_leases_at, renew_task_lease_at};
pub(super) use queue::queue_task;
pub(super) use repository_retention::{
    candidate_scan_pending as repository_retention_scan_pending,
    complete as complete_repository_retention, job as repository_retention_job,
    republished_initial_scope as repository_retention_republished_initial_scope,
    schedule as schedule_repository_retention, update_progress as update_repository_retention,
};
pub(super) use reset::reset_tasks;
#[cfg(test)]
use reset::reset_tasks_at;
pub(super) use retention::{prune_scopes, prune_scopes_with_retained, retention_status};
#[cfg(test)]
pub(in crate::storage) use scope_capacity::MAX_SCOPE_SLOTS_PER_REPOSITORY;
pub(super) use scope_capacity::{enforce_rebound_target, enforce_unfenced_target};
pub(super) use status::{active_task, queue_status, task_by_id};

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;

#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;
