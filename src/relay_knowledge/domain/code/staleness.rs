use serde::{Deserialize, Serialize};

/// Per-file staleness hint attached to retrieval hits at query time.
///
/// Encodes the freshness relationship between the indexed graph snapshot and
/// the live file state. Query-time freshness diagnostics can distinguish an
/// answer served from an older completed scope while a matching refresh task is
/// still pending from a scope that is simply stale.
///
/// New variants may be added in future releases; match exhaustively or use a
/// wildcard to remain forward-compatible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
#[non_exhaustive]
pub enum StalenessHint {
    Fresh,
    /// A matching index task is queued, running, or retrying for this query.
    PendingIndex {},
    /// Indexed snapshot is older than the latest file modification.
    Stale {},
}

impl StalenessHint {
    pub fn requires_source_verification(&self) -> bool {
        !matches!(self, StalenessHint::Fresh)
    }

    pub fn should_replace(&self, current: Option<&Self>) -> bool {
        current.is_none_or(|current| self.priority() > current.priority())
    }

    fn priority(&self) -> u8 {
        match self {
            StalenessHint::Fresh => 0,
            StalenessHint::Stale {} => 1,
            StalenessHint::PendingIndex {} => 2,
        }
    }
}

#[cfg(test)]
mod tests {
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
}
