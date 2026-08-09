use crate::{
    api::{
        GraphInspectionRequest, HybridRetrievalRequest, IndexRefreshRequest, IngestEvidence,
        IngestRequest, InterfaceKind, RequestContext,
    },
    application::RelayKnowledgeService,
    env::{EnvironmentConfig, RemoteCliEnvironmentConfig},
};

use super::{
    super::{
        CliAction, CliCommand, CliError, files, map, operations, remote,
        render::{render_project_status, render_response},
        repo, repo_set, service, setup, spec, version,
    },
    selection::{remote_environment_needed, select_remote_base_url},
};

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;

pub(crate) async fn run_command(command: CliCommand) -> Result<String, CliError> {
    if let CliAction::Help { path } = &command.action {
        return spec::render_help(path, command.format);
    }
    if command.action == CliAction::Version {
        return version::render_version(command.format);
    }
    if let CliAction::ServiceRun { mcp, web } = command.action.clone() {
        return service::run_service(mcp, web).await;
    }
    if let CliAction::Map(map_command) = command.action.clone() {
        let context = RequestContext::for_interface(InterfaceKind::Cli);
        return map::run_map(map_command, None, context, command.format).await;
    }

    let context = RequestContext::for_interface(InterfaceKind::Cli);
    if remote_environment_needed(&command) {
        let remote_environment = RemoteCliEnvironmentConfig::from_process()
            .map_err(|error| CliError::RuntimeConfigFailed(error.to_string()))?;
        if let Some(base_url) =
            select_remote_base_url(&command, remote_environment.remote_cli.base_url)
        {
            let remote_output = remote::run_remote(
                &remote_environment.network,
                &base_url,
                &command.action,
                context.clone(),
                command.format,
            )
            .await?;
            if let Some(output) = remote_output {
                return Ok(output);
            }
            return Err(remote_unsupported_error());
        }
    }

    let environment = EnvironmentConfig::from_process()
        .map_err(|error| CliError::RuntimeConfigFailed(error.to_string()))?;
    if let Some(base_url) =
        select_remote_base_url(&command, environment.remote_cli.base_url.clone())
    {
        let remote_output = remote::run_remote(
            &environment.network,
            &base_url,
            &command.action,
            context.clone(),
            command.format,
        )
        .await?;
        if let Some(output) = remote_output {
            return Ok(output);
        }
        return Err(remote_unsupported_error());
    }

    let service = RelayKnowledgeService::from_environment(&environment)
        .await
        .map_err(|error| CliError::RuntimeConfigFailed(error.to_string()))?;

    run_with_service(&service, command, context).await
}

fn remote_unsupported_error() -> CliError {
    CliError::ApiFailed(
        "remote CLI mode supports repo list, repo index, repo scope preview, repo status, repo query, repo graph, repo context, repo feature-flags, repo impact, repo report, repo software, and repo view"
            .to_owned(),
    )
}

/// Runs a parsed CLI command with an already composed service.
pub async fn run_with_service(
    service: &RelayKnowledgeService,
    command: CliCommand,
    context: RequestContext,
) -> Result<String, CliError> {
    let format = command.format;
    if let Some(output) =
        operations::run_operational_action(service, &command.action, context.clone(), format)
            .await?
    {
        return Ok(output);
    }
    if let Some(output) =
        setup::run_setup_action(service, &command.action, context.clone(), format)?
    {
        return Ok(output);
    }
    if let Some(output) =
        files::run_files(service, &command.action, context.clone(), format).await?
    {
        return Ok(output);
    }
    match command.action {
        CliAction::Status => {
            let response = service
                .project_status(context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_project_status(&response, format)
        }
        CliAction::Ingest {
            source_scope,
            content,
            entity_labels,
        } => {
            let response = service
                .ingest(
                    IngestRequest {
                        source_scope,
                        evidence: vec![IngestEvidence {
                            id: None,
                            source_path: None,
                            span: None,
                            confidence: None,
                            status: None,
                            content,
                            entity_labels,
                            extraction: None,
                        }],
                        relations: Vec::new(),
                        claims: Vec::new(),
                        events: Vec::new(),
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "knowledge.ingest",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        CliAction::Query {
            query,
            source_scope,
            limit,
            freshness,
        } => {
            let response = service
                .retrieve_context(
                    HybridRetrievalRequest {
                        query,
                        source_scope,
                        limit,
                        freshness,
                    },
                    context,
                )
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "knowledge.retrieve_context",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        CliAction::GraphInspect => {
            let response = service
                .inspect_graph(GraphInspectionRequest { source_scope: None }, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "graph.inspect",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        CliAction::IndexRefresh { kinds } => {
            let response = service
                .refresh_indexes(IndexRefreshRequest { kinds }, context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "index.refresh",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        CliAction::Map(command) => map::run_map(command, None, context, format).await,
        CliAction::Repo(command) => repo::run_repo(service, command, context, format).await,
        CliAction::RepoSet(command) => {
            repo_set::run_repo_set(service, command, context, format).await
        }
        CliAction::Health => {
            let response = service
                .health(context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "service.health",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        CliAction::ProviderProbe => {
            let response = service
                .probe_embedding_provider(context)
                .await
                .map_err(|error| CliError::api_failed(error, format))?;

            render_response(
                "provider.embedding.probe",
                response.metadata.clone(),
                &response,
                format,
            )
        }
        CliAction::VersionCheck => version::run_version_check(service, format).await,
        CliAction::ServiceRun { .. } => Err(CliError::ServiceRunFailed(
            "service run requires process runtime".to_owned(),
        )),
        CliAction::Help { path } => spec::render_help(&path, format),
        CliAction::WorkerStatus { .. }
        | CliAction::FilesIndex { .. }
        | CliAction::FilesQuery { .. }
        | CliAction::FilesContentQuery { .. }
        | CliAction::WorkerRunOnce { .. }
        | CliAction::ProposalList { .. }
        | CliAction::ProposalShow { .. }
        | CliAction::ProposalAccept { .. }
        | CliAction::ProposalReject { .. }
        | CliAction::ProposalSupersede { .. }
        | CliAction::AuditQuery { .. }
        | CliAction::ServiceStatus
        | CliAction::ServicePlan(_)
        | CliAction::ServiceDefinitionWrite
        | CliAction::ServiceOperatorStatus
        | CliAction::ServiceOperatorPause
        | CliAction::ServiceOperatorResume
        | CliAction::ServiceWorkerRun { .. }
        | CliAction::SetupDoctor
        | CliAction::SetupProfile { .. } => Err(CliError::ApiFailed(
            "operational command was not handled by the service adapter".to_owned(),
        )),
        CliAction::Version => version::render_version(command.format),
    }
}
