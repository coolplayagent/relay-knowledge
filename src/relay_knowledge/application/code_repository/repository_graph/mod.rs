//! Builds bounded OKF neighborhoods from one indexed repository snapshot.

use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryScopeMetadata, RepositoryGraphNeighborhoodResponseV1,
        RequestContext,
    },
    application::service::RelayKnowledgeService,
    domain::{RepositoryGraphNeighborhoodRequest, project_okf_neighborhood},
};

use super::{
    blocking::run_blocking_domain,
    errors::storage_api_error,
    repository::required_code_repository,
    scope::{
        indexed_commit_for_selector, indexed_source_scope, missing_indexed_source_scope_error,
        resolved_code_scope_status,
    },
};

const MAX_OKF_DOCUMENTS: usize = 2_048;
const MAX_OKF_DOCUMENT_BYTES: usize = 8 * 1_024 * 1_024;

impl RelayKnowledgeService {
    /// Returns an OKF concept/source neighborhood from a fresh indexed repository snapshot.
    pub async fn repository_graph_neighborhood(
        &self,
        mut request: RepositoryGraphNeighborhoodRequest,
        context: RequestContext,
    ) -> Result<RepositoryGraphNeighborhoodResponseV1, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status =
            required_code_repository(store.as_ref(), &request.repository.repository).await?;
        let requested_ref = request.repository.ref_selector.clone();
        request.repository.ref_selector = indexed_commit_for_selector(
            &status,
            &request.repository,
            request.repository.ref_selector.clone(),
        )
        .await?;
        let scoped_status =
            resolved_code_scope_status(&store, &status, &request.repository).await?;
        if scoped_status.stale {
            return Err(ApiError::invalid_argument(format!(
                "code repository '{}' graph scope is stale; refresh the index before requesting a neighborhood",
                scoped_status.alias
            )));
        }
        let source_scope = indexed_source_scope(&scoped_status)
            .ok_or_else(|| missing_indexed_source_scope_error(&scoped_status))?;
        let documents = store
            .repository_documents_for_scope(
                source_scope,
                request.repository.path_filters.clone(),
                MAX_OKF_DOCUMENTS,
                MAX_OKF_DOCUMENT_BYTES,
            )
            .await
            .map_err(storage_api_error)?;
        let projection_request = request.clone();
        let neighborhood =
            run_blocking_domain(move || project_okf_neighborhood(&documents, &projection_request))
                .await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(RepositoryGraphNeighborhoodResponseV1 {
            schema_version: 1,
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope: CodeRepositoryScopeMetadata::from_status(
                &scoped_status,
                &request.repository,
                requested_ref,
            ),
            request,
            nodes: neighborhood.nodes,
            edges: neighborhood.edges,
            truncated: neighborhood.truncated,
        })
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
