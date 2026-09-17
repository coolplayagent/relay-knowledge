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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BusinessKnowledgeQueryRequest {
    pub repository: CodeRepositorySelector,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    pub kind: BusinessKnowledgeQueryKind,
    pub freshness_policy: FreshnessPolicy,
    pub limit: usize,
}

impl Serialize for BusinessKnowledgeQueryRequest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut fields = serializer.serialize_struct(
            "BusinessKnowledgeQueryRequest",
            5 + usize::from(self.domain.is_some()) + usize::from(self.query.is_some()),
        )?;
        fields.serialize_field(
            "mode",
            &if self.query.is_some() {
                BusinessKnowledgeQueryMode::Search
            } else {
                BusinessKnowledgeQueryMode::List
            },
        )?;
        fields.serialize_field("repository", &self.repository)?;
        if let Some(domain) = &self.domain {
            fields.serialize_field("domain", domain)?;
        }
        if let Some(query) = &self.query {
            fields.serialize_field("query", query)?;
        }
        fields.serialize_field("kind", &self.kind)?;
        fields.serialize_field("freshness_policy", &self.freshness_policy)?;
        fields.serialize_field("limit", &self.limit)?;
        fields.end()
    }
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

/// Query operation, independent of whether any terms match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessKnowledgeQueryMode {
    List,
    Search,
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
