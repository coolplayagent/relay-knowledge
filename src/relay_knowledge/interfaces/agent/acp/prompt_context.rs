use crate::{
    api::{AgentRetrievalResult, CodeGraphContextResponse, RequestContext, RuntimeIdentity},
    application::RelayKnowledgeService,
    domain::{
        CODEGRAPH_CONTEXT_DEFAULT_LIMIT, CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES,
        CODEGRAPH_CONTEXT_MAX_BYTES, CODEGRAPH_CONTEXT_MAX_LIMIT, CODEGRAPH_CONTEXT_MIN_BYTES,
    },
};

use super::{AgentAdapterError, AgentAdapterErrorKind, MappedPromptRequest, authorize_limit};

pub(super) fn authorize_context_bytes(
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

pub(super) fn authorize_codegraph_limit(
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

pub(super) struct AcpPromptResult {
    pub(super) retrieval: Option<AgentRetrievalResult>,
    pub(super) codegraph: Option<CodeGraphContextResponse>,
}

impl AcpPromptResult {
    pub(super) fn result_count(&self) -> usize {
        self.retrieval
            .as_ref()
            .map(|result| result.results.len())
            .or_else(|| {
                self.codegraph
                    .as_ref()
                    .map(|context| context.pack.entry_points.len())
            })
            .unwrap_or(0)
    }

    pub(super) fn truncated(&self) -> bool {
        self.retrieval
            .as_ref()
            .map(|result| result.truncated)
            .or_else(|| self.codegraph.as_ref().map(|context| context.truncated))
            .unwrap_or(false)
    }
}

pub(super) async fn run_mapped_prompt(
    service: RelayKnowledgeService,
    mapped: MappedPromptRequest,
    context: RequestContext,
    identity: RuntimeIdentity,
    elapsed_ms: u64,
) -> Result<AcpPromptResult, crate::api::ApiError> {
    if mapped.repository.is_some() {
        let request = mapped
            .into_codegraph_request()
            .map_err(|error| crate::api::ApiError::invalid_argument(error.to_string()))?
            .expect("repository presence creates a codegraph request");
        let response = service.codegraph_context(request, context).await?;
        return Ok(AcpPromptResult {
            retrieval: None,
            codegraph: Some(response),
        });
    }

    let max_context_bytes = mapped.max_context_bytes;
    let response = service
        .retrieve_context(mapped.into_retrieval_request(), context)
        .await?;
    Ok(AcpPromptResult {
        retrieval: Some(AgentRetrievalResult::from_retrieval(
            response,
            identity,
            max_context_bytes,
            elapsed_ms,
        )),
        codegraph: None,
    })
}

#[cfg(test)]
#[path = "prompt_context_tests.rs"]
mod tests;
