//! Direct stale-reason priority invariants.

use super::*;

#[test]
fn index_family_reason_prioritizes_failure_lag_state_and_last_error() {
    assert_eq!(
        index_status_reason(IndexState::Failed, 4, true),
        "index family failed"
    );
    assert_eq!(
        index_status_reason(IndexState::Fresh, 4, true),
        "index family lags graph version"
    );
    assert_eq!(
        index_status_reason(IndexState::Paused, 0, true),
        "index family is not fresh"
    );
    assert_eq!(
        index_status_reason(IndexState::Fresh, 0, true),
        "index family reports last error"
    );
    assert_eq!(
        index_status_reason(IndexState::Fresh, 0, false),
        "index family is fresh"
    );
}

#[test]
fn scoped_cursor_reason_uses_the_same_priority_with_scoped_wording() {
    assert_eq!(
        index_cursor_reason(IndexState::Failed, 4, true),
        "scoped cursor failed"
    );
    assert_eq!(
        index_cursor_reason(IndexState::Fresh, 4, true),
        "scoped cursor lags graph version"
    );
    assert_eq!(
        index_cursor_reason(IndexState::Stale, 0, true),
        "scoped cursor is not fresh"
    );
    assert_eq!(
        index_cursor_reason(IndexState::Fresh, 0, true),
        "scoped cursor reports last error"
    );
    assert_eq!(
        index_cursor_reason(IndexState::Fresh, 0, false),
        "scoped cursor is fresh"
    );
}
