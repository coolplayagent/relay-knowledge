use std::ops::Deref;

use crate::domain::{GraphVersion, RetrievalHit, RetrieverSource, TraversalProvenanceTrace};

/// Bounded graph search request against an explicit graph snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSearchRequest {
    pub query: String,
    pub source_scope: Option<String>,
    pub graph_version: GraphVersion,
    pub limit: usize,
    pub disabled_retriever_sources: Vec<RetrieverSource>,
}

impl GraphSearchRequest {
    /// Returns whether storage may execute a retriever family for this request.
    pub fn allows_retriever_source(&self, source: RetrieverSource) -> bool {
        !self.disabled_retriever_sources.contains(&source)
    }

    /// Maximum provenance items exposed for this bounded search.
    pub fn max_trace_items(&self) -> usize {
        self.limit
            .saturating_mul(4)
            .max(self.limit.saturating_add(8))
    }
}

/// Search hits plus the bounded traversal trace that produced them.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphSearchOutcome {
    pub hits: Vec<RetrievalHit>,
    pub trace: TraversalProvenanceTrace,
}

impl GraphSearchOutcome {
    /// Builds a trace from already-ranked hits for simple stores and test doubles.
    pub fn from_hits(request: &GraphSearchRequest, hits: Vec<RetrievalHit>) -> Self {
        let mut trace = TraversalProvenanceTrace::from_hits(
            request.graph_version,
            request.source_scope.clone(),
            routed_intent(&request.query),
            &hits,
        );
        trace.apply_budget(request.max_trace_items());

        Self { hits, trace }
    }
}

impl Deref for GraphSearchOutcome {
    type Target = [RetrievalHit];

    fn deref(&self) -> &Self::Target {
        &self.hits
    }
}

fn routed_intent(query: &str) -> String {
    if query.split_whitespace().count() <= 3 {
        "direct_context_lookup".to_owned()
    } else {
        "multi_term_context_lookup".to_owned()
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
