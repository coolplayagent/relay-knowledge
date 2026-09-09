use super::storage_api_error;
use crate::{api::ErrorKind, storage::StorageError};

#[test]
fn rejected_query_input_does_not_report_a_retryable_storage_outage() {
    let error = storage_api_error(StorageError::InvalidQueryArgument(
        "configuration query contains no searchable terms".to_owned(),
    ));
    assert_eq!(error.error_kind, ErrorKind::InvalidArgument);
    assert_eq!(
        error.message,
        "configuration query contains no searchable terms"
    );
}

#[test]
fn legacy_storage_conditions_preserve_their_backend_error_mapping() {
    for message in ["file query timed out", "repository shard is missing"] {
        let error = storage_api_error(StorageError::InvalidInput(message.to_owned()));
        assert_eq!(error.error_kind, ErrorKind::StorageUnavailable);
        assert_eq!(error.message, format!("invalid storage input: {message}"));
    }
}

#[test]
fn query_work_budget_exhaustion_is_a_timeout_not_a_storage_outage() {
    let error = storage_api_error(StorageError::QueryBudgetExceeded(
        "call query incomplete; narrow path filters".to_owned(),
    ));
    assert_eq!(error.error_kind, ErrorKind::Timeout);
    assert!(error.message.contains("narrow path filters"));
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

#[test]
fn ambiguous_code_selector_is_an_actionable_argument_error() {
    let error = storage_api_error(StorageError::AmbiguousCodeSymbol(
        "use symbol_snapshot_id".to_owned(),
    ));
    assert_eq!(error.error_kind, ErrorKind::InvalidArgument);
    assert!(error.message.contains("symbol_snapshot_id"));
}
