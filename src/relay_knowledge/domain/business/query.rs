use serde::{Deserialize, Serialize};

use crate::domain::{CodeRepositorySelector, DomainError, FreshnessPolicy};

/// Requested business projection slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessKnowledgeQueryKind {
    Terms,
    Mappings,
    All,
}

impl BusinessKnowledgeQueryKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Terms => "terms",
            Self::Mappings => "mappings",
            Self::All => "all",
        }
    }
}

/// Repository and immutable-ref bound business knowledge request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessKnowledgeQueryRequest {
    pub repository: CodeRepositorySelector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub kind: BusinessKnowledgeQueryKind,
    pub freshness_policy: FreshnessPolicy,
    pub limit: usize,
}

impl BusinessKnowledgeQueryRequest {
    pub fn new(
        repository: CodeRepositorySelector,
        domain: Option<String>,
        query: Option<String>,
        kind: BusinessKnowledgeQueryKind,
        freshness_policy: FreshnessPolicy,
        limit: usize,
    ) -> Result<Self, DomainError> {
        if !(1..=500).contains(&limit) {
            return Err(DomainError::invalid("limit", "must be between 1 and 500"));
        }
        let domain = validate_optional("domain", domain, 128)?;
        let query = validate_optional("query", query, 1_024)?;
        Ok(Self {
            repository,
            domain,
            query,
            kind,
            freshness_policy,
            limit,
        })
    }
}

fn validate_optional(
    field: &'static str,
    value: Option<String>,
    max_bytes: usize,
) -> Result<Option<String>, DomainError> {
    value
        .map(|value| {
            let value = value.trim();
            if value.is_empty() {
                return Err(DomainError::invalid(field, "must not be empty"));
            }
            if value.len() > max_bytes {
                return Err(DomainError::invalid(
                    field,
                    format!("must be {max_bytes} bytes or less"),
                ));
            }
            Ok(value.to_owned())
        })
        .transpose()
}

/// Term-name resolution state for a business query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessKnowledgeResolution {
    List,
    Exact,
    Ambiguous,
    NotFound,
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
