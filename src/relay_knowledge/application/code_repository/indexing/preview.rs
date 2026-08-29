//! Read-only effective repository indexing-scope preview.

use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryScopeMetadata, CodeRepositoryScopePreviewResponse,
        RequestContext,
    },
    application::service::RelayKnowledgeService,
    code::preview_repository_scope,
    domain::CodeIndexRequest,
};

use super::super::{
    blocking::run_blocking_code,
    errors::storage_api_error,
    repository::{registration_from_status, required_code_repository},
    scope::merged_filters,
};

impl RelayKnowledgeService {
    /// Previews the effective code repository indexing scope without writing rows.
    pub async fn preview_code_repository_scope(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryScopePreviewResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status =
            required_code_repository(store.as_ref(), &request.repository.repository).await?;
        let registration = registration_from_status(&status);
        let selector = request.repository.clone();
        let preview =
            run_blocking_code(move || preview_repository_scope(&registration, &selector)).await?;
        let path_filters = merged_filters(&status.path_filters, &request.repository.path_filters);
        let language_filters = merged_filters(
            &status.language_filters,
            &request.repository.language_filters,
        );
        let scope_id = crate::domain::code_snapshot_scope_id_with_workspace_detection(
            &preview.repository_id,
            &preview.tree_hash,
            &path_filters,
            &language_filters,
            &request.workspace_detection,
        );
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        Ok(CodeRepositoryScopePreviewResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope: CodeRepositoryScopeMetadata {
                scope_id: scope_id.clone(),
                repository_id: preview.repository_id.clone(),
                alias: preview.alias.clone(),
                requested_ref: request.repository.ref_selector,
                resolved_commit_sha: preview.resolved_commit_sha.clone(),
                tree_hash: preview.tree_hash.clone(),
                path_filters,
                language_filters,
                indexed_file_count: preview.selected_file_count,
                index_versions: vec![format!("code:{scope_id}:{}", preview.tree_hash)],
                stale: true,
            },
            preview,
        })
    }
}
