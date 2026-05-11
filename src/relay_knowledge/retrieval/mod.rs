//! Retrieval request planning.
//!
//! Retrieval owns query-shape validation and budgets before the application
//! service asks storage and derived indexes for data.

use std::{error::Error, fmt};

use crate::domain::FreshnessPolicy;

/// Validated retrieval request with bounded result count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalPlan {
    pub query: String,
    pub source_scope: Option<String>,
    pub limit: usize,
    pub freshness: FreshnessPolicy,
}

impl RetrievalPlan {
    /// Validates query text and result limits.
    pub fn new(
        query: impl Into<String>,
        source_scope: Option<String>,
        limit: usize,
        freshness: FreshnessPolicy,
    ) -> Result<Self, RetrievalPlanError> {
        let query = query.into();
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(RetrievalPlanError::EmptyQuery);
        }

        let limit = match limit {
            1..=50 => limit,
            0 => return Err(RetrievalPlanError::ZeroLimit),
            _ => return Err(RetrievalPlanError::LimitTooLarge { max: 50 }),
        };

        Ok(Self {
            query: trimmed.to_owned(),
            source_scope,
            limit,
            freshness,
        })
    }
}

/// Retrieval planning error mapped to stable API errors by application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievalPlanError {
    EmptyQuery,
    ZeroLimit,
    LimitTooLarge { max: usize },
}

impl fmt::Display for RetrievalPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyQuery => write!(formatter, "query must not be empty"),
            Self::ZeroLimit => write!(formatter, "limit must be greater than zero"),
            Self::LimitTooLarge { max } => write!(formatter, "limit must be {max} or less"),
        }
    }
}

impl Error for RetrievalPlanError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_query_and_preserves_retrieval_policy() {
        let plan = RetrievalPlan::new(
            " SQLite ",
            Some("docs".to_owned()),
            5,
            FreshnessPolicy::GraphOnly,
        )
        .expect("plan should validate");

        assert_eq!(plan.query, "SQLite");
        assert_eq!(plan.source_scope, Some("docs".to_owned()));
        assert_eq!(plan.limit, 5);
        assert_eq!(plan.freshness, FreshnessPolicy::GraphOnly);
    }

    #[test]
    fn rejects_empty_and_unbounded_queries() {
        let empty = RetrievalPlan::new(" ", None, 1, FreshnessPolicy::AllowStale)
            .expect_err("empty query should fail");
        let zero = RetrievalPlan::new("x", None, 0, FreshnessPolicy::AllowStale)
            .expect_err("zero limit should fail");
        let too_large = RetrievalPlan::new("x", None, 51, FreshnessPolicy::AllowStale)
            .expect_err("large limit should fail");

        assert_eq!(empty.to_string(), "query must not be empty");
        assert_eq!(zero.to_string(), "limit must be greater than zero");
        assert_eq!(too_large.to_string(), "limit must be 50 or less");
    }
}
