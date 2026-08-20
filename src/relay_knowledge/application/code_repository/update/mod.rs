//! Coordinates one durable, commit-to-commit repository update request.

use crate::{
    api::{
        ApiError, CodeRepositoryIndexStartResponse, CodeRepositoryUpdateRequest, RequestContext,
    },
    application::RelayKnowledgeService,
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeRepositorySelector, FreshnessPolicy,
        clean_git_commit_from_snapshot_identity,
    },
};

impl RelayKnowledgeService {
    /// Resolves optional moving refs and submits one durable incremental index task.
    pub async fn start_code_repository_update(
        &self,
        request: CodeRepositoryUpdateRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryIndexStartResponse, ApiError> {
        let head_ref = request.head_ref.unwrap_or_else(|| "HEAD".to_owned());
        let selector = CodeRepositorySelector::new(
            request.repository,
            head_ref.clone(),
            Vec::new(),
            Vec::new(),
        )
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let status = self
            .code_repository_status(selector.clone(), context.clone())
            .await?
            .status;
        let base_ref = resolve_update_base(
            request.base_ref,
            status.last_indexed_commit.as_deref(),
            &status.alias,
        )
        .map_err(ApiError::invalid_argument)?;
        let mode = CodeIndexMode::incremental(base_ref, head_ref)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;

        self.start_code_repository_index(
            CodeIndexRequest {
                repository: selector,
                mode,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context,
        )
        .await
    }
}

fn resolve_update_base(
    explicit_base: Option<String>,
    last_indexed_commit: Option<&str>,
    alias: &str,
) -> Result<String, String> {
    if let Some(explicit_base) = explicit_base {
        return Ok(explicit_base);
    }
    last_indexed_commit
        .and_then(clean_git_commit_from_snapshot_identity)
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "code repository '{alias}' has no completed clean Git snapshot; run repo index --ref HEAD before repo update, or pass an explicit --base for a filesystem snapshot"
            )
        })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
