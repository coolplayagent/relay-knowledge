use serde::{Deserialize, Serialize};

/// Freshness policy for hybrid retrieval.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessPolicy {
    #[default]
    AllowStale,
    WaitUntilFresh,
    GraphOnly,
}

/// Retrieval path used to satisfy a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    Hybrid,
    GraphOnly,
}

/// A context item returned by retrieval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalHit {
    pub evidence_id: String,
    pub source_scope: String,
    pub content: String,
    pub entity_labels: Vec<String>,
    pub score: f64,
}
