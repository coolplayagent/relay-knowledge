//! Repository-scoped research diagnostics reuse normal ref and fact-version resolution.
use super::super::{
    errors::storage_api_error,
    scope::{indexed_commit_for_selector, resolved_code_scope_status},
};
use crate::{
    api::ApiError,
    application::RelayKnowledgeService,
    domain::{CodeRepositorySelector, research::ResearchRepositoryState},
};

impl RelayKnowledgeService {
    pub(crate) async fn research_repository_state(
        &self,
        canonical_root: String,
    ) -> Result<ResearchRepositoryState, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let registration = store
            .code_repository_at_root(canonical_root)
            .await
            .map_err(storage_api_error)?;
        let mut result = ResearchRepositoryState {
            state: "not_indexed".into(),
            requested_ref: "HEAD".into(),
            resolved_target: None,
            registration,
            served_scope: None,
            diagnostic: None,
        };
        let Some(status) = &result.registration else {
            return Ok(result);
        };
        if status.last_indexed_scope_id.is_none() {
            return Ok(result);
        }
        let selector = CodeRepositorySelector::new(&status.alias, "HEAD", Vec::new(), Vec::new())
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let target = match indexed_commit_for_selector(status, &selector, "HEAD".into()).await {
            Ok(target) => target,
            Err(error) => {
                result.state = "unknown".into();
                result.diagnostic = Some(error.message);
                return Ok(result);
            }
        };
        result.resolved_target = Some(target.clone());
        let mut selector = selector;
        selector.ref_selector = target;
        match resolved_code_scope_status(&store, status, &selector).await {
            Ok(scope) => {
                result.state = if scope.stale { "stale" } else { "fresh" }.into();
                result.served_scope = Some(scope);
            }
            Err(error) => {
                result.state = "stale".into();
                result.diagnostic = Some(error.message);
            }
        }
        Ok(result)
    }
}
