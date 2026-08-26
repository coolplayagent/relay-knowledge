//! Deserialized MCP code-tool arguments and request-policy validation.

use serde::Deserialize;

use crate::{
    api::AgentAccessPolicy,
    domain::{
        BusinessKnowledgeQueryKind, CODEGRAPH_CONTEXT_DEFAULT_LIMIT,
        CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES, CODEGRAPH_CONTEXT_MAX_BYTES,
        CODEGRAPH_CONTEXT_MAX_LIMIT, CODEGRAPH_CONTEXT_MIN_BYTES, CodeQueryKind,
        SoftwareGlobalKind,
    },
    interfaces::agent::{AgentAdapterError, AgentAdapterErrorKind, authorize_limit},
};

#[derive(Debug, Deserialize)]
pub(super) struct CodeQueryArgs {
    pub(super) repository: String,
    pub(super) query: String,
    #[serde(default)]
    pub(super) kind: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) ref_selector: Option<String>,
    #[serde(default)]
    pub(super) path_filters: Vec<String>,
    #[serde(default)]
    pub(super) language_filters: Vec<String>,
    #[serde(default)]
    pub(super) freshness: Option<String>,
    #[serde(default)]
    pub(super) exclude_generated: Option<bool>,
    #[serde(default)]
    pub(super) include_code: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodeContextArgs {
    pub(super) repository: String,
    pub(super) query: String,
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) ref_selector: Option<String>,
    #[serde(default)]
    pub(super) path_filters: Vec<String>,
    #[serde(default)]
    pub(super) language_filters: Vec<String>,
    #[serde(default)]
    pub(super) freshness: Option<String>,
    #[serde(default)]
    pub(super) max_context_bytes: Option<usize>,
    #[serde(default)]
    pub(super) include_code: Option<bool>,
    #[serde(default)]
    pub(super) exclude_generated: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RepositoryGraphArgs {
    pub(super) repository: String,
    pub(super) focus_path: String,
    #[serde(default)]
    pub(super) ref_selector: Option<String>,
    #[serde(default)]
    pub(super) path_filters: Vec<String>,
    #[serde(default)]
    pub(super) depth: Option<u8>,
    #[serde(default)]
    pub(super) node_limit: Option<usize>,
    #[serde(default)]
    pub(super) edge_limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodeImpactArgs {
    pub(super) repository: String,
    pub(super) base_ref: String,
    pub(super) head_ref: String,
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) path_filters: Vec<String>,
    #[serde(default)]
    pub(super) language_filters: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodeFeatureFlagsArgs {
    pub(super) repository: String,
    #[serde(default)]
    pub(super) query: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) ref_selector: Option<String>,
    #[serde(default)]
    pub(super) path_filters: Vec<String>,
    #[serde(default)]
    pub(super) language_filters: Vec<String>,
    #[serde(default)]
    pub(super) freshness: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodeSoftwareQueryArgs {
    pub(super) repository: String,
    #[serde(default)]
    pub(super) kind: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) ref_selector: Option<String>,
    #[serde(default)]
    pub(super) path_filters: Vec<String>,
    #[serde(default)]
    pub(super) language_filters: Vec<String>,
    #[serde(default)]
    pub(super) freshness: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodeBusinessQueryArgs {
    pub(super) repository: String,
    #[serde(default)]
    pub(super) domain: Option<String>,
    #[serde(default)]
    pub(super) query: Option<String>,
    #[serde(default)]
    pub(super) kind: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) ref_selector: Option<String>,
    #[serde(default)]
    pub(super) freshness: Option<String>,
}

pub(super) fn parse_business_query_kind(
    value: &str,
) -> Result<BusinessKnowledgeQueryKind, AgentAdapterError> {
    match value {
        "terms" => Ok(BusinessKnowledgeQueryKind::Terms),
        "mappings" => Ok(BusinessKnowledgeQueryKind::Mappings),
        "all" => Ok(BusinessKnowledgeQueryKind::All),
        other => Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!("invalid business knowledge query kind '{other}'"),
        )),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CodeRepositorySetQueryArgs {
    pub(super) repository_set: String,
    pub(super) query: String,
    #[serde(default)]
    pub(super) kind: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) path_filters: Vec<String>,
    #[serde(default)]
    pub(super) language_filters: Vec<String>,
    #[serde(default)]
    pub(super) freshness: Option<String>,
    #[serde(default)]
    pub(super) exclude_generated: Option<bool>,
    #[serde(default)]
    pub(super) include_code: Option<bool>,
}

