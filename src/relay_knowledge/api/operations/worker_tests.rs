use crate::domain::CodeIndexTaskQueueStatus;

use super::CodeIndexWorkerStatus;

#[test]
fn worker_status_bounds_slots_and_counts_ready_work() {
    let status = CodeIndexWorkerStatus::from_queue(
        2,
        CodeIndexTaskQueueStatus {
            queued_task_count: 3,
            running_task_count: 4,
            retrying_task_count: 5,
            dead_letter_task_count: 6,
            running_lease_count: 4,
            last_error: Some("lease expired".to_owned()),
        },
    );

    assert_eq!(status.active_worker_slots, 0);
    assert_eq!(status.queue_depth, 8);
    assert_eq!(status.running_task_count, 4);
    assert_eq!(status.last_error.as_deref(), Some("lease expired"));
}
