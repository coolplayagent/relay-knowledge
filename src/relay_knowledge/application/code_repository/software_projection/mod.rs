//! Coordinates repository software-projection reads and scope validation.

use crate::{
    api::{
        ApiError, ApiMetadata, RequestContext, SoftwareGlobalExportResponse, SoftwareGlobalResponse,
    },
    application::service::RelayKnowledgeService,
    domain::{
        CodeRepositoryStatus, FreshnessPolicy, GraphVersion, SoftwareExportProfile,
        SoftwareGlobalRequest, SoftwareGlobalStatus,
    },
};

mod export;

use super::{
    errors::storage_api_error,
    repository::{ensure_worktree_overlay_matches_current_worktree, required_code_repository},
    scope::{
        active_index_matches_request, indexed_commit_for_selector,
        latest_compatible_code_scope_status, resolved_code_scope_status,
    },
};

impl RelayKnowledgeService {
    /// Exports the snapshot-bound ontology through a versioned interoperability profile.
    pub async fn software_global_export(
        &self,
        mut request: SoftwareGlobalRequest,
        profile: SoftwareExportProfile,
        context: RequestContext,
    ) -> Result<SoftwareGlobalExportResponse, ApiError> {
        request.kind = crate::domain::SoftwareGlobalKind::Statements;
        let response = self.software_global_projection(request, context).await?;
        let document = export::export_document(&response, profile);
        Ok(SoftwareGlobalExportResponse {
            metadata: response.metadata,
            scope: response.scope,
            status: response.status,
            profile,
            media_type: profile.media_type().to_owned(),
            document,
        })
    }

    /// Reads the repository-scoped software global dependency and SDK projection.
    pub async fn software_global_projection(
        &self,
        request: SoftwareGlobalRequest,
        context: RequestContext,
    ) -> Result<SoftwareGlobalResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status =
            required_code_repository(store.as_ref(), &request.repository.repository).await?;
        if request.freshness_policy == FreshnessPolicy::GraphOnly {
            let graph_version = store
                .current_graph_version()
                .await
                .map_err(storage_api_error)?;
            return Ok(SoftwareGlobalResponse {
                metadata: ApiMetadata::graph_only(&context, graph_version),
                scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                    &status,
                    &request.repository,
                    request.repository.ref_selector.clone(),
                ),
                request,
                status: SoftwareGlobalStatus {
                    repository_id: status.repository_id.clone(),
                    source_scope: status
                        .last_indexed_scope_id
                        .clone()
                        .unwrap_or_else(|| "unscoped".to_owned()),
                    projected_graph_version: GraphVersion::ZERO,
                    stale: true,
                    ontology_version: crate::domain::SOFTWARE_ONTOLOGY_VERSION.to_owned(),
                    projection_schema_version: crate::domain::SOFTWARE_PROJECTION_SCHEMA_VERSION,
                    source_coverage: crate::domain::SoftwareSourceCoverage::default(),
                    completeness_basis_points: 0,
                    freshness: crate::domain::SoftwareProjectionFreshness::Stale,
                    conflict_count: 0,
                    entity_count: 0,
                    statement_count: 0,
                    diagnostic_count: 0,
                    component_count: 0,
                    sdk_usage_count: 0,
                    file_count: 0,
                    topic_count: 0,
                    relationship_count: 0,
                    build_target_count: 0,
                    iac_resource_count: 0,
                    design_element_count: 0,
                    last_error: Some("graph_only freshness policy selected".to_owned()),
                },
                components: Vec::new(),
                dependency_usages: Vec::new(),
                sdk_usages: Vec::new(),
                files: Vec::new(),
                topics: Vec::new(),
                relationships: Vec::new(),
                build_targets: Vec::new(),
                iac_resources: Vec::new(),
                design_elements: Vec::new(),
                entities: Vec::new(),
                statements: Vec::new(),
                diagnostics: Vec::new(),
            });
        }

        let requested_ref = request.repository.ref_selector.clone();
        let mut request = software_request_at_indexed_ref(request, &status).await?;
        if requested_ref == "worktree" {
            ensure_worktree_overlay_matches_current_worktree(&store, &status, &request.repository)
                .await?;
        }
        let mut served_stale_scope = false;
        let scoped_status =
            match resolved_code_scope_status(&store, &status, &request.repository).await {
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
                    stale_status
                }
                Err(error) => return Err(error),
            };
        if let Some(last_indexed_commit) = scoped_status.last_indexed_commit.clone() {
            request.repository.ref_selector = last_indexed_commit;
        }
        request.repository.repository = status.repository_id.clone();

        let source_scope = scoped_status.last_indexed_scope_id.clone().ok_or_else(|| {
            ApiError::invalid_argument(format!(
                "code repository '{}' does not have an indexed source scope",
                scoped_status.alias
            ))
        })?;
        let projection = store
            .software_global_projection_for_scope(source_scope, request.clone())
            .await
            .map_err(storage_api_error)?;
        if request.freshness_policy == FreshnessPolicy::WaitUntilFresh
            && (projection.status.stale || scoped_status.stale)
        {
            return Err(ApiError::invalid_argument(format!(
                "software global projection for repository '{}' scope '{}' is stale; run repo index before querying with wait_until_fresh",
                status.alias, projection.status.source_scope
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

        let mut scope_selector = request.repository.clone();
        scope_selector.path_filters = scoped_status.path_filters.clone();
        scope_selector.language_filters = scoped_status.language_filters.clone();
        let mut scope = crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &scope_selector,
            requested_ref,
        );
        if served_stale_scope {
            scope.stale = true;
        }

        let mut status = projection.status;
        if scoped_status.stale || served_stale_scope {
            status.stale = true;
            status.freshness = crate::domain::SoftwareProjectionFreshness::Stale;
        } else if scoped_status.degraded_reason.is_some() {
            status.freshness = crate::domain::SoftwareProjectionFreshness::Degraded;
        }

        Ok(SoftwareGlobalResponse {
            metadata,
            scope,
            request,
            status,
            components: projection.components,
            dependency_usages: projection.dependency_usages,
            sdk_usages: projection.sdk_usages,
            files: projection.files,
            topics: projection.topics,
            relationships: projection.relationships,
            build_targets: projection.build_targets,
            iac_resources: projection.iac_resources,
            design_elements: projection.design_elements,
            entities: projection.entities,
            statements: projection.statements,
            diagnostics: projection.diagnostics,
        })
    }
}

async fn software_request_at_indexed_ref(
    mut request: SoftwareGlobalRequest,
    status: &CodeRepositoryStatus,
) -> Result<SoftwareGlobalRequest, ApiError> {
    request.repository.ref_selector = indexed_commit_for_selector(
        status,
        &request.repository,
        request.repository.ref_selector.clone(),
    )
    .await?;

    Ok(request)
}
