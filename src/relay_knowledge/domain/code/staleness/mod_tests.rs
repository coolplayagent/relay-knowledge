//! Direct contracts for query-time staleness priority.

use super::*;

#[test]
fn fresh_serializes_with_tag() {
    let hint = StalenessHint::Fresh;
    let json = serde_json::to_string(&hint).unwrap();
    assert_eq!(json, "{\"state\":\"fresh\"}");
    let parsed: StalenessHint = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, StalenessHint::Fresh);
}

#[test]
fn stale_round_trips() {
    let hint = StalenessHint::Stale {};
    let json = serde_json::to_string(&hint).unwrap();
    assert!(json.contains("\"state\":\"stale\""));
    let parsed: StalenessHint = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, StalenessHint::Stale {});
}

#[test]
fn pending_index_round_trips() {
    let hint = StalenessHint::PendingIndex {};
    let json = serde_json::to_string(&hint).unwrap();
    assert_eq!(json, "{\"state\":\"pending_index\"}");
    let parsed: StalenessHint = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, StalenessHint::PendingIndex {});
}

#[test]
fn pending_index_requires_source_verification_without_being_plain_stale() {
    let hint = StalenessHint::PendingIndex {};
    assert!(hint.requires_source_verification());
    assert_ne!(hint, StalenessHint::Stale {});
}

#[test]
fn pending_index_replaces_plain_stale_on_merge() {
    let pending = StalenessHint::PendingIndex {};
    assert!(pending.should_replace(Some(&StalenessHint::Stale {})));
}

#[test]
fn discriminants_are_distinct() {
    use std::mem::discriminant;
    let fresh = StalenessHint::Fresh;
    let pending = StalenessHint::PendingIndex {};
    let stale = StalenessHint::Stale {};
    assert_ne!(discriminant(&fresh), discriminant(&stale));
    assert_ne!(discriminant(&fresh), discriminant(&pending));
    assert_ne!(discriminant(&pending), discriminant(&stale));
}