pub(super) fn parse_code_query_kind(value: &str) -> Result<CodeQueryKind, AgentAdapterError> {
    match value {
        "hybrid" => Ok(CodeQueryKind::Hybrid),
        "symbol" | "symbols" => Ok(CodeQueryKind::Symbol),
        "definition" | "definitions" => Ok(CodeQueryKind::Definition),
        "reference" | "references" => Ok(CodeQueryKind::References),
        "caller" | "callers" => Ok(CodeQueryKind::Callers),
        "callee" | "callees" => Ok(CodeQueryKind::Callees),
        "import" | "imports" => Ok(CodeQueryKind::Imports),
        "sbom" => Ok(CodeQueryKind::Sbom),
        other => Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!("invalid code query kind '{other}'"),
        )),
    }
}

fn authorize_context_bytes(
    requested: Option<usize>,
    max_context_bytes: usize,
) -> Result<usize, AgentAdapterError> {
    let value = requested.unwrap_or(max_context_bytes);
    if value == 0 {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            "max_context_bytes must be greater than zero",
        ));
    }
    if value > max_context_bytes {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::LimitExceeded,
            format!("max_context_bytes {value} exceeds MCP max_context_bytes {max_context_bytes}"),
        ));
    }

    Ok(value)
}

pub(super) fn authorize_code_context_bytes(
    requested: Option<usize>,
    max_context_bytes: usize,
) -> Result<usize, AgentAdapterError> {
    let value = match requested {
        Some(value) => authorize_context_bytes(Some(value), max_context_bytes)?,
        None => CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES.min(max_context_bytes),
    };
    if value < CODEGRAPH_CONTEXT_MIN_BYTES {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::LimitExceeded,
            format!(
                "MCP max_context_bytes {max_context_bytes} is below codegraph context minimum {CODEGRAPH_CONTEXT_MIN_BYTES}"
            ),
        ));
    }
    if value > CODEGRAPH_CONTEXT_MAX_BYTES {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::LimitExceeded,
            format!(
                "max_context_bytes {value} exceeds codegraph context max_context_bytes {CODEGRAPH_CONTEXT_MAX_BYTES}"
            ),
        ));
    }

    Ok(value)
}

pub(super) fn authorize_code_context_limit(
    limit: Option<usize>,
    policy: &AgentAccessPolicy,
) -> Result<usize, AgentAdapterError> {
    let value = match limit {
        Some(limit) => authorize_limit(Some(limit), policy)?,
        None => CODEGRAPH_CONTEXT_DEFAULT_LIMIT.min(policy.max_limit),
    };
    if value > CODEGRAPH_CONTEXT_MAX_LIMIT {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::LimitExceeded,
            format!(
                "limit {value} exceeds codegraph context max_limit {CODEGRAPH_CONTEXT_MAX_LIMIT}"
            ),
        ));
    }

    Ok(value)
}

pub(super) fn parse_software_query_kind(
    value: &str,
) -> Result<SoftwareGlobalKind, AgentAdapterError> {
    match value {
        "dependency" | "dependencies" => Ok(SoftwareGlobalKind::Dependencies),
        "sdk" | "sdks" => Ok(SoftwareGlobalKind::Sdks),
        "file" | "files" => Ok(SoftwareGlobalKind::Files),
        "topic" | "topics" => Ok(SoftwareGlobalKind::Topics),
        "relationship" | "relationships" | "config" | "configuration" | "configurations" => {
            Ok(SoftwareGlobalKind::Relationships)
        }
        "build" => Ok(SoftwareGlobalKind::Build),
        "iac" => Ok(SoftwareGlobalKind::Iac),
        "design" | "model" | "models" => Ok(SoftwareGlobalKind::Design),
        "all" => Ok(SoftwareGlobalKind::All),
        other => Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!("invalid software query kind '{other}'"),
        )),
    }
}

#[cfg(test)]
#[path = "request_contracts_tests.rs"]
mod tests;
