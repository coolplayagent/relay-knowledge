use serde::{Deserialize, Serialize};

use super::super::{DomainError, FreshnessPolicy, error::required_text};
use super::CodeRepositorySelector;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldQualifiers {
    search_text: String,
    kind_filters: Vec<String>,
    language_filters: Vec<String>,
    path_substrings: Vec<String>,
    name_substrings: Vec<String>,
}

fn parse_field_qualifiers(query: &str) -> FieldQualifiers {
    let mut plain_terms = Vec::new();
    let mut qualifiers = FieldQualifiers {
        search_text: String::new(),
        kind_filters: Vec::new(),
        language_filters: Vec::new(),
        path_substrings: Vec::new(),
        name_substrings: Vec::new(),
    };

    for token in query.split_whitespace() {
        if !push_field_qualifier(token, &mut qualifiers) {
            plain_terms.push(token);
        }
    }

    qualifiers.search_text = plain_terms.join(" ");
    if qualifiers.search_text.is_empty() && !query.trim().is_empty() {
        qualifiers.search_text = query.trim().to_owned();
    }

    qualifiers
}

fn push_field_qualifier(token: &str, qualifiers: &mut FieldQualifiers) -> bool {
    let Some((prefix, value)) = token.split_once(':') else {
        return false;
    };
    if value.trim().is_empty() || value.starts_with(':') {
        return false;
    }

    match prefix.to_ascii_lowercase().as_str() {
        "kind" => {
            extend_qualifier_values(&mut qualifiers.kind_filters, value, true);
            true
        }
        "lang" | "language" => {
            extend_qualifier_values(&mut qualifiers.language_filters, value, true);
            true
        }
        "path" => {
            extend_qualifier_values(&mut qualifiers.path_substrings, value, false);
            true
        }
        "name" => {
            extend_qualifier_values(&mut qualifiers.name_substrings, value, false);
            true
        }
        _ => false,
    }
}

fn extend_qualifier_values(values: &mut Vec<String>, raw_value: &str, ascii_lowercase: bool) {
    for value in raw_value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let value = if ascii_lowercase {
            value.to_ascii_lowercase()
        } else {
            value.to_owned()
        };
        if !values.contains(&value) {
            values.push(value);
        }
    }
}

/// Retrieval query kind for code graph and lexical search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeQueryKind {
    Hybrid,
    Symbol,
    Definition,
    References,
    Callers,
    Callees,
    Imports,
    Sbom,
    Impact,
}

/// Code repository retrieval request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRetrievalRequest {
    pub query: String,
    pub repository: CodeRepositorySelector,
    pub code_query_kind: CodeQueryKind,
    pub limit: usize,
    pub freshness_policy: FreshnessPolicy,
    #[serde(default)]
    pub exclude_generated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_kind_filters: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_language_filters: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_path_substrings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_name_substrings: Vec<String>,
}

impl CodeRetrievalRequest {
    /// Validates query text and result limits before storage is consulted.
    pub fn new(
        query: impl Into<String>,
        repository: CodeRepositorySelector,
        code_query_kind: CodeQueryKind,
        limit: usize,
        freshness_policy: FreshnessPolicy,
    ) -> Result<Self, DomainError> {
        let limit = match limit {
            1..=50 => limit,
            0 => return Err(DomainError::invalid("limit", "must be greater than zero")),
            _ => return Err(DomainError::invalid("limit", "must be 50 or less")),
        };

        let qualifiers = parse_field_qualifiers(&required_text("query", query)?);

        Ok(Self {
            query: qualifiers.search_text,
            repository,
            code_query_kind,
            limit,
            freshness_policy,
            exclude_generated: false,
            query_kind_filters: qualifiers.kind_filters,
            query_language_filters: qualifiers.language_filters,
            query_path_substrings: qualifiers.path_substrings,
            query_name_substrings: qualifiers.name_substrings,
        })
    }
}

/// Feature-flag graph query over an indexed repository scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFeatureFlagRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub repository: CodeRepositorySelector,
    pub limit: usize,
    pub freshness_policy: FreshnessPolicy,
}

impl CodeFeatureFlagRequest {
    /// Validates optional filter text and bounds the number of returned flags.
    pub fn new(
        query: Option<String>,
        repository: CodeRepositorySelector,
        limit: usize,
        freshness_policy: FreshnessPolicy,
    ) -> Result<Self, DomainError> {
        let limit = match limit {
            1..=100 => limit,
            0 => return Err(DomainError::invalid("limit", "must be greater than zero")),
            _ => return Err(DomainError::invalid("limit", "must be 100 or less")),
        };
        let query = query
            .map(|value| required_text("query", value))
            .transpose()?;

        Ok(Self {
            query,
            repository,
            limit,
            freshness_policy,
        })
    }
}

/// Code impact analysis request over a Git diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeImpactRequest {
    pub repository: CodeRepositorySelector,
    pub base_ref: String,
    pub head_ref: String,
    pub limit: usize,
}

impl CodeImpactRequest {
    /// Validates diff refs and bounds the impact result count.
    pub fn new(
        repository: CodeRepositorySelector,
        base_ref: impl Into<String>,
        head_ref: impl Into<String>,
        limit: usize,
    ) -> Result<Self, DomainError> {
        let limit = match limit {
            1..=100 => limit,
            0 => return Err(DomainError::invalid("limit", "must be greater than zero")),
            _ => return Err(DomainError::invalid("limit", "must be 100 or less")),
        };

        Ok(Self {
            repository,
            base_ref: required_text("base_ref", base_ref)?,
            head_ref: required_text("head_ref", head_ref)?,
            limit,
        })
    }
}

/// Retrieval layer that contributed to a code hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeRetrievalLayer {
    Lexical,
    Symbol,
    Definition,
    Reference,
    CallGraph,
    ImportGraph,
    Sbom,
    Impact,
    TextFallback,
}

impl CodeRetrievalLayer {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Symbol => "symbol",
            Self::Definition => "definition",
            Self::Reference => "reference",
            Self::CallGraph => "call_graph",
            Self::ImportGraph => "import_graph",
            Self::Sbom => "sbom",
            Self::Impact => "impact",
            Self::TextFallback => "text_fallback",
        }
    }
}

#[cfg(test)]
#[path = "retrieval_request_tests.rs"]
mod tests;
