use super::is_missing_code_scope_error;
use crate::storage::StorageError;

#[test]
fn missing_scope_classifier_does_not_hide_unrelated_storage_failures() {
    let missing = StorageError::InvalidInput("repository has no index for ref 'old'".to_owned());
    let unrelated = StorageError::InvalidInput("repository shard is missing".to_owned());

    assert!(is_missing_code_scope_error(&missing));
    assert!(!is_missing_code_scope_error(&unrelated));
}
