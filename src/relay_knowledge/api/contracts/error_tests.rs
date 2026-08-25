use super::*;

#[test]
fn builds_stable_error_shapes() {
    let invalid = ApiError::invalid_argument("bad input");
    let storage = ApiError::storage_unavailable("database busy");
    let qos = ApiError::qos_rejected("request budget exhausted");
    let internal = ApiError::internal("checkpoint invariant failed");

    assert_eq!(invalid.error_kind, ErrorKind::InvalidArgument);
    assert_eq!(invalid.message, "bad input");
    assert_eq!(storage.error_kind, ErrorKind::StorageUnavailable);
    assert_eq!(storage.message, "database busy");
    assert_eq!(qos.error_kind, ErrorKind::QosRejected);
    assert_eq!(qos.message, "request budget exhausted");
    assert_eq!(internal.error_kind, ErrorKind::Internal);
    assert_eq!(internal.message, "checkpoint invariant failed");
}
