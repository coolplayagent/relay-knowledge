//! Retrieval request planning and optional derived backend adapters.
//!
//! Retrieval owns query-shape validation and budgets before the application
//! service asks storage and derived indexes for data.

use std::{error::Error, fmt, future::Future, pin::Pin};

use crate::domain::{
    FreshnessPolicy, GraphVersion, RetrievalBackendState, RetrievalBackendStatus, RetrievalHit,
    RetrieverSource,
};

pub type RetrievalAdapterFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, RetrievalAdapterError>> + Send + 'a>>;

/// Validated retrieval request with bounded result count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalPlan {
    pub query: String,
    pub source_scope: Option<String>,
    pub limit: usize,
    pub freshness: FreshnessPolicy,
}

/// Bounded request sent to semantic or vector retrieval backends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedRetrievalRequest {
    pub query: String,
    pub source_scope: Option<String>,
    pub graph_version: GraphVersion,
    pub limit: usize,
}

/// Successful derived retrieval output with backend status metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedRetrievalOutcome {
    pub hits: Vec<RetrievalHit>,
    pub status: RetrievalBackendStatus,
}

/// Adapter boundary for semantic/vector read models.
pub trait DerivedRetrievalAdapter: Send + Sync {
    fn search(
        &self,
        request: DerivedRetrievalRequest,
    ) -> RetrievalAdapterFuture<'_, DerivedRetrievalOutcome>;
}

/// Unavailable backend adapter used until a real read model is configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendUnavailableAdapter {
    source: RetrieverSource,
    reason: &'static str,
}

impl BackendUnavailableAdapter {
    /// Creates an adapter that records an unavailable backend decision.
    pub const fn new(source: RetrieverSource, reason: &'static str) -> Self {
        Self { source, reason }
    }
}

impl DerivedRetrievalAdapter for BackendUnavailableAdapter {
    fn search(
        &self,
        request: DerivedRetrievalRequest,
    ) -> RetrievalAdapterFuture<'_, DerivedRetrievalOutcome> {
        let status = RetrievalBackendStatus {
            source: self.source,
            state: RetrievalBackendState::Unavailable,
            scope_post_filter: request.source_scope.is_some(),
            indexed_graph_version: Some(request.graph_version),
            reason: Some(self.reason.to_owned()),
        };

        Box::pin(async move { Err(RetrievalAdapterError { status }) })
    }
}

/// Adapter failure that still carries stable API metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalAdapterError {
    pub status: RetrievalBackendStatus,
}

/// Phase 1 derived backends are explicit but not yet configured.
pub fn phase1_unavailable_adapters() -> [BackendUnavailableAdapter; 2] {
    [
        BackendUnavailableAdapter::new(
            RetrieverSource::Semantic,
            "semantic retrieval backend is not configured",
        ),
        BackendUnavailableAdapter::new(
            RetrieverSource::Vector,
            "vector retrieval backend is not configured",
        ),
    ]
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

    #[tokio::test]
    async fn unavailable_adapter_reports_scope_post_filter_metadata() {
        let adapter = BackendUnavailableAdapter::new(
            RetrieverSource::Semantic,
            "semantic retrieval backend is not configured",
        );
        let error = adapter
            .search(DerivedRetrievalRequest {
                query: "SQLite".to_owned(),
                source_scope: Some("docs".to_owned()),
                graph_version: GraphVersion::new(7),
                limit: 5,
            })
            .await
            .expect_err("backend should be unavailable");

        assert_eq!(error.status.source, RetrieverSource::Semantic);
        assert_eq!(error.status.state, RetrievalBackendState::Unavailable);
        assert!(error.status.scope_post_filter);
        assert_eq!(
            error.status.indexed_graph_version,
            Some(GraphVersion::new(7))
        );
    }
}
