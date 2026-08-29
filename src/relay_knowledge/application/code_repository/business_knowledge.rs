//! Coordinates immutable repository-scoped business knowledge reads.

use crate::{
    api::{ApiError, ApiMetadata, BusinessKnowledgeQueryResponse, RequestContext},
    application::service::RelayKnowledgeService,
    domain::{
        BusinessKnowledgeQueryRequest, BusinessKnowledgeResolution, BusinessKnowledgeStatus,
        FreshnessPolicy, GraphVersion,
    },
};

use super::{
    errors::storage_api_error,
    repository::{ensure_worktree_overlay_matches_current_worktree, required_code_repository},
    scope::{
        active_index_matches_request, indexed_commit_for_selector,
        latest_compatible_code_scope_status, resolved_code_scope_status,
    },
};

impl RelayKnowledgeService {
    /// Reads business terms and declared technical mappings from the indexed graph only.
    pub async fn business_knowledge_query(
        &self,
        request: BusinessKnowledgeQueryRequest,
        context: RequestContext,
    ) -> Result<BusinessKnowledgeQueryResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let repository_status =
            required_code_repository(store.as_ref(), &request.repository.repository).await?;
        if request.freshness_policy == FreshnessPolicy::GraphOnly {
            let graph_version = store
                .current_graph_version()
                .await
                .map_err(storage_api_error)?;
            let scope = crate::api::CodeRepositoryScopeMetadata::from_status(
                &repository_status,
                &request.repository,
                request.repository.ref_selector.clone(),
            );
            return Ok(BusinessKnowledgeQueryResponse {
                metadata: ApiMetadata::graph_only(&context, graph_version),
                status: BusinessKnowledgeStatus {
                    repository_id: repository_status.repository_id,
                    source_scope: scope.scope_id.clone(),
                    resolved_commit_sha: scope.resolved_commit_sha.clone(),
                    projected_graph_version: GraphVersion::ZERO,
                    stale: true,
                    source_count: 0,
                    domain_count: 0,
                    term_count: 0,
                    mapping_count: 0,
                    last_error: Some("graph_only freshness policy selected".to_owned()),
                },
                scope,
                request,
                resolution: BusinessKnowledgeResolution::NotFound,
                domains: Vec::new(),
                terms: Vec::new(),
            });
        }

        let requested_ref = request.repository.ref_selector.clone();
        let mut request = request_at_indexed_ref(request, &repository_status).await?;
        if requested_ref == "worktree" {
            ensure_worktree_overlay_matches_current_worktree(
                &store,
                &repository_status,
                &request.repository,
            )
            .await?;
        }
        let mut served_stale_scope = false;
        let scoped_status =
            match resolved_code_scope_status(&store, &repository_status, &request.repository).await
            {
                Ok(status) => status,
                Err(error) if request.freshness_policy == FreshnessPolicy::AllowStale => {
                    if !active_index_matches_request(
                        &store,
                        &repository_status,
                        &request.repository,
                    )
                    .await?
                    {
                        return Err(error);
                    }
                    let Some(stale) =
                        latest_compatible_code_scope_status(&store, &request.repository).await?
                    else {
                        return Err(error);
                    };
                    let Some(commit) = stale.last_indexed_commit.clone() else {
                        return Err(error);
                    };
                    request.repository.ref_selector = commit;
                    served_stale_scope = true;
                    stale
                }
                Err(error) => return Err(error),
            };
        let source_scope = scoped_status.last_indexed_scope_id.clone().ok_or_else(|| {
            ApiError::invalid_argument(format!(
                "code repository '{}' does not have an indexed source scope",
                scoped_status.alias
            ))
        })?;
        request.repository.repository = repository_status.repository_id.clone();
        if let Some(commit) = scoped_status.last_indexed_commit.clone() {
            request.repository.ref_selector = commit;
        }
        let projection = store
            .business_knowledge_projection_for_scope(source_scope, request.clone())
            .await
            .map_err(storage_api_error)?;
        if request.freshness_policy == FreshnessPolicy::WaitUntilFresh
            && (projection.status.stale || scoped_status.stale)
        {
            return Err(ApiError::invalid_argument(format!(
                "business knowledge projection for repository '{}' scope '{}' is stale; run repo index before querying with wait_until_fresh",
                repository_status.alias, projection.status.source_scope
            )));
        }
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let mut metadata = ApiMetadata::graph_only(&context, graph_version);
        if projection.status.stale || scoped_status.stale || served_stale_scope {
            metadata.stale = true;
        }
        let mut selector = request.repository.clone();
        selector.path_filters = scoped_status.path_filters.clone();
        selector.language_filters = scoped_status.language_filters.clone();
        let mut scope = crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &selector,
            requested_ref,
        );
        if served_stale_scope {
            scope.stale = true;
        }
        let mut status = projection.status;
        if scoped_status.stale || served_stale_scope {
            status.stale = true;
        }
        Ok(BusinessKnowledgeQueryResponse {
            metadata,
            scope,
            request,
            status,
            resolution: projection.resolution,
            domains: projection.domains,
            terms: projection.terms,
        })
    }
}

async fn request_at_indexed_ref(
    mut request: BusinessKnowledgeQueryRequest,
    status: &crate::domain::CodeRepositoryStatus,
) -> Result<BusinessKnowledgeQueryRequest, ApiError> {
    request.repository.ref_selector = indexed_commit_for_selector(
        status,
        &request.repository,
        request.repository.ref_selector.clone(),
    )
    .await?;
    Ok(request)
}
