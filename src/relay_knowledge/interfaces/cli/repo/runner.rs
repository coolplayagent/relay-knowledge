use crate::{
    api::{CodeRepositoryRegisterRequest, CodeRepositoryUpdateRequest, RequestContext},
    application::RelayKnowledgeService,
    domain::{
        BusinessKnowledgeQueryRequest, CodeFeatureFlagRequest, CodeGraphContextRequest,
        CodeImpactRequest, CodeIndexMode, CodeIndexRequest, CodeRetrievalRequest, FreshnessPolicy,
        RepositoryGraphNeighborhoodRequest, SoftwareGlobalRequest,
    },
    interfaces::code_index_mode::{mode_for_index_ref, selector_for_index_request},
};

use super::{
    CliError, OutputFormat, RepoCommand,
    index::{CodeIndexWorkerRunResponse, finish_started_index_task, render_index_worker_response},
    render_response,
    report::render_report_response,
    selector,
};

pub async fn run_repo(
    service: &RelayKnowledgeService,
    command: RepoCommand,
    context: RequestContext,
    format: OutputFormat,
) -> Result<String, CliError> {
    match command {
        RepoCommand::List => {
            let response = service
                .list_indexed_code_repositories(context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.list",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Register {
            root_path,
            alias,
            path_filters,
            language_filters,
        } => {
            let response = service
                .register_code_repository(
                    CodeRepositoryRegisterRequest {
                        root_path,
                        alias,
                        path_filters,
                        language_filters,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.register",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Remove { alias } => {
            let response = service
                .remove_code_repository(alias, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.remove",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Index {
            alias,
            ref_selector,
            dry_run,
            reuse_historical,
        } => {
            let selected_mode = mode_for_index_ref(&ref_selector);
            let mode = if dry_run {
                CodeIndexMode::Full
            } else {
                selected_mode.clone()
            };
            let selector = selector(alias, ref_selector, Vec::new(), Vec::new(), format)?;
            let request = CodeIndexRequest {
                repository: selector_for_index_request(selector.clone(), &selected_mode),
                mode,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::AllowStale,
                reuse_historical,
            };
            if dry_run {
                let response = service
                    .preview_code_repository_scope(request, context)
                    .await
                    .map_err(|error| CliError::api_failed(error, format))?;

                return render_response(
                    "code.repo.scope_preview",
                    response.metadata.clone(),
                    &response,
                    format,
                );
            }
            let worker_context = context.clone();
            let mut response = service
                .start_code_repository_index(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;
            finish_started_index_task(service, &mut response, selector, worker_context, format)
                .await?;

            render_response(
                "code.repo.index",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::IndexReset { alias } => {
            let response = service
                .reset_code_repository_index_tasks(alias, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.index_reset",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::IndexWorker { task_id } => {
            let completed = service
                .run_code_index_task_once(task_id, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;
            let (maintenance_active, maintenance_error) =
                match service.run_code_scope_retention_once().await {
                    Ok(active) => (active, None),
                    Err(error) => (false, Some(error.message)),
                };
            let response = CodeIndexWorkerRunResponse {
                claimed: completed.is_some(),
                task: completed,
                maintenance_active,
                maintenance_error,
            };
            render_index_worker_response(&response, format)
        }
        RepoCommand::ScopePreview {
            alias,
            ref_selector,
        } => {
            let response = service
                .preview_code_repository_scope(
                    CodeIndexRequest {
                        repository: selector(alias, ref_selector, Vec::new(), Vec::new(), format)?,
                        mode: CodeIndexMode::Full,
                        workspace_detection: Default::default(),
                        freshness_policy: FreshnessPolicy::AllowStale,
                        reuse_historical: false,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.scope_preview",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Update {
            alias,
            base_ref,
            head_ref,
        } => {
            let requested_head = head_ref.clone().unwrap_or_else(|| "HEAD".to_owned());
            let update_selector = selector(
                alias.clone(),
                requested_head,
                Vec::new(),
                Vec::new(),
                format,
            )?;
            let worker_context = context.clone();
            let mut response = service
                .start_code_repository_update(
                    CodeRepositoryUpdateRequest {
                        repository: alias,
                        base_ref,
                        head_ref,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;
            finish_started_index_task(
                service,
                &mut response,
                update_selector,
                worker_context,
                format,
            )
            .await?;

            render_response(
                "code.repo.update",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Query {
            alias,
            query,
            kind,
            limit,
            ref_selector,
            path_filters,
            language_filters,
            freshness,
            exclude_generated,
        } => {
            let mut request = CodeRetrievalRequest::new(
                query,
                selector(alias, ref_selector, path_filters, language_filters, format)?,
                kind,
                limit,
                freshness,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            request.exclude_generated = exclude_generated;
            let response = service
                .query_code_repository(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.query",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Graph {
            alias,
            focus_path,
            depth,
            ref_selector,
            path_filters,
            node_limit,
            edge_limit,
        } => {
            let request = RepositoryGraphNeighborhoodRequest::new(
                selector(
                    alias,
                    ref_selector,
                    path_filters,
                    vec!["markdown".to_owned()],
                    format,
                )?,
                focus_path,
                depth,
                node_limit,
                edge_limit,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = service
                .repository_graph_neighborhood(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.graph",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Context {
            alias,
            query,
            limit,
            ref_selector,
            path_filters,
            language_filters,
            freshness,
            max_context_bytes,
            include_code,
            exclude_generated,
        } => {
            let request = CodeGraphContextRequest::new(
                selector(alias, ref_selector, path_filters, language_filters, format)?,
                query,
                limit,
                freshness,
                max_context_bytes,
                include_code,
                exclude_generated,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = service
                .codegraph_context(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.context",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::FeatureFlags {
            alias,
            query,
            limit,
            ref_selector,
            path_filters,
            language_filters,
            freshness,
        } => {
            let request = CodeFeatureFlagRequest::new(
                query,
                selector(alias, ref_selector, path_filters, language_filters, format)?,
                limit,
                freshness,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = service
                .query_code_repository_feature_flags(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.feature_flags",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Impact {
            alias,
            base_ref,
            head_ref,
            limit,
        } => {
            let request = CodeImpactRequest::new(
                selector(alias, head_ref.clone(), Vec::new(), Vec::new(), format)?,
                base_ref,
                head_ref,
                limit,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = service
                .impact_code_repository(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.impact",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Status { alias } => {
            let response = service
                .code_repository_status(
                    selector(alias, "HEAD", Vec::new(), Vec::new(), format)?,
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.status",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Report { alias } => {
            let response = service
                .code_repository_report(
                    selector(alias, "HEAD", Vec::new(), Vec::new(), format)?,
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;
            render_report_response(&response, format)
        }
        RepoCommand::Software {
            alias,
            ref_selector,
            kind,
            freshness,
            limit,
        } => {
            let request = SoftwareGlobalRequest::new(
                selector(alias, ref_selector, Vec::new(), Vec::new(), format)?,
                kind,
                freshness,
                limit,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = service
                .software_global_projection(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "code.repo.software",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::Business {
            alias,
            ref_selector,
            domain,
            query,
            kind,
            freshness,
            limit,
        } => {
            let request = BusinessKnowledgeQueryRequest::new(
                selector(alias, ref_selector, Vec::new(), Vec::new(), format)?,
                domain,
                query,
                kind,
                freshness,
                limit,
            )
            .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?;
            let response = service
                .business_knowledge_query(request, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;
            render_response(
                "code.repo.business",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        RepoCommand::View(command) => {
            super::view::run_view(service, command, context, format).await
        }
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod runner_tests;
