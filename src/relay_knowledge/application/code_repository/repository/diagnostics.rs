//! Resolves diagnostic selectors once and pins subsequent pages to that snapshot.
use super::super::{
    errors::storage_api_error,
    repository::required_code_repository,
    scope::{indexed_commit_for_selector, resolved_code_scope_status},
};
use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryDiagnosticsResponse, CodeRepositoryScopeMetadata,
        RequestContext,
    },
    application::service::RelayKnowledgeService,
    domain::{CodeDiagnosticsCursor, CodeDiagnosticsPageRequest, CodeDiagnosticsRequest},
};

impl RelayKnowledgeService {
    /// Enumerates persisted file diagnostics without parsing source or starting index work.
    pub async fn code_repository_diagnostics(
        &self,
        mut request: CodeDiagnosticsRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryDiagnosticsResponse, ApiError> {
        request.validate().map_err(ApiError::invalid_argument)?;
        request
            .normalize_paths()
            .map_err(ApiError::invalid_argument)?;
        request.repository = crate::domain::CodeRepositorySelector::new(
            request.repository.repository,
            request.repository.ref_selector,
            request.repository.path_filters,
            Vec::new(),
        )
        .map_err(|e| ApiError::invalid_argument(e.to_string()))?;
        let store = self.store().await.map_err(storage_api_error)?;
        let base = required_code_repository(store.as_ref(), &request.repository.repository).await?;
        let requested_ref = request.repository.ref_selector.clone();
        let cursor: Option<CodeDiagnosticsCursor> = request
            .cursor
            .as_ref()
            .map(|value| serde_json::from_str(value))
            .transpose()
            .map_err(|_| ApiError::invalid_argument("invalid diagnostic cursor"))?;
        if let Some(cursor) = &cursor {
            if cursor.repository_id != base.repository_id
                || cursor.requested_ref != requested_ref
                || cursor.path_filters != request.repository.path_filters
            {
                return Err(ApiError::invalid_argument(
                    "diagnostic cursor does not match repository, ref or path filters",
                ));
            }
            request.repository.ref_selector = cursor.resolved_commit_sha.clone();
        } else {
            request.repository.ref_selector =
                indexed_commit_for_selector(&base, &request.repository, requested_ref.clone())
                    .await?;
        }
        let status = resolved_code_scope_status(&store, &base, &request.repository).await?;
        let source_scope = status
            .last_indexed_scope_id
            .clone()
            .ok_or_else(|| ApiError::invalid_argument("diagnostic snapshot is unavailable"))?;
        if status.stale
            || cursor
                .as_ref()
                .is_some_and(|c| c.source_scope != source_scope)
        {
            return Err(ApiError::invalid_argument(
                "diagnostic snapshot is unavailable or no longer published",
            ));
        }
        let page = store
            .code_repository_diagnostics(CodeDiagnosticsPageRequest {
                repository_id: base.repository_id.clone(),
                source_scope: source_scope.clone(),
                path_filters: request.repository.path_filters.clone(),
                limit: request.limit,
                after: cursor.map(|c| (c.after_path, c.after_message)),
            })
            .await
            .map_err(storage_api_error)?;
        let next_cursor = if page.has_more {
            let last = page.diagnostics.last().ok_or_else(|| {
                ApiError::invalid_argument("invalid empty diagnostic continuation")
            })?;
            Some(
                serde_json::to_string(&CodeDiagnosticsCursor {
                    repository_id: base.repository_id,
                    source_scope,
                    resolved_commit_sha: request.repository.ref_selector.clone(),
                    requested_ref: requested_ref.clone(),
                    path_filters: request.repository.path_filters.clone(),
                    after_path: last.path.clone(),
                    after_message: last.message.clone(),
                })
                .map_err(|e| ApiError::invalid_argument(e.to_string()))?,
            )
        } else {
            None
        };
        let version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        Ok(CodeRepositoryDiagnosticsResponse {
            metadata: ApiMetadata::graph_only(&context, version),
            scope: CodeRepositoryScopeMetadata::from_status(
                &status,
                &request.repository,
                requested_ref,
            ),
            degraded_file_count: page.degraded_file_count,
            diagnostics: page.diagnostics,
            next_cursor,
        })
    }
}

#[cfg(test)]
mod mod_tests;
