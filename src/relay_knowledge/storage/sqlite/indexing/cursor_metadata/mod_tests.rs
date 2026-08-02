//! Direct cursor model metadata validation invariants.

use super::*;

#[test]
fn checked_model_dimension_accepts_u32_bounds_and_rejects_zero_or_overflow() {
    assert_eq!(checked_model_dimension(1).expect("minimum dimension"), 1);
    assert_eq!(
        checked_model_dimension(u64::from(u32::MAX)).expect("maximum dimension"),
        u32::MAX
    );
    assert!(matches!(
        checked_model_dimension(0),
        Err(StorageError::InvalidInput(message)) if message.contains("greater than zero")
    ));
    assert!(matches!(
        checked_model_dimension(u64::from(u32::MAX) + 1),
        Err(StorageError::InvalidInput(message)) if message.contains("too large")
    ));
}

#[test]
fn model_name_and_dimension_must_be_normalized_and_supplied_together() {
    assert_eq!(
        normalized_model_name(Some("  embed-v1  ")).expect("model should normalize"),
        Some("embed-v1".to_owned())
    );
    assert!(normalized_model_name(Some("  ")).is_err());
    assert!(validate_model_dimension_pair(None, None).is_ok());
    assert!(validate_model_dimension_pair(Some("embed-v1"), Some(384)).is_ok());
    assert!(validate_model_dimension_pair(Some("embed-v1"), None).is_err());
    assert!(validate_model_dimension_pair(None, Some(384)).is_err());
    assert!(validate_model_dimension_pair(Some("embed-v1"), Some(0)).is_err());
}
