use super::*;

#[test]
fn recognizes_only_default_optional_code_index_lease_unavailable_errors() {
    assert!(storage_error_message_is(
        &StorageError::InvalidInput(CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE.to_owned()),
        CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
    ));
    assert!(storage_error_message_is(
        &StorageError::InvalidInput(CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE.to_owned()),
        CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE,
    ));
    assert!(!storage_error_message_is(
        &StorageError::InvalidInput("code index task lease expired".to_owned()),
        CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
    ));
}

#[test]
fn code_index_worker_pid_parses_only_owned_worker_leases() {
    assert_eq!(code_index_worker_pid("code-index-worker-123"), Some(123));
    assert_eq!(code_index_worker_pid("worker-123"), None);
    assert_eq!(code_index_worker_pid("code-index-worker-"), None);
    assert_eq!(code_index_worker_pid("code-index-worker-pid"), None);
}

#[test]
fn current_process_is_treated_as_running() {
    assert!(process_is_running(
        std::process::id(),
        std::path::Path::new("tasklist.exe")
    ));
}
