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
#[path = "staleness_tests.rs"]
mod tests;
