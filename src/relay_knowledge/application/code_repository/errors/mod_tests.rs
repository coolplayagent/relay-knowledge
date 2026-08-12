use super::storage_api_error;
use crate::{api::ErrorKind, storage::StorageError};

#[test]
fn code_index_task_queue_capacity_maps_to_retryable_qos_rejection() {
    let error = storage_api_error(StorageError::CapacityExceeded(
        "code index task queue is full; retry after queued work completes".to_owned(),
    ));

    assert_eq!(error.error_kind, ErrorKind::QosRejected);
    assert!(error.message.contains("retry"));
}
