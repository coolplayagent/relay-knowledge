use super::*;

#[test]
fn source_io_recheck_identity_depends_only_on_source_state_until_the_gap_is_cleared() {
    let first = source_io_recheck_observation(None, true, "partial");
    assert!(first.is_some());
    assert_eq!(first, source_io_recheck_observation(None, true, "partial"));
    assert_ne!(
        first,
        source_io_recheck_observation(None, true, "other-scope")
    );
    assert_ne!(
        first,
        source_io_recheck_observation(Some(10), true, "partial")
    );
    assert_eq!(source_io_recheck_observation(None, false, "complete"), None);
    assert_eq!(
        source_io_recheck_observation(Some(10), false, "complete"),
        Some(10)
    );
}
