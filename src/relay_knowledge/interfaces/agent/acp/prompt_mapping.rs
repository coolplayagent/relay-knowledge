use crate::{
    api::HybridRetrievalRequest,
    application::AgentRuntimeConfig,
    domain::{
        CODEGRAPH_CONTEXT_DEFAULT_LIMIT, CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES,
        CODEGRAPH_CONTEXT_MAX_BYTES, CODEGRAPH_CONTEXT_MAX_LIMIT, CODEGRAPH_CONTEXT_MIN_BYTES,
        CodeGraphContextRequest, CodeRepositorySelector, FreshnessPolicy,
    },
};

use super::{
    super::{authorize_limit, authorize_scope},
    AgentAdapterError, AgentAdapterErrorKind,
    protocol::AcpPromptRequest,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MappedPromptRequest {
    pub(super) query: String,
    pub(super) source_scope: Option<String>,
    pub(super) repository: Option<String>,
    ref_selector: Option<String>,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    pub(super) limit: usize,
    pub(super) freshness: FreshnessPolicy,
    pub(super) max_context_bytes: usize,
    include_code: bool,
    exclude_generated: bool,
}

impl MappedPromptRequest {
    pub(super) fn audit_scope(&self) -> Option<String> {
        self.repository
            .clone()
            .or_else(|| self.source_scope.clone())
    }

    pub(super) fn into_retrieval_request(self) -> HybridRetrievalRequest {
        HybridRetrievalRequest {
            query: self.query,
            source_scope: self.source_scope,
            limit: self.limit,
            freshness: self.freshness,
        }
    }

    pub(super) fn into_codegraph_request(
        self,
    ) -> Result<Option<CodeGraphContextRequest>, AgentAdapterError> {
        let Some(repository) = self.repository else {
            return Ok(None);
        };
        let selector = CodeRepositorySelector::new(
            repository,
            self.ref_selector.unwrap_or_else(|| "HEAD".to_owned()),
            self.path_filters,
            self.language_filters,
        )
        .map_err(|error| {
            AgentAdapterError::new(AgentAdapterErrorKind::InvalidScope, error.to_string())
        })?;

        CodeGraphContextRequest::new(
            selector,
            self.query,
            self.limit,
            self.freshness,
            self.max_context_bytes,
            self.include_code,
            self.exclude_generated,
        )
        .map(Some)
        .map_err(|error| {
            AgentAdapterError::new(AgentAdapterErrorKind::InvalidArgument, error.to_string())
        })
    }
}

pub(super) fn map_prompt_request(
    agent: &AgentRuntimeConfig,
    request: AcpPromptRequest,
) -> Result<MappedPromptRequest, AgentAdapterError> {
    let relay = request
        .meta
        .and_then(|meta| meta.relay_knowledge)
        .unwrap_or_default();
    let query = relay.query.unwrap_or(request.prompt);
    let requested_source_scope = relay.source_scope;
    let requested_repository = relay.repository;
    let repository = requested_repository
        .map(|repository| authorize_scope(Some(repository), &agent.access_policy))
        .transpose()?
        .flatten();
    let source_scope = if requested_source_scope.is_some() || repository.is_none() {
        authorize_scope(requested_source_scope, &agent.access_policy)?
    } else {
        None
    };
    let limit = if repository.is_some() {
        authorize_codegraph_limit(relay.limit, &agent.access_policy)?
    } else {
        authorize_limit(relay.limit, &agent.access_policy)?
    };
    let max_context_bytes = authorize_context_bytes(
        relay.max_context_bytes,
        agent.access_policy.max_context_bytes,
        repository.is_some(),
    )?;
    let freshness = parse_freshness(relay.freshness.as_deref())?;

    if query.trim().is_empty() {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            "ACP prompt query must not be empty",
        ));
    }

    Ok(MappedPromptRequest {
        query,
        source_scope,
        repository,
        ref_selector: relay.ref_selector,
        path_filters: relay.path_filters,
        language_filters: relay.language_filters,
        limit,
        freshness,
        max_context_bytes,
        include_code: relay.include_code.unwrap_or(true),
        exclude_generated: relay.exclude_generated.unwrap_or(false),
    })
}

fn authorize_context_bytes(
    requested: Option<usize>,
    max_context_bytes: usize,
    codegraph_context: bool,
) -> Result<usize, AgentAdapterError> {
    let default_bytes = if codegraph_context {
        CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES.min(max_context_bytes)
    } else {
        max_context_bytes
    };
    let value = requested.unwrap_or(default_bytes);
    if value == 0 {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            "max_context_bytes must be greater than zero",
        ));
    }
    if value > max_context_bytes {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::LimitExceeded,
            format!("max_context_bytes {value} exceeds ACP max_context_bytes {max_context_bytes}"),
        ));
    }
    if codegraph_context && value < CODEGRAPH_CONTEXT_MIN_BYTES {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!(
                "max_context_bytes {value} is below codegraph context minimum {CODEGRAPH_CONTEXT_MIN_BYTES}"
            ),
        ));
    }
    if codegraph_context && value > CODEGRAPH_CONTEXT_MAX_BYTES {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::LimitExceeded,
            format!(
                "max_context_bytes {value} exceeds codegraph context max_context_bytes {CODEGRAPH_CONTEXT_MAX_BYTES}"
            ),
        ));
    }

    Ok(value)
}

fn authorize_codegraph_limit(
    limit: Option<usize>,
    policy: &crate::api::AgentAccessPolicy,
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

fn parse_freshness(value: Option<&str>) -> Result<FreshnessPolicy, AgentAdapterError> {
    match value.unwrap_or("allow-stale") {
        "allow-stale" => Ok(FreshnessPolicy::AllowStale),
        "wait-until-fresh" => Ok(FreshnessPolicy::WaitUntilFresh),
        "graph-only" => Ok(FreshnessPolicy::GraphOnly),
        other => Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!("invalid freshness '{other}'"),
        )),
    }
}

#[cfg(test)]
#[path = "prompt_mapping_tests.rs"]
mod tests;
