//! Resolves diagnostic selectors once and pins subsequent pages to that snapshot.
use super::super::{
    errors::storage_api_error,
    repository::required_code_repository,
    scope::{
        code_scope_matches_current_fact_version, indexed_commit_for_selector,
        resolved_code_scope_status,
    },
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
        let path_filters_fingerprint = request.path_filters_fingerprint();
        let cursor: Option<CodeDiagnosticsCursor> = request
            .cursor
            .as_ref()
            .map(|value| serde_json::from_str(value))
            .transpose()
            .map_err(|_| ApiError::invalid_argument("invalid diagnostic cursor"))?;
        let source_scope = if let Some(cursor) = &cursor {
            if cursor.repository_id != base.repository_id
                || cursor.requested_ref != requested_ref
                || cursor.path_filters_fingerprint != path_filters_fingerprint
            {
                return Err(ApiError::invalid_argument(
                    "diagnostic cursor does not match repository, ref or path filters",
                ));
            }
            request.repository.ref_selector = cursor.resolved_commit_sha.clone();
            cursor.source_scope.clone()
        } else {
            request.repository.ref_selector =
                indexed_commit_for_selector(&base, &request.repository, requested_ref.clone())
                    .await?;
            resolved_code_scope_status(&store, &base, &request.repository)
                .await?
                .last_indexed_scope_id
                .ok_or_else(|| ApiError::invalid_argument("diagnostic snapshot is unavailable"))?
        };
        let page = store
            .code_repository_diagnostics(CodeDiagnosticsPageRequest {
                repository_id: base.repository_id.clone(),
                source_scope: source_scope.clone(),
                resolved_commit_sha: request.repository.ref_selector.clone(),
                path_filters: request.repository.path_filters.clone(),
                limit: request.limit,
                after: cursor.map(|c| (c.after_path, c.after_message)),
            })
            .await
            .map_err(storage_api_error)?;
        let status = page.scope_status;
        if !code_scope_matches_current_fact_version(&status) {
            return Err(ApiError::invalid_argument(
                "diagnostic snapshot is unavailable at the current code fact version",
            ));
        }
        let next_cursor = if page.has_more {
            let last = page.diagnostics.last().ok_or_else(|| {
                ApiError::invalid_argument("invalid empty diagnostic continuation")
            })?;
            Some(
                CodeDiagnosticsCursor {
                    repository_id: base.repository_id,
                    source_scope,
                    resolved_commit_sha: request.repository.ref_selector.clone(),
                    requested_ref: requested_ref.clone(),
                    path_filters_fingerprint,
                    after_path: last.path.clone(),
                    after_message: last.message.clone(),
                }
                .encode()
                .map_err(ApiError::invalid_argument)?,
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
