use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryFeatureFlagsResponse, CodeRepositoryQueryResponse,
        RequestContext,
    },
    domain::{CodeFeatureFlagRequest, CodeRetrievalRequest, FreshnessPolicy},
};

use crate::application::service::RelayKnowledgeService;

use super::{
    freshness::{
        CodeFeatureFlagFreshnessContext, CodeQueryFreshnessContext,
        code_feature_flag_freshness_diagnostics, code_query_freshness_diagnostics,
    },
    repository_staleness::annotate_query_result_staleness,
    support::{
        active_index_matches_request, apply_code_grep_fallback,
        feature_flag_request_at_indexed_ref, indexed_source_scope,
        latest_compatible_code_scope_status, missing_indexed_source_scope_error,
        required_code_repository, resolved_code_scope_status, retrieval_request_at_indexed_ref,
        storage_api_error,
    },
    worktree_freshness::ensure_worktree_overlay_matches_current_worktree,
};

impl RelayKnowledgeService {
    /// Queries indexed symbols, references, imports, calls, and code chunks.
    pub async fn query_code_repository(
        &self,
        request: CodeRetrievalRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryQueryResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        if request.freshness_policy == FreshnessPolicy::GraphOnly {
            let graph_version = store
                .current_graph_version()
                .await
                .map_err(storage_api_error)?;
            let degraded_reason = "graph_only freshness policy selected".to_owned();
            return Ok(CodeRepositoryQueryResponse {
                metadata: ApiMetadata::graph_only(&context, graph_version),
                scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                    &status,
                    &request.repository,
                    request.repository.ref_selector.clone(),
                ),
                freshness: crate::api::CodeRepositoryFreshnessDiagnostics::graph_only(
                    graph_version.get(),
                    request.freshness_policy,
                    indexed_source_scope(&status),
                    request.repository.ref_selector.clone(),
                    degraded_reason.clone(),
                ),
                request,
                results: Vec::new(),
                degraded_reason: Some(degraded_reason),
            });
        }
        let requested_ref = request.repository.ref_selector.clone();
        let mut request = retrieval_request_at_indexed_ref(request, &status).await?;
        if requested_ref == "worktree" {
            ensure_worktree_overlay_matches_current_worktree(&store, &status, &request.repository)
                .await?;
        }
        let requested_resolved_ref = request.repository.ref_selector.clone();
        let freshness_target = request.repository.clone();
        let mut served_stale_scope = false;
        let mut stale_reason = None;
        let scoped_status = match resolved_code_scope_status(&store, &status, &request.repository)
            .await
        {
            Ok(scoped_status) => scoped_status,
            Err(error) if request.freshness_policy == FreshnessPolicy::AllowStale => {
                if !active_index_matches_request(&store, &status, &request.repository).await? {
                    return Err(error);
                }
                let Some(stale_status) =
                    latest_compatible_code_scope_status(&store, &request.repository).await?
                else {
                    return Err(error);
                };
                let Some(last_indexed_commit) = stale_status.last_indexed_commit.clone() else {
                    return Err(error);
                };
                request.repository.ref_selector = last_indexed_commit;
                served_stale_scope = true;
                stale_reason = Some(
                    "requested ref is not indexed yet; served last completed code index".to_owned(),
                );
                stale_status
            }
            Err(error) => return Err(error),
        };
        if request.freshness_policy == FreshnessPolicy::WaitUntilFresh && scoped_status.stale {
            return Err(ApiError::invalid_argument(format!(
                "code repository '{}' scope '{}' is stale; run repo index or repo update before querying with wait_until_fresh",
                scoped_status.alias,
                scoped_status
                    .last_indexed_scope_id
                    .as_deref()
                    .unwrap_or("unscoped")
            )));
        }
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let source_scope = indexed_source_scope(&scoped_status)
            .ok_or_else(|| missing_indexed_source_scope_error(&scoped_status))?;
        let mut results = store
            .search_code_scope(source_scope, request.clone())
            .await
            .map_err(storage_api_error)?;
        let fallback_degraded_reason =
            apply_code_grep_fallback(&store, &status, &scoped_status, &request, &mut results)
                .await?;
        let degraded_reason = results
            .iter()
            .find_map(|hit| hit.degraded_reason.clone())
            .or(fallback_degraded_reason)
            .or_else(|| scoped_status.degraded_reason.clone())
            .or_else(|| stale_reason.clone());
        let mut scope = crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &request.repository,
            requested_ref.clone(),
        );
        if served_stale_scope {
            scope.stale = true;
        }
        let mut metadata = ApiMetadata::graph_only(&context, graph_version);
        if served_stale_scope {
            metadata.stale = true;
        }
        let freshness = code_query_freshness_diagnostics(
            &store,
            CodeQueryFreshnessContext {
                base_status: &status,
                scoped_status: &scoped_status,
                request: &request,
                requested_ref,
                requested_resolved_ref,
                freshness_target,
                stale_reason,
                degraded_reason: degraded_reason.clone(),
                results: &results,
                graph_version: graph_version.get(),
            },
        )
        .await?;
        annotate_query_result_staleness(&mut results, &freshness);

        Ok(CodeRepositoryQueryResponse {
            metadata,
            scope,
            freshness,
            request,
            results,
            degraded_reason,
        })
    }

    /// Lists configuration-driven feature flags and their code graph relationships.
    pub async fn query_code_repository_feature_flags(
        &self,
        request: CodeFeatureFlagRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryFeatureFlagsResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        if request.freshness_policy == FreshnessPolicy::GraphOnly {
            let graph_version = store
                .current_graph_version()
                .await
                .map_err(storage_api_error)?;
            let degraded_reason = "graph_only freshness policy selected".to_owned();
            return Ok(CodeRepositoryFeatureFlagsResponse {
                metadata: ApiMetadata::graph_only(&context, graph_version),
                scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                    &status,
                    &request.repository,
                    request.repository.ref_selector.clone(),
                ),
                freshness: crate::api::CodeRepositoryFreshnessDiagnostics::graph_only(
                    graph_version.get(),
                    request.freshness_policy,
                    indexed_source_scope(&status),
                    request.repository.ref_selector.clone(),
                    degraded_reason.clone(),
                ),
                request,
                flags: Vec::new(),
                degraded_reason: Some(degraded_reason),
            });
        }
        let requested_ref = request.repository.ref_selector.clone();
        let mut request = feature_flag_request_at_indexed_ref(request, &status).await?;
        if requested_ref == "worktree" {
            ensure_worktree_overlay_matches_current_worktree(&store, &status, &request.repository)
                .await?;
        }
        let requested_resolved_ref = request.repository.ref_selector.clone();
        let freshness_target = request.repository.clone();
        let mut served_stale_scope = false;
        let mut stale_reason = None;
        let scoped_status = match resolved_code_scope_status(&store, &status, &request.repository)
            .await
        {
            Ok(scoped_status) => scoped_status,
            Err(error) if request.freshness_policy == FreshnessPolicy::AllowStale => {
                if !active_index_matches_request(&store, &status, &request.repository).await? {
                    return Err(error);
                }
                let Some(stale_status) =
                    latest_compatible_code_scope_status(&store, &request.repository).await?
                else {
                    return Err(error);
                };
                let Some(last_indexed_commit) = stale_status.last_indexed_commit.clone() else {
                    return Err(error);
                };
                request.repository.ref_selector = last_indexed_commit;
                served_stale_scope = true;
                stale_reason = Some(
                    "requested ref is not indexed yet; served last completed code index".to_owned(),
                );
                stale_status
            }
            Err(error) => return Err(error),
        };
        if request.freshness_policy == FreshnessPolicy::WaitUntilFresh && scoped_status.stale {
            return Err(ApiError::invalid_argument(format!(
                "code repository '{}' scope '{}' is stale; run repo index or repo update before querying feature flags with wait_until_fresh",
                scoped_status.alias,
                scoped_status
                    .last_indexed_scope_id
                    .as_deref()
                    .unwrap_or("unscoped")
            )));
        }
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let source_scope = indexed_source_scope(&scoped_status)
            .ok_or_else(|| missing_indexed_source_scope_error(&scoped_status))?;
        let flags = store
            .search_code_feature_flags_scope(source_scope, request.clone())
            .await
            .map_err(storage_api_error)?;
        let mut scope = crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &request.repository,
            requested_ref.clone(),
        );
        if served_stale_scope {
            scope.stale = true;
        }
        let degraded_reason = scoped_status
            .degraded_reason
            .clone()
            .or_else(|| stale_reason.clone());
        let mut metadata = ApiMetadata::graph_only(&context, graph_version);
        if served_stale_scope {
            metadata.stale = true;
        }
        let freshness = code_feature_flag_freshness_diagnostics(
            &store,
            CodeFeatureFlagFreshnessContext {
                base_status: &status,
                scoped_status: &scoped_status,
                request: &request,
                requested_ref,
                requested_resolved_ref,
                freshness_target,
                stale_reason,
                degraded_reason: degraded_reason.clone(),
                flags: &flags,
                graph_version: graph_version.get(),
            },
        )
        .await?;

        Ok(CodeRepositoryFeatureFlagsResponse {
            metadata,
            scope,
            freshness,
            request,
            flags,
            degraded_reason,
        })
    }
}
