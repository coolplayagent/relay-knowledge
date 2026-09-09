use super::*;
#[test]
fn aggregate_origin_budget_accepts_exact_bounds_and_keeps_state_on_failure() {
    let mut bytes = OriginReparseBudget::default();
    bytes
        .charge(CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH)
        .unwrap();
    assert!(bytes.charge(1).is_err());
    assert!(bytes.charge(usize::MAX).is_err());
    assert_eq!(bytes.files, 1);
    assert_eq!(
        bytes.bytes,
        CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH
    );
    let mut files = OriginReparseBudget::default();
    for _ in 0..CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH {
        files.charge(0).unwrap();
    }
    assert!(files.charge(0).is_err());
    assert_eq!(
        files.files,
        CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH
    );
}
