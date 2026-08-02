//! Direct numeric-boundary contract for root metadata publication.

use super::i64_from_u64;

#[test]
fn rejects_file_metadata_values_outside_sqlite_integer_range() {
    assert!(i64_from_u64(u64::MAX).is_err());
    assert_eq!(i64_from_u64(42).expect("value should fit"), 42);
}
