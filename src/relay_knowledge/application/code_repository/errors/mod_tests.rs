use super::storage_api_error;
use crate::{api::ErrorKind, storage::StorageError};

#[test]
fn frozen_index_budget_maps_to_a_permanent_typed_error() {
    let error = storage_api_error(StorageError::ResourceBudgetExceeded(
        "indivisible file".to_owned(),
    ));
    assert_eq!(error.error_kind, ErrorKind::ResourceBudgetExceeded);
    assert_eq!(error.message, "indivisible file");
}

#[test]
fn code_index_task_queue_capacity_maps_to_retryable_qos_rejection() {
    let error = storage_api_error(StorageError::CapacityExceeded(
        "code index task queue is full; retry after queued work completes".to_owned(),
    ));

    assert_eq!(error.error_kind, ErrorKind::QosRejected);
    assert!(error.message.contains("retry"));
}

#[test]
fn checkpoint_invariant_maps_to_internal_error() {
    let error = storage_api_error(StorageError::Invariant(
        "durable checkpoint progress is inconsistent".to_owned(),
    ));

    assert_eq!(error.error_kind, ErrorKind::Internal);
    assert!(error.message.contains("checkpoint"));
}
