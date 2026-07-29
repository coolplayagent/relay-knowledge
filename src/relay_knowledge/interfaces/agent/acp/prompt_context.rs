use crate::{
    api::{AgentRetrievalResult, CodeGraphContextResponse, RequestContext, RuntimeIdentity},
    application::RelayKnowledgeService,
};

use super::prompt_mapping::MappedPromptRequest;

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
