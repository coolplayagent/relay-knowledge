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
        CliAction, CliCommand, CliError,
        cli_render::{render_project_status, render_response},
        cli_spec, files_cli, map_cli, ops_cli, remote_cli, repo_cli, repo_set_cli, service_cli,
        setup_cli, version_cli,
    },
    selection::{remote_environment_needed, select_remote_base_url},
};

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;

pub(crate) async fn run_command(command: CliCommand) -> Result<String, CliError> {
    if let CliAction::Help { path } = &command.action {
        return cli_spec::render_help(path, command.format);
    }
    if command.action == CliAction::Version {
        return version_cli::render_version(command.format);
    }
    if let CliAction::ServiceRun { mcp, web } = command.action.clone() {
        return service_cli::run_service(mcp, web).await;
    }
    if let CliAction::Map(map_command) = command.action.clone() {
        let context = RequestContext::for_interface(InterfaceKind::Cli);
        return map_cli::run_map(map_command, None, context, command.format).await;
    }

    let context = RequestContext::for_interface(InterfaceKind::Cli);
    if remote_environment_needed(&command) {
        let remote_environment = RemoteCliEnvironmentConfig::from_process()
            .map_err(|error| CliError::RuntimeConfigFailed(error.to_string()))?;
        if let Some(base_url) =
            select_remote_base_url(&command, remote_environment.remote_cli.base_url)
        {
            let remote_output = remote_cli::run_remote(
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
        let remote_output = remote_cli::run_remote(
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
        "remote CLI mode supports repo index, repo scope preview, repo status, repo query, repo context, repo feature-flags, repo impact, repo report, and repo software"
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
        ops_cli::run_operational_action(service, &command.action, context.clone(), format).await?
    {
        return Ok(output);
    }
    if let Some(output) =
        setup_cli::run_setup_action(service, &command.action, context.clone(), format)?
    {
        return Ok(output);
    }
    if let Some(output) =
        files_cli::run_files(service, &command.action, context.clone(), format).await?
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
        CliAction::Map(command) => map_cli::run_map(command, None, context, format).await,
        CliAction::Repo(command) => repo_cli::run_repo(service, command, context, format).await,
        CliAction::RepoSet(command) => {
            repo_set_cli::run_repo_set(service, command, context, format).await
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
        CliAction::VersionCheck => version_cli::run_version_check(service, format).await,
        CliAction::ServiceRun { .. } => Err(CliError::ServiceRunFailed(
            "service run requires process runtime".to_owned(),
        )),
        CliAction::Help { path } => cli_spec::render_help(&path, format),
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
        CliAction::Version => version_cli::render_version(command.format),
    }
}
