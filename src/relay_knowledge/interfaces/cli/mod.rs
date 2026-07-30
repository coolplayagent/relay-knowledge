//! CLI adapter for the shared application service.

#[path = "grammar.rs"]
mod cli_grammar;
#[path = "render/mod.rs"]
mod cli_render;
#[path = "spec/mod.rs"]
mod cli_spec;
#[path = "command/mod.rs"]
mod command;
#[path = "files.rs"]
mod files_cli;
#[path = "knowledge.rs"]
mod knowledge_cli;
#[path = "map.rs"]
pub(crate) mod map_cli;
#[path = "operations.rs"]
mod ops_cli;
#[path = "remote.rs"]
mod remote_cli;
#[path = "repo/mod.rs"]
mod repo_cli;
#[path = "repo/view.rs"]
pub(crate) mod repo_cli_view;
#[path = "repo_set/mod.rs"]
mod repo_set_cli;
#[path = "service.rs"]
mod service_cli;
#[path = "setup/mod.rs"]
mod setup_cli;
#[path = "version.rs"]
mod version_cli;

use crate::{
    api::{
        GraphInspectionRequest, HybridRetrievalRequest, IndexRefreshRequest, IngestEvidence,
        IngestRequest, InterfaceKind, RequestContext, ServicePlanRequest,
    },
    application::RelayKnowledgeService,
    domain::{FreshnessPolicy, IndexKind, ProposalState, WorkerKind},
    env::{EnvironmentConfig, RemoteCliEnvironmentConfig},
};

use cli_render::{render_project_status, render_response, serialize_line};
pub use command::{CliDiagnostic, CliError};

/// Supported CLI output formats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Markdown,
    StreamingJson,
}

impl OutputFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::StreamingJson => "streaming-json",
        }
    }

    fn is_machine_readable(self) -> bool {
        matches!(self, Self::Json | Self::StreamingJson)
    }
}

/// Parsed CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCommand {
    pub action: CliAction,
    pub format: OutputFormat,
    pub remote_base_url: Option<String>,
    pub help: bool,
}

/// CLI action after global options are removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Status,
    Ingest {
        source_scope: String,
        content: String,
        entity_labels: Vec<String>,
    },
    Query {
        query: String,
        source_scope: Option<String>,
        limit: usize,
        freshness: FreshnessPolicy,
    },
    FilesIndex {
        source_scope: Option<String>,
        roots: Vec<String>,
    },
    FilesQuery {
        query: String,
        source_scope: Option<String>,
        root_id: Option<String>,
        limit: usize,
        freshness: FreshnessPolicy,
    },
    FilesContentQuery {
        query: String,
        source_scope: Option<String>,
        root_id: Option<String>,
        limit: usize,
        freshness: FreshnessPolicy,
    },
    GraphInspect,
    IndexRefresh {
        kinds: Vec<IndexKind>,
    },
    Map(map_cli::MapCommand),
    WorkerStatus {
        kind: Option<WorkerKind>,
    },
    WorkerRunOnce {
        kind: Option<WorkerKind>,
    },
    ProposalList {
        state: Option<ProposalState>,
        limit: usize,
    },
    ProposalShow {
        proposal_id: String,
    },
    ProposalAccept {
        proposal_id: String,
        actor: String,
        reason: Option<String>,
    },
    ProposalReject {
        proposal_id: String,
        actor: String,
        reason: Option<String>,
    },
    ProposalSupersede {
        proposal_id: String,
        actor: String,
        reason: Option<String>,
    },
    AuditQuery {
        operation: Option<String>,
        limit: usize,
    },
    ProviderProbe,
    Repo(repo_cli::RepoCommand),
    RepoSet(repo_set_cli::RepoSetCommand),
    Health,
    ServiceStatus,
    ServicePlan(ServicePlanRequest),
    ServiceDefinitionWrite,
    ServiceOperatorStatus,
    ServiceOperatorPause,
    ServiceOperatorResume,
    ServiceWorkerRun {
        task_id: Option<String>,
    },
    ServiceRun {
        mcp: ServiceMcpTransport,
        web: bool,
    },
    SetupDoctor,
    SetupProfile {
        profile: setup_cli::SetupProfile,
    },
    Version,
    VersionCheck,
    Help {
        path: Vec<String>,
    },
}

/// MCP transport option for foreground service mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceMcpTransport {
    Configured,
    StreamableHttp,
}

/// Runs the CLI command and renders its response.
pub async fn run<I, S>(args: I) -> Result<String, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let output = crate::bootstrap::cli::run_process(args, false).await?;
    Ok(output.stdout)
}

/// Rendered stdout/stderr for the process entry point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliProcessOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Runs the CLI command and renders only the command result.
pub async fn run_process<I, S>(
    args: I,
    interactive_text_output: bool,
) -> Result<CliProcessOutput, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    crate::bootstrap::cli::run_process(args, interactive_text_output).await
}

/// Renders best-effort process-only notices after primary command output is emitted.
pub async fn process_update_notice<I, S>(args: I, interactive_text_output: bool) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let command = CliCommand::parse(args).ok()?;
    version_cli::update_notice_for_process(&command, interactive_text_output).await
}

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
        if let Some(remote) = remote_selection(&command, remote_environment.remote_cli.base_url) {
            let remote_output = remote_cli::run_remote(
                &remote_environment.network,
                &remote.base_url,
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
    if let Some(remote) = remote_selection(&command, environment.remote_cli.base_url.clone()) {
        let remote_output = remote_cli::run_remote(
            &environment.network,
            &remote.base_url,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteSelection {
    base_url: String,
    explicit: bool,
}

fn remote_selection(command: &CliCommand, env_base_url: Option<String>) -> Option<RemoteSelection> {
    if let Some(base_url) = command.remote_base_url.clone() {
        return Some(RemoteSelection {
            base_url,
            explicit: true,
        });
    }
    if remote_cli::supports(&command.action) || remote_cli::blocks_local_fallback(&command.action) {
        return env_base_url.map(|base_url| RemoteSelection {
            base_url,
            explicit: false,
        });
    }

    None
}

fn remote_environment_needed(command: &CliCommand) -> bool {
    command.remote_base_url.is_some()
        || remote_cli::supports(&command.action)
        || remote_cli::blocks_local_fallback(&command.action)
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

#[cfg(test)]
use service_cli::ensure_web_remote_bind_allowed;

#[cfg(test)]
#[path = "tests/naming.rs"]
mod cli_naming_tests;

#[cfg(test)]
#[path = "tests/general.rs"]
mod cli_tests;

#[cfg(test)]
#[path = "tests/parse.rs"]
mod cli_parse_tests;

#[cfg(test)]
#[path = "tests/remote.rs"]
mod remote_cli_tests;

#[cfg(test)]
#[path = "tests/map.rs"]
mod cli_map_tests;

#[cfg(test)]
#[path = "tests/service.rs"]
mod cli_service_tests;

#[cfg(test)]
#[path = "tests/version.rs"]
mod cli_version_tests;
