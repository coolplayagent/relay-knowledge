use std::{collections::BTreeMap, path::PathBuf};

use crate::{
    api::{ApiError, ApiMetadata, CodeRepositoryImpactResponse, RequestContext},
    code::{
        changed_paths_for_diff_with_filters, changed_paths_for_filesystem_diff,
        deleted_symbol_names_for_diff, partition_changed_paths_for_selector,
    },
    domain::CodeImpactRequest,
    storage::CodeImpactChanges,
};

use crate::application::service::RelayKnowledgeService;

use super::{
    blocking::run_blocking_code,
    errors::storage_api_error,
    repository_status::{registration_from_status, required_code_repository},
    scope::{
        indexed_source_scope, missing_indexed_source_scope_error, resolve_code_ref_for_selector,
        resolved_code_scope_status,
    },
};

impl RelayKnowledgeService {
    /// Returns impact radius for a Git diff using the indexed code graph.
    pub async fn impact_code_repository(
        &self,
        mut request: CodeImpactRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryImpactResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        let head_commit =
            resolve_code_ref_for_selector(&status, &request.repository, request.head_ref.clone())
                .await?;
        request.repository.ref_selector = head_commit.clone();
        let scoped_status =
            resolved_code_scope_status(&store, &status, &request.repository).await?;
        let root = PathBuf::from(status.root_path.clone());
        let base_ref = request.base_ref.clone();
        let head_ref = head_commit.clone();
        let path_filters = scoped_status.path_filters.clone();
        let language_filters = scoped_status.language_filters.clone();
        let base_fingerprints = if base_ref.starts_with("filesystem:") {
            let mut base_selector = request.repository.clone();
            base_selector.ref_selector = base_ref.clone();
            match resolved_code_scope_status(&store, &status, &base_selector).await {
                Ok(base_status) => match base_status.last_indexed_scope_id {
                    Some(source_scope) => Some(
                        store
                            .code_file_fingerprints_for_scope(source_scope)
                            .await
                            .map_err(storage_api_error)?,
                    ),
                    None => None,
                },
                Err(_) => None,
            }
        } else {
            None
        };
        let changed_paths = if let Some(base_fingerprints) = base_fingerprints {
            run_blocking_code(move || {
                let previous_hashes = base_fingerprints
                    .into_iter()
                    .map(|file| (file.path, file.blob_hash))
                    .collect::<BTreeMap<_, _>>();
                changed_paths_for_filesystem_diff(
                    &root,
                    &head_ref,
                    &path_filters,
                    &language_filters,
                    &previous_hashes,
                )
            })
            .await?
        } else {
            run_blocking_code(move || {
                changed_paths_for_diff_with_filters(
                    root,
                    &base_ref,
                    &head_ref,
                    &path_filters,
                    &language_filters,
                )
            })
            .await?
        };
        let registration = registration_from_status(&status);
        let path_groups = {
            let registration = registration.clone();
            let selector = request.repository.clone();
            let changed_paths = changed_paths.clone();
            run_blocking_code(move || {
                partition_changed_paths_for_selector(&registration, &selector, changed_paths)
            })
            .await?
        };
        let selector = request.repository.clone();
        let base_ref = request.base_ref.clone();
        let head_ref = head_commit;
        let deleted_symbol_names = run_blocking_code(move || {
            deleted_symbol_names_for_diff(&registration, &selector, &base_ref, &head_ref)
        })
        .await?;
        let source_scope = indexed_source_scope(&scoped_status)
            .ok_or_else(|| missing_indexed_source_scope_error(&scoped_status))?;
        let results = store
            .analyze_code_impact_scope(
                source_scope,
                request.clone(),
                CodeImpactChanges {
                    paths: changed_paths.clone(),
                    deleted_symbol_names,
                },
            )
            .await
            .map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let scope = crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &request.repository,
            request.head_ref.clone(),
        );

        Ok(CodeRepositoryImpactResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope,
            request,
            path_groups,
            results,
        })
    }
}
