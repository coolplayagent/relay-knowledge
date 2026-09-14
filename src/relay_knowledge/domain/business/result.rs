//! Independent business query outcome and authored knowledge readiness contracts.

use super::BusinessKnowledgeStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessKnowledgeResultStatus {
    Matched,
    NoMatch,
    Ambiguous,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessKnowledgeMatchType {
    Exact,
    Partial,
}

/// Status is determined before pagination; counts describe only returned data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessKnowledgeResult {
    pub status: BusinessKnowledgeResultStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_type: Option<BusinessKnowledgeMatchType>,
    pub returned_term_count: usize,
    pub returned_mapping_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessKnowledgeState {
    Unknown,
    NoSources,
    EmptyGlossary,
    TermsOnly,
    Mapped,
}

/// Scope-wide persisted cardinalities, separate from query results and freshness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessKnowledgeSummary {
    pub state: BusinessKnowledgeState,
    #[serde(flatten)]
    pub projection: BusinessKnowledgeStatus,
}

impl BusinessKnowledgeSummary {
    pub(crate) fn from_projection(projection: BusinessKnowledgeStatus) -> Self {
        let state = if projection.source_count == 0 {
            BusinessKnowledgeState::NoSources
        } else if projection.term_count == 0 {
            BusinessKnowledgeState::EmptyGlossary
        } else if projection.mapping_count == 0 {
            BusinessKnowledgeState::TermsOnly
        } else {
            BusinessKnowledgeState::Mapped
        };
        Self { state, projection }
    }
}

#[cfg(test)]
#[path = "result_tests.rs"]
mod tests;
