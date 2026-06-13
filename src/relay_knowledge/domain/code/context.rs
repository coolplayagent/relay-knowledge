use serde::{Deserialize, Serialize};

use super::{
    CodeQueryKind, CodeRepositorySelector, CodeRetrievalHit, CodeRetrievalLayer, DomainError,
    FreshnessPolicy, RepositoryCodeRange, error::required_text,
};

pub const CODEGRAPH_CONTEXT_DEFAULT_LIMIT: usize = 8;
pub const CODEGRAPH_CONTEXT_MAX_LIMIT: usize = 20;
pub const CODEGRAPH_CONTEXT_MIN_BYTES: usize = 1024;
pub const CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES: usize = 65_536;
pub const CODEGRAPH_CONTEXT_MAX_BYTES: usize = 262_144;

/// Agent-facing one-call code graph context request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeGraphContextRequest {
    pub repository: CodeRepositorySelector,
    pub query: String,
    pub limit: usize,
    pub freshness_policy: FreshnessPolicy,
    pub max_context_bytes: usize,
    #[serde(default = "default_include_code")]
    pub include_code: bool,
    #[serde(default)]
    pub exclude_generated: bool,
}

impl CodeGraphContextRequest {
    /// Validates text and hard bounds before context orchestration starts.
    pub fn new(
        repository: CodeRepositorySelector,
        query: impl Into<String>,
        limit: usize,
        freshness_policy: FreshnessPolicy,
        max_context_bytes: usize,
        include_code: bool,
        exclude_generated: bool,
    ) -> Result<Self, DomainError> {
        let limit = match limit {
            1..=CODEGRAPH_CONTEXT_MAX_LIMIT => limit,
            0 => return Err(DomainError::invalid("limit", "must be greater than zero")),
            _ => {
                return Err(DomainError::invalid(
                    "limit",
                    "must be 20 or less for codegraph context",
                ));
            }
        };
        let max_context_bytes = match max_context_bytes {
            0..CODEGRAPH_CONTEXT_MIN_BYTES => {
                return Err(DomainError::invalid(
                    "max_context_bytes",
                    "must be at least 1024 for codegraph context",
                ));
            }
            CODEGRAPH_CONTEXT_MIN_BYTES..=CODEGRAPH_CONTEXT_MAX_BYTES => max_context_bytes,
            _ => {
                return Err(DomainError::invalid(
                    "max_context_bytes",
                    "must be 262144 or less",
                ));
            }
        };

        Ok(Self {
            repository,
            query: required_text("query", query)?,
            limit,
            freshness_policy,
            max_context_bytes,
            include_code,
            exclude_generated,
        })
    }
}

fn default_include_code() -> bool {
    true
}

/// Context item provenance used by packed codegraph responses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeGraphContextProvenance {
    pub query_kind: CodeQueryKind,
    pub retrieval_layers: Vec<CodeRetrievalLayer>,
    pub score: f64,
}

/// Compact code excerpt retained inside the context byte budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeGraphCodeExcerpt {
    pub path: String,
    pub language_id: String,
    pub line_range: RepositoryCodeRange,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_snapshot_id: Option<String>,
    pub provenance: CodeGraphContextProvenance,
    pub excerpt: String,
}

/// Structural risk hint around a seed symbol or file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeGraphImpactHint {
    pub path: String,
    pub line_range: RepositoryCodeRange,
    pub relationship: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_snapshot_id: Option<String>,
    pub retrieval_layers: Vec<CodeRetrievalLayer>,
    pub score: f64,
}

/// Budget consumed by one codegraph context orchestration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeGraphContextBudget {
    pub limit: usize,
    pub max_context_bytes: usize,
    pub candidate_count: usize,
    pub returned_count: usize,
    pub context_bytes: usize,
    pub elapsed_ms: u64,
}

/// Internal context packing input grouped by structural role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeGraphContextPack {
    pub entry_points: Vec<CodeRetrievalHit>,
    pub related_symbols: Vec<CodeRetrievalHit>,
    pub graph_paths: Vec<CodeRetrievalHit>,
    pub impact_hints: Vec<CodeGraphImpactHint>,
    pub code_excerpts: Vec<CodeGraphCodeExcerpt>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_request_rejects_empty_query() {
        let error = request(" ", 1, 1024).expect_err("empty query should fail");

        assert!(error.to_string().contains("query"));
    }

    #[test]
    fn context_request_bounds_limit_and_context_bytes() {
        assert!(request("retry", 0, 1024).is_err());
        assert!(request("retry", CODEGRAPH_CONTEXT_MAX_LIMIT + 1, 1024).is_err());
        assert!(request("retry", 1, 0).is_err());
        assert!(request("retry", 1, CODEGRAPH_CONTEXT_MIN_BYTES - 1).is_err());
        assert!(request("retry", 1, CODEGRAPH_CONTEXT_MAX_BYTES + 1).is_err());
        assert!(
            request(
                "retry",
                CODEGRAPH_CONTEXT_MAX_LIMIT,
                CODEGRAPH_CONTEXT_MIN_BYTES
            )
            .is_ok()
        );
    }

    #[test]
    fn context_request_defaults_optional_code_toggles_from_json() {
        let request: CodeGraphContextRequest = serde_json::from_value(serde_json::json!({
            "repository": {
                "repository": "repo",
                "ref_selector": "HEAD",
                "path_filters": [],
                "language_filters": []
            },
            "query": "retry",
            "limit": 1,
            "freshness_policy": "allow_stale",
            "max_context_bytes": CODEGRAPH_CONTEXT_MIN_BYTES
        }))
        .expect("request should deserialize with default toggles");

        assert!(request.include_code);
        assert!(!request.exclude_generated);
    }

    fn request(
        query: &str,
        limit: usize,
        max_context_bytes: usize,
    ) -> Result<CodeGraphContextRequest, DomainError> {
        CodeGraphContextRequest::new(
            CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())?,
            query,
            limit,
            FreshnessPolicy::AllowStale,
            max_context_bytes,
            true,
            false,
        )
    }
}
