// Direct tests for repository-set error classification.

use super::storage_api_error;
use crate::{api::ErrorKind, storage::StorageError};

#[test]
fn storage_errors_preserve_invalid_input_qos_and_availability_kinds() {
    assert_eq!(
        storage_api_error(StorageError::InvalidInput("bad request".to_owned())).error_kind,
        ErrorKind::InvalidArgument
    );
    assert_eq!(
        storage_api_error(StorageError::CapacityExceeded("queue full".to_owned())).error_kind,
        ErrorKind::QosRejected
    );
    assert_eq!(
        storage_api_error(StorageError::Busy("database unavailable".to_owned())).error_kind,
        ErrorKind::StorageUnavailable
    );
}
