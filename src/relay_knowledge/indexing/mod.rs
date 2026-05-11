//! Index refresh planning for derived read models.
//!
//! This module owns index-family selection rules. Concrete index writers remain
//! outside the domain model and update only derived metadata/read models.

use crate::domain::IndexKind;

/// Deduplicated index refresh plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRefreshPlan {
    kinds: Vec<IndexKind>,
}

impl IndexRefreshPlan {
    /// Builds a refresh plan. An empty request means all v1 index families.
    pub fn from_requested(kinds: Vec<IndexKind>) -> Self {
        if kinds.is_empty() {
            return Self {
                kinds: IndexKind::ALL.to_vec(),
            };
        }

        let mut deduped = Vec::new();
        for kind in kinds {
            if !deduped.contains(&kind) {
                deduped.push(kind);
            }
        }

        Self { kinds: deduped }
    }

    /// Consumes the plan into index kinds in refresh order.
    pub fn into_kinds(self) -> Vec<IndexKind> {
        self.kinds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_request_refreshes_all_index_families() {
        let plan = IndexRefreshPlan::from_requested(Vec::new());

        assert_eq!(plan.into_kinds(), IndexKind::ALL);
    }

    #[test]
    fn duplicate_index_kinds_are_removed_in_order() {
        let plan = IndexRefreshPlan::from_requested(vec![
            IndexKind::Vector,
            IndexKind::Vector,
            IndexKind::Bm25,
        ]);

        assert_eq!(plan.into_kinds(), [IndexKind::Vector, IndexKind::Bm25]);
    }
}
