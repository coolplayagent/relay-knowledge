mod checkpoint;
mod completion;
mod lease;
mod queue;
mod record_mapping;
mod reset;
mod retention;
mod status;
mod worktree;

pub(super) use checkpoint::{checkpoint, latest_checkpoint_for_repository};
pub(super) use completion::{complete_task, fail_task};
pub(super) use lease::{
    claim_task, recover_expired_task_leases, recover_task_leases_by_task, renew_task_lease,
    running_task_leases,
};
pub(super) use queue::queue_task;
pub(super) use reset::reset_tasks;
pub(super) use retention::{prune_scopes, prune_scopes_with_retained, retention_status};
pub(super) use status::{active_task, queue_status, task_by_id};

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
