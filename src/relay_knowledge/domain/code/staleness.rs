use serde::{Deserialize, Serialize};

/// Per-file staleness hint attached to retrieval hits at query time.
///
/// Encodes the freshness relationship between the indexed graph snapshot and
/// the live file state. The current binary model (`Fresh` / `Stale`) reflects
/// that [`CodeRepositoryStatus`](super::CodeRepositoryStatus) exposes only a
/// `stale: bool` field and cannot distinguish "pending index" from "fresh".
/// When the status model gains richer states this enum will gain matching
/// variants (enabled by `#[non_exhaustive]`).
///
/// New variants may be added in future releases; match exhaustively or use a
/// wildcard to remain forward-compatible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
#[non_exhaustive]
pub enum StalenessHint {
    Fresh,
    /// Indexed snapshot is older than the latest file modification.
    Stale {},
}

impl StalenessHint {
    pub fn is_stale(&self) -> bool {
        matches!(self, StalenessHint::Stale { .. })
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
    fn discriminants_are_distinct() {
        use std::mem::discriminant;
        let fresh = StalenessHint::Fresh;
        let stale = StalenessHint::Stale {};
        assert_ne!(discriminant(&fresh), discriminant(&stale));
    }
}
