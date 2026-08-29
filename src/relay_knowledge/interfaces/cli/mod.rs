//! CLI adapter for the shared application service.

mod command;
mod files;
mod grammar;
mod knowledge;
pub(crate) mod map;
mod operations;
mod remote;
mod render;
mod repo;
mod repo_set;
mod runtime;
mod service;
mod setup;
mod spec;
mod version;

use crate::{
    api::ServicePlanRequest,
    application::{KnowledgeMapService, ProcessRuntimeConfig},
    domain::{FreshnessPolicy, IndexKind, ProposalState, WorkerKind},
    paths::discover_repository_root,
    project::KNOWLEDGE_MAP_RELATIVE_PATH,
};

pub use command::{CliDiagnostic, CliError};
use render::{render_response, serialize_line};
pub(crate) use runtime::run_command;
pub use runtime::run_with_service;

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
    Map(map::MapCommand),
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
    Repo(repo::RepoCommand),
    RepoSet(repo_set::RepoSetCommand),
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
        profile: setup::SetupProfile,
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
#[deprecated(since = "1.1.14", note = "use bootstrap::cli::run_process")]
pub async fn run<I, S>(args: I) -> Result<String, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let output = legacy_run_process(args, false).await?;
    Ok(output.stdout)
}

/// Rendered stdout/stderr for the process entry point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliProcessOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Runs the CLI command and renders only the command result.
#[deprecated(since = "1.1.14", note = "use bootstrap::cli::run_process")]
pub async fn run_process<I, S>(
    args: I,
    interactive_text_output: bool,
) -> Result<CliProcessOutput, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    legacy_run_process(args, interactive_text_output).await
}

async fn legacy_run_process<I, S>(
    args: I,
    _interactive_text_output: bool,
) -> Result<CliProcessOutput, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let command = CliCommand::parse(args)?;
    let service = match &command.action {
        CliAction::Map(map_command) if map_command.needs_repository_root() => {
            Some(legacy_knowledge_map_service(command.format)?)
        }
        _ => None,
    };
    let stdout = run_command(command, service.as_ref(), ProcessRuntimeConfig::default()).await?;
    Ok(CliProcessOutput {
        stdout,
        stderr: String::new(),
    })
}

fn legacy_knowledge_map_service(format: OutputFormat) -> Result<KnowledgeMapService, CliError> {
    let current = std::env::current_dir().map_err(|error| {
        CliError::invalid_api_argument(
            format!("failed to resolve current directory: {error}"),
            format,
        )
    })?;
    let root = discover_repository_root(&current)
        .map_err(|error| CliError::invalid_api_argument(error.to_string(), format))?
        .ok_or_else(|| {
            CliError::invalid_api_argument(
                format!("failed to find repository root for {KNOWLEDGE_MAP_RELATIVE_PATH}"),
                format,
            )
        })?;
    Ok(KnowledgeMapService::new(root))
}

/// Renders best-effort process-only notices after primary command output is emitted.
pub async fn process_update_notice<I, S>(args: I, interactive_text_output: bool) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    update_notice_for_process(args, interactive_text_output).await
}

pub(crate) async fn update_notice_for_process<I, S>(
    args: I,
    interactive_text_output: bool,
) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let command = CliCommand::parse(args).ok()?;
    version::update_notice_for_process(&command, interactive_text_output).await
}

#[cfg(test)]
use service::ensure_web_remote_bind_allowed;

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
#[allow(deprecated)]
#[path = "tests/version.rs"]
mod cli_version_tests;
